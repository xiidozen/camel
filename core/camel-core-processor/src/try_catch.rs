//! `doTry` / `doCatch` / `doFinally` (Java `TryProcessor`, `CatchProcessor` and
//! `FinallyProcessor`).

use crate::{prepare_out_to_in, process_and_capture};
use camel_api::error::{CamelError, CamelResult, ErrorKind, ErrorType};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    EXCEPTION_CAUGHT, EXCEPTION_HANDLED, FAILURE_ENDPOINT, FAILURE_LOCATION, FAILURE_NODE_ID,
    FAILURE_ROUTE_ID, TO_ENDPOINT, TRY_ROUTE_BLOCK,
};
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedPredicate, SharedProcessor,
};
use camel_api::spi::event::{notify, CamelEvent};
use camel_util::value::Value;
use std::sync::atomic::{AtomicU64, Ordering};

/// The value of the `CamelExceptionCaught` property for the error.
pub fn caught_value(error: &CamelError) -> Value {
    Value::object_display(error.clone())
}

/// The error stored in the `CamelExceptionCaught` property, if any.
pub fn exception_caught(exchange: &Exchange) -> Option<CamelError> {
    exchange
        .property(EXCEPTION_CAUGHT)
        .and_then(|v| v.downcast_ref::<CamelError>())
        .cloned()
}

/// Runs the try block, then the catch clauses and the finally clause, in order (each clause
/// checks whether it applies).
pub struct TryProcessor {
    try_processor: SharedProcessor,
    catches: Vec<SharedProcessor>,
    finally: Option<SharedProcessor>,
}

impl TryProcessor {
    /// Creates the processor.
    pub fn new(
        try_processor: SharedProcessor,
        catches: Vec<SharedProcessor>,
        finally: Option<SharedProcessor>,
    ) -> Self {
        TryProcessor {
            try_processor,
            catches,
            finally,
        }
    }
}

impl TryProcessor {
    /// The try block, the catch and the finally clauses.
    fn all(&self) -> Vec<SharedProcessor> {
        std::iter::once(&self.try_processor)
            .chain(&self.catches)
            .chain(&self.finally)
            .cloned()
            .collect()
    }
}

impl Processor for TryProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors(&self.all()).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(&self.all()).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let last_handled = exchange.property(EXCEPTION_HANDLED).cloned();
            let last_try_route_block = exchange.property(TRY_ROUTE_BLOCK).cloned();
            let saved: Vec<(&str, Option<Value>)> = [
                FAILURE_ENDPOINT,
                FAILURE_ROUTE_ID,
                FAILURE_NODE_ID,
                FAILURE_LOCATION,
            ]
            .iter()
            .map(|k| (*k, exchange.property(k).cloned()))
            .collect();
            exchange.remove_property(EXCEPTION_HANDLED);
            let mut failure_origin_captured = false;
            let processors = std::iter::once(&self.try_processor)
                .chain(self.catches.iter())
                .chain(self.finally.iter());
            for p in processors {
                if exchange.is_route_stop() {
                    break;
                }
                if !failure_origin_captured && exchange.exception().is_some() {
                    failure_origin_captured = true;
                    if let Some(id) = exchange.from_route_id().map(str::to_string) {
                        exchange.set_property(FAILURE_ROUTE_ID, id);
                    }
                }
                exchange.set_property(TRY_ROUTE_BLOCK, true);
                prepare_out_to_in(exchange);
                process_and_capture(p.as_ref(), exchange).await;
            }
            prepare_out_to_in(exchange);
            match last_try_route_block {
                Some(v) => exchange.set_property(TRY_ROUTE_BLOCK, v),
                None => {
                    exchange.remove_property(TRY_ROUTE_BLOCK);
                }
            }
            match last_handled {
                Some(v) => exchange.set_property(EXCEPTION_HANDLED, v),
                None => {
                    exchange.remove_property(EXCEPTION_HANDLED);
                }
            }
            if exchange.exception().is_none() {
                for (k, v) in saved {
                    if let Some(v) = v {
                        exchange.set_property(k, v);
                    }
                }
            }
            Ok(())
        })
    }
}

/// A `doCatch` clause: handles the exception of the exchange if it (or a cause) is one of the
/// types and the optional `onWhen` predicate matches.
pub struct CatchProcessor {
    exceptions: Vec<ErrorType>,
    processor: SharedProcessor,
    on_when: Option<SharedPredicate>,
    matches: AtomicU64,
}

