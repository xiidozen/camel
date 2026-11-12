//! Multicast and splitter (Java `MulticastProcessor` and `Splitter`): each copy of the exchange
//! is processed, in sequence or in parallel (tokio tasks), and the results are aggregated.
//!
//! In parallel, the results are aggregated in the order of the copies, or in the order they
//! complete when streaming; with a timeout, the copies not done in time are given to
//! [`AggregationStrategy::timeout`] and
//! the processing continues without them (they still complete in the background, as in Java).
//!
//! Not ported yet: lazy (streaming) iteration of the split parts, share unit of work, custom
//! thread pools, onPrepare, the aggregation on completion callbacks.

use crate::aggregate::UseLatestAggregationStrategy;
use crate::{continue_processing, process_and_capture};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    CORRELATION_ID, EXCEPTION_CAUGHT, MULTICAST_COMPLETE, MULTICAST_INDEX, SPLIT_COMPLETE,
    SPLIT_INDEX, SPLIT_SIZE,
};
use camel_api::processor::{
    start_processors, stop_processors, AggregationStrategy, BoxFuture, Processor,
    SharedAggregationStrategy, SharedExpression, SharedProcessor,
};
use camel_support::exchange_helper::{copy_results, create_correlated_copy};
use camel_util::value::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// The options of the processing of the copies (Java `MulticastDefinition` options shared by
/// multicast, split and recipient list).
#[derive(Clone, Debug, Default)]
pub struct MulticastConfig {
    /// Whether to stop on the first failed copy.
    pub stop_on_exception: bool,
    /// Whether the copies are processed concurrently.
    pub parallel_processing: bool,
    /// Whether the results are aggregated in the order they complete (in parallel) rather
    /// than in the order of the copies.
    pub streaming: bool,
    /// The total timeout of the parallel processing.
    pub timeout: Option<Duration>,
}

/// How the results of the copies are aggregated.
#[derive(Clone)]
pub(crate) enum Aggregation {
    /// A strategy (none: Java `UseLatestAggregationStrategy`, the default of multicast and
    /// recipient list).
    Strategy(Option<SharedAggregationStrategy>),
    /// Keep the original exchange, propagating the errors of the copies (Java
    /// `UseOriginalAggregationStrategy` of the splitter).
    UseOriginal,
}

impl Aggregation {
    /// Aggregates the result of a copy (Java `doAggregate`).
    fn aggregate(
        &self,
        result: &mut Option<Exchange>,
        sub: Exchange,
        original: &mut Exchange,
    ) -> CamelResult<()> {
        match self {
            Aggregation::UseOriginal => {
                if let Some(e) = sub.exception() {
                    original.set_exception(Some(e.clone()));
                }
                if let Some(c) = sub.property(EXCEPTION_CAUGHT).cloned() {
                    original.set_property(EXCEPTION_CAUGHT, c);
                }
            }
            Aggregation::Strategy(Some(s)) => {
                *result = s.aggregate_with_input(result.take(), Some(sub), original)?;
            }
            // the default of multicast and recipient list (Java MulticastReifier): keeps the
            // latest result, propagating the failures
            Aggregation::Strategy(None) => {
                *result = UseLatestAggregationStrategy.aggregate(result.take(), Some(sub))?;
            }
        }
        Ok(())
    }

    /// Tells the strategy that the copy of the index timed out.
    fn timeout(
        &self,
        result: &mut Option<Exchange>,
        original: &mut Exchange,
        index: usize,
        total: usize,
        timeout: Duration,
    ) {
        if let Aggregation::Strategy(Some(s)) = self {
            let target = match result.as_mut() {
                Some(r) => r,
                None => original,
            };
            s.timeout(
                target,
                index as i32,
                total as i32,
                timeout.as_millis() as i64,
            );
        }
    }
}

/// Processes the copies, in sequence or in parallel, and copies the aggregated result to the
/// original exchange.
pub(crate) async fn process_pairs(
    original: &mut Exchange,
    pairs: Vec<(SharedProcessor, Exchange)>,
    aggregation: &Aggregation,
    config: &MulticastConfig,
) -> CamelResult<()> {
    if config.parallel_processing && tokio::runtime::Handle::try_current().is_ok() {
        process_parallel(original, pairs, aggregation, config).await
    } else {
        process_sequential(original, pairs, aggregation, config.stop_on_exception).await
    }
}

/// Stops on the failed copy (Java: the task callback when not continuing).
fn stop_on_failure(original: &mut Exchange, sub: &Exchange, index: usize) {
    let msg = format!("Multicast processing failed for number {index}");
    if let Some(e) = sub.exception().cloned() {
        let wrapped = CamelError::camel_exchange_cause(&msg, sub, e);
        original.set_exception(Some(wrapped));
    }
    original.set_redelivery_exhausted(sub.exception().is_some() || sub.is_redelivery_exhausted());
}

