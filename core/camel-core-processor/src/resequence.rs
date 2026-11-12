//! The resequencer EIP (Java `Resequencer` and `StreamResequencer` with `ResequencerEngine`):
//! sends the exchanges to its steps in the order of an expression.
//!
//! * batch mode: collects the exchanges until no new one arrives for the batch timeout (or
//!   the batch size is reached), then sends them sorted by the expression (duplicates removed
//!   unless allowed, optionally in reverse order);
//! * stream mode: the expression is a sequence number; an exchange is sent as soon as the one
//!   before it was sent, else after waiting up to the timeout for the missing ones (or when
//!   the caller waits while the capacity is reached).
//!
//! The exchanges are sent on a tokio task started with the route; the exchange given to the
//! resequencer completes when it is queued (as in Java).

use crate::process_and_capture;
use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedExpression, SharedProcessor};
use camel_support::object_helper::type_coerce_compare;
use camel_util::value::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// The Java class name of `org.apache.camel.processor.resequencer.MessageRejectedException`.
pub const MESSAGE_REJECTED_EXCEPTION: &str =
    "org.apache.camel.processor.resequencer.MessageRejectedException";

/// The configuration of a resequencer.
#[derive(Debug, Clone)]
pub enum ResequenceConfig {
    /// The batch mode (Java `BatchResequencerConfig`).
    Batch {
        /// The maximum number of exchanges of a batch (default 100).
        size: usize,
        /// The time without new exchanges completing a batch (default 1000 millis).
        timeout: Duration,
        /// Whether exchanges with the same expression value are all kept.
        allow_duplicates: bool,
        /// Whether the order is reversed.
        reverse: bool,
        /// Whether exchanges without an expression value are ignored instead of failing.
        ignore_invalid: bool,
    },
    /// The stream mode (Java `StreamResequencerConfig`).
    Stream {
        /// The maximum number of exchanges waiting (default 1000): a new exchange waits for
        /// room.
        capacity: usize,
        /// The time to wait for a missing exchange (default 1000 millis).
        timeout: Duration,
        /// Whether exchanges older than the last sent one fail.
        reject_old: bool,
        /// Whether exchanges without a sequence number are ignored instead of failing.
        ignore_invalid: bool,
    },
}

impl ResequenceConfig {
    /// The default batch configuration.
    pub fn batch() -> Self {
        ResequenceConfig::Batch {
            size: 100,
            timeout: Duration::from_millis(1000),
            allow_duplicates: false,
            reverse: false,
            ignore_invalid: false,
        }
    }

    /// The default stream configuration.
    pub fn stream() -> Self {
        ResequenceConfig::Stream {
            capacity: 1000,
            timeout: Duration::from_millis(1000),
            reject_old: false,
            ignore_invalid: false,
        }
    }
}

/// A waiting exchange of the stream mode, with the time it waits until (`None`: deliverable).
struct Waiting {
    exchange: Exchange,
    until: Option<Instant>,
}

#[derive(Default)]
struct State {
    /// The batch mode: the queued exchanges with their expression values.
    batch: Vec<(Value, Exchange)>,
    /// The stream mode: the exchanges by sequence number.
    stream: BTreeMap<i64, Waiting>,
    /// The stream mode: the last sent sequence number.
    last: Option<i64>,
}

struct Inner {
    expression: SharedExpression,
    config: ResequenceConfig,
    processor: SharedProcessor,
    state: Mutex<State>,
    wake: Notify,
    /// Notified when the stream mode sent exchanges (room for the waiting callers).
    space: Notify,
}

