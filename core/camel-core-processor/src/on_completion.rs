//! The onCompletion EIP (Java `OnCompletionProcessor`): routes a copy of the exchange when its
//! unit of work is done.

use crate::unit_of_work::UnitOfWorkProcessor;
use camel_api::enums::ExchangePattern;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{EXCEPTION_CAUGHT, ON_COMPLETION, ON_COMPLETION_ROUTE_IDS};
use camel_api::message::Message;
use camel_api::processor::{BoxFuture, Processor, SharedPredicate, SharedProcessor};
use camel_api::spi::synchronization::{Synchronization, ORDER_LOWEST};
use camel_support::exchange_helper::create_correlated_copy;
use camel_util::value::Value;
use std::future::ready;
use std::sync::Arc;

/// The options of an onCompletion.
#[derive(Clone, Default)]
pub struct OnCompletionOptions {
    /// Only when the exchange completed successfully.
    pub on_complete_only: bool,
    /// Only when the exchange failed.
    pub on_failure_only: bool,
    /// Only when the exchange matches.
    pub on_when: Option<SharedPredicate>,
    /// Routes the original in message instead of the current one.
    pub use_original_message: bool,
    /// Runs when the route is done, before its consumer continues (Java mode
    /// `BeforeConsumer`), instead of when the unit of work is done (`AfterConsumer`).
    pub before_consumer: bool,
    /// Whether the onCompletion is defined in a route (it then runs for that route only)
    /// rather than globally.
    pub route_scoped: bool,
    /// Routes the copy in a background task.
    pub parallel_processing: bool,
}

struct Shared {
    processor: SharedProcessor,
    options: OnCompletionOptions,
    route_id: String,
}

/// Registers the onCompletion callback on each exchange routed through it (the onCompletion
/// processor is the first of its route).
pub struct OnCompletionProcessor {
    shared: Arc<Shared>,
}

impl OnCompletionProcessor {
    /// Creates the processor of the route: the steps run in their own unit of work.
    pub fn new(route_id: &str, steps: SharedProcessor, options: OnCompletionOptions) -> Self {
        let processor: SharedProcessor = Arc::new(UnitOfWorkProcessor::new(None, steps));
        OnCompletionProcessor {
            shared: Arc::new(Shared {
                processor,
                options,
                route_id: route_id.to_string(),
            }),
        }
    }
}

impl Processor for OnCompletionProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.shared.processor.start_processor()
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.shared.processor.stop_processor()
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let s: Arc<dyn Synchronization> = if self.shared.options.before_consumer {
            Arc::new(BeforeConsumer(self.shared.clone()))
        } else {
            Arc::new(AfterConsumer(self.shared.clone()))
        };
        exchange.add_on_completion(s);
        Box::pin(ready(Ok(())))
    }
}

impl Shared {
    /// Whether a copy is routed: in parallel or after the consumer (the unit of work of the
    /// exchange is done then); before the consumer the exchange itself is routed (Java
    /// `isCreateCopy`).
    fn create_copy(&self) -> bool {
        self.options.parallel_processing || !self.options.before_consumer
    }

    /// Prepares the exchange to route (Java `prepareExchange`).
    fn prepare(&self, answer: &mut Exchange, original: Option<Message>) {
        if let Some(original) = original {
            answer.set_in(original);
        }
        answer.set_property(ON_COMPLETION, true);
    }

    /// Routes the exchange, or a copy (see [`create_copy`](Shared::create_copy)).
    async fn fire(self: &Arc<Self>, exchange: &mut Exchange) {
        let original = if self.options.use_original_message {
            exchange.original_message().cloned()
        } else {
            None
        };
        if self.create_copy() {
            let mut copy = create_correlated_copy(exchange);
            if let Some(out) = copy.out_message().cloned() {
                copy.set_in(out);
                copy.set_out(None);
            }
            copy.set_pattern(ExchangePattern::InOnly);
            self.prepare(&mut copy, original);
            if self.options.parallel_processing && tokio::runtime::Handle::try_current().is_ok() {
                let me = self.clone();
                tokio::spawn(async move { me.do_process(&mut copy).await });
            } else {
                self.do_process(&mut copy).await;
            }
        } else {
            self.prepare(exchange, original);
            self.do_process(exchange).await;
        }
    }