/// Processes the copies in sequence (Java `MulticastProcessor` sequential processing and
/// `doDone`).
pub(crate) async fn process_sequential(
    original: &mut Exchange,
    pairs: Vec<(SharedProcessor, Exchange)>,
    aggregation: &Aggregation,
    stop_on_exception: bool,
) -> CamelResult<()> {
    let mut result: Option<Exchange> = None;
    for (index, (processor, mut sub)) in pairs.into_iter().enumerate() {
        process_and_capture(processor.as_ref(), &mut sub).await;
        let msg = format!("Multicast processing failed for number {index}");
        if !continue_processing(&sub, &msg) && stop_on_exception {
            stop_on_failure(original, &sub, index);
            return Ok(());
        }
        aggregation.aggregate(&mut result, sub, original)?;
    }
    done(original, result, stop_on_exception, false);
    Ok(())
}

/// Processes the copies in parallel tokio tasks (Java `MulticastReactiveTask` in parallel
/// mode with the `AsyncCompletionService`).
async fn process_parallel(
    original: &mut Exchange,
    pairs: Vec<(SharedProcessor, Exchange)>,
    aggregation: &Aggregation,
    config: &MulticastConfig,
) -> CamelResult<()> {
    let total = pairs.len();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, Exchange)>();
    for (index, (processor, mut sub)) in pairs.into_iter().enumerate() {
        let tx = tx.clone();
        tokio::spawn(async move {
            process_and_capture(processor.as_ref(), &mut sub).await;
            let _ = tx.send((index, sub));
        });
    }
    drop(tx);
    let deadline = config.timeout.map(|t| tokio::time::Instant::now() + t);
    let mut completed: BTreeMap<usize, Exchange> = BTreeMap::new();
    let mut result: Option<Exchange> = None;
    let mut aggregated = 0;
    while aggregated < total {
        let received = match deadline {
            Some(d) => match tokio::time::timeout_at(d, rx.recv()).await {
                Ok(r) => r,
                Err(_) => {
                    // the timeout: aggregate what completed, in order, and tell the strategy
                    // about the others (Java MulticastTask.timeout)
                    let timeout = config.timeout.unwrap_or_default();
                    for index in aggregated..total {
                        match completed.remove(&index) {
                            Some(sub) => aggregation.aggregate(&mut result, sub, original)?,
                            None => {
                                aggregation.timeout(&mut result, original, index, total, timeout)
                            }
                        }
                    }
                    done(original, result, config.stop_on_exception, true);
                    return Ok(());
                }
            },
            None => rx.recv().await,
        };
        let Some((index, sub)) = received else {
            // a task ended without a result (it panicked)
            original.set_exception(Some(CamelError::camel_exchange(
                "Multicast processing ended without the result of a sub task",
                &*original,
            )));
            return Ok(());
        };
        let msg = format!("Multicast processing failed for number {index}");
        if !continue_processing(&sub, &msg) && config.stop_on_exception {
            stop_on_failure(original, &sub, index);
            return Ok(());
        }
        if config.streaming {
            aggregation.aggregate(&mut result, sub, original)?;
            aggregated += 1;
        } else {
            completed.insert(index, sub);
            while let Some(sub) = completed.remove(&aggregated) {
                aggregation.aggregate(&mut result, sub, original)?;
                aggregated += 1;
            }
        }
    }
    done(original, result, config.stop_on_exception, false);
    Ok(())
}

/// Copies the aggregated result to the original exchange (Java `doDone`).
fn done(
    original: &mut Exchange,
    result: Option<Exchange>,
    stop_on_exception: bool,
    force_exhaust: bool,
) {
    if let Some(r) = result {
        if r.exception().is_some() && stop_on_exception {
            original.set_exception(r.exception().cloned());
        } else {
            let correlation = original.remove_property(CORRELATION_ID);
            copy_results(original, &r);
            match correlation {
                Some(c) => original.set_property(CORRELATION_ID, c),
                None => {
                    original.remove_property(CORRELATION_ID);
                }
            }
        }
    }
    if force_exhaust && original.exception().is_some() {
        original.set_redelivery_exhausted(true);
    }
}

/// Sends a copy of the exchange to each processor (the multicast EIP).
pub struct MulticastProcessor {
    processors: Vec<SharedProcessor>,
    strategy: Option<SharedAggregationStrategy>,
    config: MulticastConfig,
}

impl MulticastProcessor {
    /// Creates the processor; without strategy the result of the last copy is used.
    pub fn new(
        processors: Vec<SharedProcessor>,
        strategy: Option<SharedAggregationStrategy>,
        config: MulticastConfig,
    ) -> Self {
        MulticastProcessor {
            processors,
            strategy,
            config,
        }
    }
}