/// The resequencer processor.
pub struct ResequenceProcessor {
    inner: Arc<Inner>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ResequenceProcessor {
    /// Creates the processor sending to the processor of the steps.
    pub fn new(
        expression: SharedExpression,
        config: ResequenceConfig,
        processor: SharedProcessor,
    ) -> Self {
        ResequenceProcessor {
            inner: Arc::new(Inner {
                expression,
                config,
                processor,
                state: Mutex::new(State::default()),
                wake: Notify::new(),
                space: Notify::new(),
            }),
            task: Mutex::new(None),
        }
    }

    fn invalid(exchange: &Exchange, ignore: bool) -> CamelResult<()> {
        if ignore {
            log::debug!("Invalid Exchange. This Exchange will be ignored: {exchange}");
            Ok(())
        } else {
            Err(CamelError::camel_exchange(
                "Exchange is not valid to be used by the BatchProcessor",
                exchange,
            ))
        }
    }

    fn enqueue(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let inner = &self.inner;
        let value = inner.expression.evaluate_mut(exchange)?;
        match &inner.config {
            ResequenceConfig::Batch { ignore_invalid, .. } => {
                if value.is_null() {
                    return Self::invalid(exchange, *ignore_invalid);
                }
                inner
                    .state
                    .lock()
                    .unwrap()
                    .batch
                    .push((value, exchange.clone()));
            }
            ResequenceConfig::Stream {
                timeout,
                reject_old,
                ignore_invalid,
                ..
            } => {
                let Some(n) = i64::from_value(&value).ok().flatten() else {
                    return Self::invalid(exchange, *ignore_invalid);
                };
                let mut s = inner.state.lock().unwrap();
                if *reject_old && s.last.is_some_and(|l| n < l) {
                    return Err(CamelError::camel_exchange(
                        &format!(
                            "rejecting message [{exchange}], it should have been sent before the last delivered message [{}]",
                            s.last.unwrap_or_default()
                        ),
                        exchange,
                    )
                    .with_subtype(MESSAGE_REJECTED_EXCEPTION));
                }
                if s.stream.contains_key(&n) || s.last == Some(n) {
                    // a duplicate (Java keeps the sequence as a set)
                    return Ok(());
                }
                // the successor does not wait any more
                if let Some(next) = s.stream.get_mut(&(n + 1)) {
                    next.until = None;
                }
                let successor_of_last = s.last == Some(n - 1);
                let has_predecessor = s.stream.contains_key(&(n - 1));
                let until = if !successor_of_last && !has_predecessor {
                    Some(Instant::now() + *timeout)
                } else {
                    None
                };
                s.stream.insert(
                    n,
                    Waiting {
                        exchange: exchange.clone(),
                        until,
                    },
                );
            }
        }
        inner.wake.notify_one();
        Ok(())
    }
}

/// Sorts a batch by the expression values (duplicates removed unless allowed).
fn sort_batch(
    mut batch: Vec<(Value, Exchange)>,
    allow_duplicates: bool,
    reverse: bool,
) -> Vec<Exchange> {
    batch.sort_by(|a, b| {
        let o = type_coerce_compare(&a.0, &b.0);
        if reverse {
            o.reverse()
        } else {
            o
        }
    });
    if !allow_duplicates {
        batch.dedup_by(|b, a| type_coerce_compare(&a.0, &b.0).is_eq());
    }
    batch.into_iter().map(|(_, e)| e).collect()
}

async fn send(inner: &Inner, exchanges: Vec<Exchange>) {
    for mut e in exchanges {
        process_and_capture(inner.processor.as_ref(), &mut e).await;
        if let Some(err) = e.exception() {
            log::warn!("Error processing resequenced exchange {e}. Caused by: {err}");
        }
    }
}

async fn run_batch(
    inner: Arc<Inner>,
    size: usize,
    timeout: Duration,
    allow_duplicates: bool,
    reverse: bool,
) {
    loop {
        let notified = inner.wake.notified();
        let timed_out = tokio::select! {
            _ = notified => false,
            _ = tokio::time::sleep(timeout) => true,
        };
        loop {
            let batch = {
                let mut s = inner.state.lock().unwrap();
                if s.batch.is_empty() || (!timed_out && s.batch.len() < size) {
                    break;
                }
                let n = s.batch.len().min(size);
                s.batch.drain(..n).collect::<Vec<_>>()
            };
            send(&inner, sort_batch(batch, allow_duplicates, reverse)).await;
        }
    }
}

async fn run_stream(inner: Arc<Inner>) {
    loop {
        let (ready, wait) = {
            let mut s = inner.state.lock().unwrap();
            let now = Instant::now();
            let mut ready = Vec::new();
            while let Some((&n, first)) = s.stream.iter().next() {
                if first.until.is_some_and(|u| u > now) {
                    break;
                }
                let w = s.stream.remove(&n).expect("the first element");
                s.last = Some(n);
                // the next one does not wait for its predecessor any more
                if let Some(next) = s.stream.get_mut(&(n + 1)) {
                    next.until = None;
                }
                ready.push(w.exchange);
            }
            let wait = s
                .stream
                .values()
                .next()
                .and_then(|w| w.until)
                .map(|u| u.saturating_duration_since(now));
            (ready, wait)
        };
        if !ready.is_empty() {
            inner.space.notify_waiters();
            send(&inner, ready).await;
            continue;
        }
        let notified = inner.wake.notified();
        match wait {
            Some(d) => {
                tokio::select! {
                    _ = notified => {}
                    _ = tokio::time::sleep(d) => {}
                }
            }
            None => notified.await,
        }
    }
}

impl Processor for ResequenceProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.inner.processor.start_processor().await?;
            let mut task = self.task.lock().unwrap();
            if task.is_none() {
                let inner = self.inner.clone();
                *task = Some(tokio::spawn(async move {
                    match inner.config.clone() {
                        ResequenceConfig::Batch {
                            size,
                            timeout,
                            allow_duplicates,
                            reverse,
                            ..
                        } => run_batch(inner, size, timeout, allow_duplicates, reverse).await,
                        ResequenceConfig::Stream { .. } => run_stream(inner).await,
                    }
                }));
            }
            Ok(())
        })
    }

    /// Stops the sending task; a batch in progress is sent first, the waiting exchanges of the
    /// stream mode are kept for a restart (as Java).
    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let task = self.task.lock().unwrap().take();
            if let Some(t) = task {
                t.abort();
                let _ = t.await;
            }
            if let ResequenceConfig::Batch {
                allow_duplicates,
                reverse,
                ..
            } = self.inner.config
            {
                let batch = std::mem::take(&mut self.inner.state.lock().unwrap().batch);
                if !batch.is_empty() {
                    send(&self.inner, sort_batch(batch, allow_duplicates, reverse)).await;
                }
            }
            self.inner.processor.stop_processor().await
        })
    }

    /// Queues the exchange; in stream mode the caller first waits until fewer exchanges than
    /// the capacity are waiting (Java blocks the caller too).
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            if let ResequenceConfig::Stream { capacity, .. } = self.inner.config {
                loop {
                    let space = self.inner.space.notified();
                    if self.inner.state.lock().unwrap().stream.len() < capacity {
                        break;
                    }
                    space.await;
                }
            }
            self.enqueue(exchange)
        })
    }
}
