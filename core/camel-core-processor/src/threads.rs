//! The threads EIP (Java `ThreadsProcessor` with the thread pool of `ThreadsReifier`): the
//! steps after `threads()` run on another tokio task of a bounded pool.
//!
//! At most `max_pool_size` exchanges are routed at a time; the next ones wait for room, up to
//! `max_queue_size` of them (`-1`: no limit, `0`: none); when the pool is full the exchange is
//! rejected: with the `CallerRuns` policy (and `caller_runs_when_rejected`) the caller routes
//! it itself, otherwise it fails with a `RejectedExecutionException`.
//!
//! The caller waits for the routing of the exchange to complete (the Rust routing engine
//! completes the exchange when its future completes; Java releases the calling thread and
//! completes the exchange from the pool thread, so a consumer can take the next message
//! meanwhile).

use crate::process_and_capture;
use camel_api::enums::ThreadPoolRejectedPolicy;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// The configuration of the pool of a `threads` EIP.
#[derive(Debug, Clone)]
pub struct ThreadsConfig {
    /// The number of tasks kept (default 10).
    pub pool_size: i32,
    /// The maximum number of exchanges routed at a time (default 20).
    pub max_pool_size: i32,
    /// The maximum number of exchanges waiting (default 1000, `-1` unbounded).
    pub max_queue_size: i32,
    /// The name of the pool (default `Threads`).
    pub thread_name: String,
    /// What happens to an exchange when the pool is full (default `CallerRuns`).
    pub rejected_policy: ThreadPoolRejectedPolicy,
    /// Whether the caller routes a rejected exchange with the `CallerRuns` policy (default
    /// `true`).
    pub caller_runs_when_rejected: bool,
}

impl Default for ThreadsConfig {
    fn default() -> Self {
        ThreadsConfig {
            pool_size: 10,
            max_pool_size: 20,
            max_queue_size: 1000,
            thread_name: "Threads".into(),
            rejected_policy: ThreadPoolRejectedPolicy::CallerRuns,
            caller_runs_when_rejected: true,
        }
    }
}

impl ThreadsConfig {
    /// Validates the sizes (Java `ThreadPoolBuilder`).
    ///
    /// # Errors
    ///
    /// An `IllegalArgumentException` for a negative pool size or a max pool size below it.
    pub fn validate(&self) -> CamelResult<()> {
        if self.pool_size < 0 {
            return Err(CamelError::illegal_argument(format!(
                "CorePoolSize must be >= 0, was {}",
                self.pool_size
            )));
        }
        if self.max_pool_size < self.pool_size {
            return Err(CamelError::illegal_argument(format!(
                "MaxPoolSize must be >= corePoolSize, was {} >= {}",
                self.max_pool_size, self.pool_size
            )));
        }
        if self.max_pool_size == 0 {
            return Err(CamelError::illegal_argument(
                "MaxPoolSize must be > 0, was 0",
            ));
        }
        Ok(())
    }
}

/// The processor of the `threads` EIP, routing the exchanges to the following steps on the
/// pool.
pub struct ThreadsProcessor {
    config: ThreadsConfig,
    processor: SharedProcessor,
    permits: Arc<Semaphore>,
    waiting: AtomicI64,
    running: AtomicBool,
}

impl ThreadsProcessor {
    /// Creates the processor of the pool, routing to the processor (the following steps).
    ///
    /// # Errors
    ///
    /// An invalid configuration (see [`ThreadsConfig::validate`]).
    pub fn new(config: ThreadsConfig, processor: SharedProcessor) -> CamelResult<Self> {
        config.validate()?;
        Ok(ThreadsProcessor {
            permits: Arc::new(Semaphore::new(config.max_pool_size as usize)),
            config,
            processor,
            waiting: AtomicI64::new(0),
            running: AtomicBool::new(false),
        })
    }

    /// The configuration of the pool.
    pub fn config(&self) -> &ThreadsConfig {
        &self.config
    }

    fn rejected(&self, exchange: &mut Exchange) {
        log::trace!("Rejected routing exchange {exchange}");
        exchange.set_exception(Some(CamelError::new(ErrorKind::RejectedExecution, "")));
    }

    /// Routes the exchange on a task of the pool, holding the permit.
    async fn route_on_task(
        &self,
        permit: tokio::sync::OwnedSemaphorePermit,
        exchange: &mut Exchange,
    ) -> CamelResult<()> {
        let processor = self.processor.clone();
        let mut task_exchange = exchange.clone();
        let handle = tokio::spawn(async move {
            let _permit = permit;
            process_and_capture(processor.as_ref(), &mut task_exchange).await;
            task_exchange
        });
        match handle.await {
            Ok(routed) => {
                *exchange = routed;
                Ok(())
            }
            Err(e) => Err(CamelError::new(
                ErrorKind::Runtime,
                format!("The task of {} failed: {e}", self.config.thread_name),
            )),
        }
    }
}

impl Processor for ThreadsProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.processor.start_processor().await?;
            self.running.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.running.store(false, Ordering::SeqCst);
            self.processor.stop_processor().await
        })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            if !self.running.load(Ordering::SeqCst) {
                return Err(CamelError::illegal_state(
                    "ThreadsProcessor is not running.",
                ));
            }
            if exchange.is_transacted() {
                // a transaction stays on the task of the caller (as Java)
                return self.processor.process(exchange).await;
            }
            if let Ok(permit) = self.permits.clone().try_acquire_owned() {
                return self.route_on_task(permit, exchange).await;
            }
            let max_queue = i64::from(self.config.max_queue_size);
            let queued = self.waiting.fetch_add(1, Ordering::SeqCst);
            if max_queue < 0 || queued < max_queue {
                let permit = self.permits.clone().acquire_owned().await;
                self.waiting.fetch_sub(1, Ordering::SeqCst);
                let permit = permit
                    .map_err(|_| CamelError::illegal_state("ThreadsProcessor is not running."))?;
                return self.route_on_task(permit, exchange).await;
            }
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            let caller_runs = self.config.rejected_policy == ThreadPoolRejectedPolicy::CallerRuns
                && self.config.caller_runs_when_rejected;
            if caller_runs {
                self.processor.process(exchange).await
            } else {
                self.rejected(exchange);
                Ok(())
            }
        })
    }
}