/// The copies of a multicast (or recipient list), with their index properties (Java
/// `MulticastProcessor.updateNewExchange`).
pub(crate) fn multicast_pairs(
    exchange: &Exchange,
    processors: impl IntoIterator<Item = SharedProcessor>,
) -> Vec<(SharedProcessor, Exchange)> {
    let processors: Vec<SharedProcessor> = processors.into_iter().collect();
    let size = processors.len();
    processors
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut sub = create_correlated_copy(exchange);
            sub.set_property(MULTICAST_INDEX, i as i64);
            sub.set_property(MULTICAST_COMPLETE, i + 1 == size);
            (p, sub)
        })
        .collect()
}

impl Processor for MulticastProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors(&self.processors).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(&self.processors).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let pairs = multicast_pairs(exchange, self.processors.iter().cloned());
            process_pairs(
                exchange,
                pairs,
                &Aggregation::Strategy(self.strategy.clone()),
                &self.config,
            )
            .await
        })
    }
}

/// The items to split a value into (Java `ObjectHelper.createIterator` with the delimiter):
/// a list, the parts of a text separated by the delimiter (empty parts skipped; `false` does
/// not split), the entries of a map ([`camel_util::value::MapEntry`]), or the value itself.
pub fn split_items(value: Value, delimiter: &str) -> Vec<Value> {
    match value {
        Value::Null => vec![],
        Value::List(l) => l,
        // the entries of a map (Java Map.entrySet())
        Value::Map(m) => m
            .into_iter()
            .map(|(key, value)| camel_util::value::MapEntry { key, value }.into_value())
            .collect(),
        Value::Text(t) => {
            // the delimiter false turns the splitting of texts off
            if delimiter.is_empty()
                || delimiter.eq_ignore_ascii_case("false")
                || !t.contains(delimiter)
            {
                if t.is_empty() {
                    vec![]
                } else {
                    vec![Value::Text(t)]
                }
            } else {
                t.split(delimiter)
                    .filter(|p| !p.is_empty())
                    .map(Value::from)
                    .collect()
            }
        }
        other => vec![other],
    }
}

/// Splits the message into parts and processes each part (the split EIP).
pub struct SplitProcessor {
    expression: SharedExpression,
    processor: SharedProcessor,
    strategy: Option<SharedAggregationStrategy>,
    config: MulticastConfig,
    delimiter: String,
}

impl SplitProcessor {
    /// Creates the processor; without strategy the original exchange is kept (with the errors
    /// of the parts).
    pub fn new(
        expression: SharedExpression,
        processor: SharedProcessor,
        strategy: Option<SharedAggregationStrategy>,
        config: MulticastConfig,
    ) -> Self {
        SplitProcessor {
            expression,
            processor,
            strategy,
            config,
            delimiter: ",".to_string(),
        }
    }

    /// Sets the delimiter of texts (default `,`).
    pub fn with_delimiter(mut self, delimiter: &str) -> Self {
        self.delimiter = delimiter.to_string();
        self
    }
}

impl Processor for SplitProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors([&self.processor]).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors([&self.processor]).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let value = match self.expression.evaluate_mut(exchange) {
                Ok(v) => v,
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            let items = split_items(value, &self.delimiter);
            let size = items.len();
            let streaming = self.config.streaming;
            let pairs: Vec<(SharedProcessor, Exchange)> = items
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    let mut sub = create_correlated_copy(exchange);
                    sub.set_out(None);
                    sub.message_mut().set_body(item);
                    sub.set_property(SPLIT_INDEX, i as i64);
                    // when streaming, the size is only known (and set) on the last part
                    if !streaming || i + 1 == size {
                        sub.set_property(SPLIT_SIZE, size as i64);
                    }
                    sub.set_property(SPLIT_COMPLETE, i + 1 == size);
                    (self.processor.clone(), sub)
                })
                .collect();
            let aggregation = match &self.strategy {
                Some(s) => Aggregation::Strategy(Some(s.clone())),
                None => Aggregation::UseOriginal,
            };
            process_pairs(exchange, pairs, &aggregation, &self.config).await
        })
    }
}

/// Delays the exchange by the number of millis of the expression (the delay EIP); a `Null`
/// or not positive delay does not wait.
pub struct DelayProcessor {
    expression: SharedExpression,
}

impl DelayProcessor {
    /// Creates the processor.
    pub fn new(expression: SharedExpression) -> Self {
        DelayProcessor { expression }
    }
}

impl Processor for DelayProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let v = self.expression.evaluate_mut(exchange)?;
            let delay = <i64 as camel_api::convert::FromValue>::from_value(&v)?.unwrap_or(0);
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
            }
            Ok(())
        })
    }
}
