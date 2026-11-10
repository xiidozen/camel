//! The pipeline: runs processors in sequence (Java `org.apache.camel.processor.Pipeline`).

use crate::{continue_processing, prepare_out_to_in, process_and_capture};
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedProcessor,
};

/// Runs the processors in sequence, each one receiving the result of the previous one (an out
/// message becomes the in message of the next processor). Stops when the exchange fails or is
/// marked to stop routing; the error stays on the exchange.
pub struct Pipeline {
    processors: Vec<SharedProcessor>,
}

impl Pipeline {
    /// Creates the pipeline.
    pub fn new(processors: Vec<SharedProcessor>) -> Self {
        Pipeline { processors }
    }

    /// The processors.
    pub fn processors(&self) -> &[SharedProcessor] {
        &self.processors
    }
}

impl Processor for Pipeline {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors(&self.processors).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(&self.processors).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            for (i, p) in self.processors.iter().enumerate() {
                if exchange.is_route_stop()
                    || (i > 0 && !continue_processing(exchange, "so breaking out of pipeline"))
                {
                    break;
                }
                prepare_out_to_in(exchange);
                process_and_capture(p.as_ref(), exchange).await;
            }
            log::trace!(
                "Processing complete for exchangeId: {} >>> {exchange}",
                exchange.exchange_id()
            );
            Ok(())
        })
    }
}
