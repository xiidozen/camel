//! # camel-core-processor
//!
//! The processors implementing the Enterprise Integration Patterns of the routing engine:
//! pipeline, filter, content based router (choice), message translators (set body, set
//! header, ...), log, stop and the send processor (`to`). This crate is the Rust port of the
//! Java `core/camel-core-processor` module (`org.apache.camel.processor`).
//!
//! As in Java, an error is carried *on the exchange*: [`process_and_capture`] runs a processor
//! and stores a returned error with [`Exchange::set_exception`](camel_api::exchange::Exchange::set_exception),
//! and the routing processors stop when the exchange has failed (see [`continue_processing`]).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod choice;
pub mod errorhandler;
pub mod filter;
pub mod log_processor;
pub mod pipeline;
pub mod send;
pub mod transform;
pub mod try_catch;

use camel_api::exchange::Exchange;
use camel_api::processor::Processor;

/// Whether routing of the exchange should continue (Java `PipelineHelper.continueProcessing`):
/// not when it failed, is marked for rollback, was handled by an error handler, or is marked
/// to stop routing.
pub fn continue_processing(exchange: &Exchange, message: &str) -> bool {
    let stop = exchange.is_failed()
        || exchange.is_rollback_only()
        || exchange.is_rollback_only_last()
        || exchange.error_handler_handled() == Some(true);
    if stop {
        log::debug!("Message exchange has failed: {message} for exchange: {exchange}");
        return false;
    }
    if exchange.is_route_stop() {
        log::debug!(
            "ExchangeId: {} is marked to stop routing: {exchange}",
            exchange.exchange_id()
        );
        return false;
    }
    true
}

/// Moves the out message (if any) to the in message, to prepare the exchange for the next
/// processor (Java `ExchangeHelper.prepareOutToIn`).
pub fn prepare_out_to_in(exchange: &mut Exchange) {
    if let Some(out) = exchange.out_message().cloned() {
        exchange.set_in(out);
        exchange.set_out(None);
    }
}

/// Runs the processor, storing a returned error on the exchange (an error is carried by the
/// exchange through the routing, as in Java).
pub async fn process_and_capture(processor: &dyn Processor, exchange: &mut Exchange) {
    if let Err(e) = processor.process(exchange).await {
        exchange.set_exception(Some(e));
    }
}
