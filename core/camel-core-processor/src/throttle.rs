//! The throttle EIP (Java `TotalRequestsThrottler` and `ConcurrentRequestsThrottler`): limits
//! the number of exchanges per time period (the default mode), or the number of exchanges
//! being processed at the same time, optionally per correlation key. Exchanges over the
//! limit wait for a permit, or fail with a `ThrottlerRejectedExecutionException` when
//! `rejectExecution` is enabled.

use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedExpression};
use camel_api::spi::synchronization::Synchronization;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The Java class name of `org.apache.camel.processor.ThrottlerRejectedExecutionException`.
pub const THROTTLER_REJECTED_EXECUTION_EXCEPTION: &str =
    "org.apache.camel.processor.ThrottlerRejectedExecutionException";

/// The throttling mode (Java `ThrottlingMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottlingMode {
    /// At most the number of exchanges per time period (millis).
    TotalRequests(u64),
    /// At most the number of exchanges processed at the same time.
    ConcurrentRequests,
}

/// The state of one correlation key.
struct KeyState {
    /// The start times of the requests of the current window (total requests).
    window: VecDeque<Instant>,
    /// The permits of the concurrent requests, and their number.
    semaphore: Arc<Semaphore>,
    permits: usize,
}

/// The throttle processor.
pub struct ThrottleProcessor {
    max_requests: SharedExpression,
    mode: ThrottlingMode,
    correlation: Option<SharedExpression>,
    reject_execution: bool,
    states: Mutex<HashMap<String, KeyState>>,
}

impl ThrottleProcessor {
    /// Creates the processor.
    pub fn new(
        max_requests: SharedExpression,
        mode: ThrottlingMode,
        correlation: Option<SharedExpression>,
        reject_execution: bool,
    ) -> Self {
        ThrottleProcessor {
            max_requests,
            mode,
            correlation,
            reject_execution,
            states: Mutex::new(HashMap::new()),
        }
    }

    fn rejected(&self, rate: usize) -> CamelError {
        let message = match self.mode {
            ThrottlingMode::TotalRequests(period) => {
                format!("Exceeded the max throttle rate of {rate} within {period}ms")
            }
            ThrottlingMode::ConcurrentRequests => {
                format!("Exceeded the max throttle rate of {rate}")
            }
        };
        CamelError::new(ErrorKind::RejectedExecution, message)
            .with_subtype(THROTTLER_REJECTED_EXECUTION_EXCEPTION)
    }

    async fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let key = match &self.correlation {
            Some(c) => c.evaluate_mut(exchange)?.to_string(),
            None => String::new(),
        };
        let v = self.max_requests.evaluate_mut(exchange)?;
        let rate = i64::from_value(&v)?.ok_or_else(|| {
            CamelError::illegal_argument(format!(
                "The max requests expression was evaluated as null: {}",
                self.max_requests.describe()
            ))
        })?;
        if rate <= 0 {
            return Err(CamelError::illegal_argument(format!(
                "The maximumRequestsPerPeriod must be a positive number, was: {rate}"
            )));
        }
        let rate = rate as usize;
        match self.mode {
            ThrottlingMode::TotalRequests(period) => self.total(&key, rate, period).await,
            ThrottlingMode::ConcurrentRequests => self.concurrent(exchange, &key, rate).await,
        }
    }

    /// Waits until the window of the period has room for one more request.
    async fn total(&self, key: &str, rate: usize, period: u64) -> CamelResult<()> {
        let period = Duration::from_millis(period);
        loop {
            let wait = {
                let mut states = self.states.lock().unwrap();
                let state = states.entry(key.to_string()).or_insert_with(|| KeyState {
                    window: VecDeque::new(),
                    semaphore: Arc::new(Semaphore::new(rate)),
                    permits: rate,
                });
                let now = Instant::now();
                while state
                    .window
                    .front()
                    .is_some_and(|t| now.duration_since(*t) >= period)
                {
                    state.window.pop_front();
                }
                if state.window.len() < rate {
                    state.window.push_back(now);
                    return Ok(());
                }
                if self.reject_execution {
                    return Err(self.rejected(rate));
                }
                period - now.duration_since(state.window[0])
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// Acquires a permit, released when the exchange completes.
    async fn concurrent(&self, exchange: &mut Exchange, key: &str, rate: usize) -> CamelResult<()> {
        let semaphore = {
            let mut states = self.states.lock().unwrap();
            let state = states.entry(key.to_string()).or_insert_with(|| KeyState {
                window: VecDeque::new(),
                semaphore: Arc::new(Semaphore::new(rate)),
                permits: rate,
            });
            // the rate may change (an expression): adjust the permits
            if rate > state.permits {
                state.semaphore.add_permits(rate - state.permits);
                state.permits = rate;
            } else if rate < state.permits {
                state.semaphore.forget_permits(state.permits - rate);
                state.permits = rate;
            }
            state.semaphore.clone()
        };
        let permit = if self.reject_execution {
            semaphore
                .try_acquire_owned()
                .map_err(|_| self.rejected(rate))?
        } else {
            semaphore
                .acquire_owned()
                .await
                .map_err(|_| CamelError::illegal_state("The throttler is closed"))?
        };
        exchange.add_on_completion(Arc::new(ReleasePermit(Mutex::new(Some(permit)))));
        Ok(())
    }
}

/// Releases the permit of the concurrent throttler when the exchange completes.
struct ReleasePermit(Mutex<Option<OwnedSemaphorePermit>>);

impl Synchronization for ReleasePermit {
    fn on_complete<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().take();
        Box::pin(std::future::ready(()))
    }

    fn on_failure<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        self.0.lock().unwrap().take();
        Box::pin(std::future::ready(()))
    }
}

impl Processor for ThrottleProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(self.run(exchange))
    }
}