    /// Routes the exchange (Java `doProcess`): the routing state of a failed exchange is reset
    /// while the onCompletion runs, so its own errors are handled, and restored after.
    async fn do_process(&self, exchange: &mut Exchange) {
        let stop = exchange.is_route_stop();
        exchange.set_route_stop(false);
        let failure_handled = exchange.is_failure_handled();
        exchange.set_failure_handled(false);
        let error_handler_handled = exchange.error_handler_handled();
        exchange.set_error_handler_handled(None);
        let caught = exchange.property(EXCEPTION_CAUGHT).cloned();
        let rollback_only = exchange.is_rollback_only();
        exchange.set_rollback_only(false);
        let rollback_only_last = exchange.is_rollback_only_last();
        exchange.set_rollback_only_last(false);
        let exhausted = exchange.is_redelivery_exhausted();
        exchange.set_redelivery_exhausted(false);
        let cause = exchange.exception().cloned();
        exchange.set_exception(None);
        if let Err(e) = self.processor.process(exchange).await {
            exchange.set_exception(Some(e));
        }
        exchange.set_route_stop(stop);
        let new_failure = cause.is_none() && exchange.exception().is_some();
        if new_failure {
            if failure_handled {
                exchange.set_failure_handled(true);
            }
            if error_handler_handled.is_some() {
                exchange.set_error_handler_handled(error_handler_handled);
            }
            if let Some(e) = exchange.exception() {
                log::warn!("Error processing onCompletion: {exchange}. Caused by: {e}");
            }
        } else {
            exchange.set_failure_handled(failure_handled);
            exchange.set_error_handler_handled(error_handler_handled);
            match caught {
                Some(c) => exchange.set_property(EXCEPTION_CAUGHT, c),
                None => {
                    exchange.remove_property(EXCEPTION_CAUGHT);
                }
            }
        }
        exchange.set_rollback_only(rollback_only);
        exchange.set_rollback_only_last(rollback_only_last);
        exchange.set_redelivery_exhausted(exhausted);
        if let Some(cause) = cause {
            // an error of the onCompletion is suppressed by the original one
            exchange.set_exception(Some(cause));
        }
    }

    fn matches_when(&self, exchange: &Exchange) -> bool {
        self.options
            .on_when
            .as_ref()
            .map_or(true, |p| p.matches(exchange).unwrap_or(false))
    }
}

fn route_ids(exchange: &Exchange) -> Vec<Value> {
    match exchange.property(ON_COMPLETION_ROUTE_IDS) {
        Some(Value::List(l)) => l.clone(),
        _ => Vec::new(),
    }
}

/// Runs when the unit of work is done (Java `OnCompletionSynchronizationAfterConsumer`).
struct AfterConsumer(Arc<Shared>);

impl AfterConsumer {
    fn should_skip(&self, exchange: &Exchange, only_other: bool) -> bool {
        let s = &self.0;
        let current_route = exchange.from_route_id();
        if !s.options.route_scoped && current_route.is_some_and(|r| r != s.route_id) {
            return true;
        }
        if s.options.route_scoped
            && !route_ids(exchange).contains(&Value::from(s.route_id.as_str()))
        {
            return true;
        }
        only_other || !s.matches_when(exchange)
    }
}

impl Synchronization for AfterConsumer {
    fn order(&self) -> i32 {
        ORDER_LOWEST - 1
    }

    /// Records that the route of a route scoped onCompletion was done.
    fn on_after_route<'a>(
        &'a self,
        route_id: &'a str,
        exchange: &'a mut Exchange,
    ) -> BoxFuture<'a, ()> {
        let s = &self.0;
        if s.options.route_scoped && route_id == s.route_id {
            let mut ids = route_ids(exchange);
            ids.push(Value::from(route_id));
            exchange.set_property(ON_COMPLETION_ROUTE_IDS, Value::List(ids));
        }
        Box::pin(ready(()))
    }

    fn on_complete<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if self.should_skip(exchange, self.0.options.on_failure_only) {
                return;
            }
            self.0.fire(exchange).await;
        })
    }

    fn on_failure<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if self.should_skip(exchange, self.0.options.on_complete_only) {
                return;
            }
            self.0.fire(exchange).await;
        })
    }

    fn before_handover(&self, target: &mut Exchange) {
        let s = &self.0;
        if s.options.route_scoped && target.from_route_id() == Some(s.route_id.as_str()) {
            let mut ids = route_ids(target);
            let id = Value::from(s.route_id.as_str());
            if !ids.contains(&id) {
                ids.push(id);
                target.set_property(ON_COMPLETION_ROUTE_IDS, Value::List(ids));
            }
        }
    }
}

/// Runs when the route is done, before its consumer continues (Java
/// `OnCompletionSynchronizationBeforeConsumer`).
struct BeforeConsumer(Arc<Shared>);

impl Synchronization for BeforeConsumer {
    fn order(&self) -> i32 {
        ORDER_LOWEST
    }

    fn allow_handover(&self) -> bool {
        false
    }

    fn on_after_route<'a>(
        &'a self,
        route_id: &'a str,
        exchange: &'a mut Exchange,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let s = &self.0;
            if route_id != s.route_id {
                return;
            }
            if !s.options.route_scoped && exchange.from_route_id() != Some(s.route_id.as_str()) {
                return;
            }
            let failed = exchange.is_failed();
            if (failed && s.options.on_complete_only)
                || (!failed && s.options.on_failure_only)
                || !s.matches_when(exchange)
            {
                return;
            }
            s.fire(exchange).await;
        })
    }

    fn on_complete<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        Box::pin(ready(()))
    }

    fn on_failure<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        Box::pin(ready(()))
    }
}
