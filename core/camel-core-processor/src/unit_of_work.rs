//! The unit of work around the routing of an exchange (Java `CamelInternalProcessor` with the
//! `UnitOfWorkProcessorAdvice`).

use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedProcessor,
};
use camel_api::spi::synchronization::{after_route_synchronizations, done_synchronizations};

/// Runs the processor in a unit of work: if the exchange has none, one is started, and when
/// the processor is done the on completions of the exchange run (`on_failure` if it failed,
/// else `on_complete`). With a route id, the `on_after_route` callbacks run after the
/// processor too.
///
/// Errors of the processor are set on the exchange (so the on completions see the failure).
pub struct UnitOfWorkProcessor {
    route_id: Option<String>,
    processor: SharedProcessor,
}

impl UnitOfWorkProcessor {
    /// Creates the processor, for the route if any.
    pub fn new(route_id: Option<String>, processor: SharedProcessor) -> Self {
        UnitOfWorkProcessor {
            route_id,
            processor,
        }
    }
}

impl Processor for UnitOfWorkProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors([&self.processor]).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors([&self.processor]).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let created = !exchange.has_unit_of_work();
            if created {
                exchange.begin_unit_of_work();
            }
            if let Some(route_id) = &self.route_id {
                for s in exchange.on_completions().to_vec() {
                    s.on_before_route(route_id, exchange).await;
                }
            }
            if let Err(e) = self.processor.process(exchange).await {
                exchange.set_exception(Some(e));
            }
            if let Some(route_id) = &self.route_id {
                let list = exchange.on_completions().to_vec();
                after_route_synchronizations(route_id, exchange, list).await;
            }
            if created {
                let list = exchange.take_on_completions();
                done_synchronizations(exchange, list).await;
                exchange.end_unit_of_work();
            }
            Ok(())
        })
    }
}