impl CatchProcessor {
    /// Creates the clause.
    pub fn new(
        exceptions: Vec<ErrorType>,
        processor: SharedProcessor,
        on_when: Option<SharedPredicate>,
    ) -> Self {
        CatchProcessor {
            exceptions,
            processor,
            on_when,
            matches: AtomicU64::new(0),
        }
    }

    /// The number of caught exceptions.
    pub fn caught_count(&self) -> u64 {
        self.matches.load(Ordering::Relaxed)
    }

    fn catches(&self, exchange: &Exchange, error: &CamelError) -> CamelResult<bool> {
        for e in camel_util::object_helper::create_exception_iterable(error) {
            if self.exceptions.iter().any(|t| t.matches(e)) {
                let when = match &self.on_when {
                    Some(p) => p.matches(exchange)?,
                    None => true,
                };
                if when {
                    self.matches.fetch_add(1, Ordering::Relaxed);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

impl Processor for CatchProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors([&self.processor]).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors([&self.processor]).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let Some(e) = exchange.exception().cloned() else {
                return Ok(());
            };
            if exchange.property(EXCEPTION_HANDLED).is_some() {
                return Ok(());
            }
            match self.catches(exchange, &e) {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(on_when_error) => {
                    exchange.set_exception(Some(on_when_error));
                    return Ok(());
                }
            }
            let stop = exchange.is_route_stop();
            exchange.set_route_stop(false);
            let rollback_only = exchange.is_rollback_only();
            exchange.set_rollback_only(false);
            let rollback_only_last = exchange.is_rollback_only_last();
            exchange.set_rollback_only_last(false);
            if exchange.property(FAILURE_ENDPOINT).is_none() {
                if let Some(to) = exchange.property(TO_ENDPOINT).cloned() {
                    exchange.set_property(FAILURE_ENDPOINT, to);
                }
            }
            exchange.set_property(EXCEPTION_HANDLED, true);
            exchange.set_property(EXCEPTION_CAUGHT, caught_value(&e));
            exchange.set_exception(None);
            exchange.set_redelivery_exhausted(false);
            let context = exchange.context().clone();
            notify(
                context.as_ref(),
                CamelEvent::ExchangeFailureHandling {
                    exchange,
                    dead_letter: false,
                    dead_letter_uri: None,
                },
            );
            process_and_capture(self.processor.as_ref(), exchange).await;
            notify(
                context.as_ref(),
                CamelEvent::ExchangeFailureHandled {
                    exchange,
                    dead_letter: false,
                    dead_letter_uri: None,
                    handled: false,
                },
            );
            exchange.set_redelivery_exhausted(false);
            if rollback_only || rollback_only_last || stop {
                exchange.set_route_stop(stop);
                exchange.set_rollback_only(rollback_only);
                exchange.set_rollback_only_last(rollback_only_last);
                if e.is(ErrorKind::RollbackExchange) {
                    exchange.set_exception(Some(e));
                }
            }
            Ok(())
        })
    }
}

/// The `doFinally` clause: always runs; an exception of the exchange is kept aside while it
/// runs and set back afterwards.
pub struct FinallyProcessor {
    processor: SharedProcessor,
}

impl FinallyProcessor {
    /// Creates the clause.
    pub fn new(processor: SharedProcessor) -> Self {
        FinallyProcessor { processor }
    }
}

impl Processor for FinallyProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors([&self.processor]).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors([&self.processor]).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let exception = exchange.exception().cloned();
            if let Some(e) = &exception {
                exchange.set_exception(None);
                exchange.set_property(EXCEPTION_CAUGHT, caught_value(e));
                if exchange.property(FAILURE_ENDPOINT).is_none() {
                    if let Some(to) = exchange.property(TO_ENDPOINT).cloned() {
                        exchange.set_property(FAILURE_ENDPOINT, to);
                    }
                }
            }
            process_and_capture(self.processor.as_ref(), exchange).await;
            match exception {
                None => {
                    if exchange.exception().is_none() {
                        for k in [
                            FAILURE_ENDPOINT,
                            FAILURE_ROUTE_ID,
                            FAILURE_NODE_ID,
                            FAILURE_LOCATION,
                        ] {
                            exchange.remove_property(k);
                        }
                    }
                }
                Some(e) => {
                    // the error of the try block wins over an error of the finally block
                    exchange.set_exception(Some(e.clone()));
                    exchange.set_property(EXCEPTION_CAUGHT, caught_value(&e));
                }
            }
            Ok(())
        })
    }
}
