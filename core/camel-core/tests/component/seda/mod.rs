//! Tests of `org.apache.camel.component.seda`.

mod same_seda_queue_size_and_no_size_test;
mod seda_discard_if_no_consumer_test;
mod seda_discard_when_full_test;
mod seda_in_only_test;
mod seda_in_out_chained_test;
mod seda_in_out_test;
mod seda_in_out_with_error_test;
mod seda_route_test;

use camel_core::prelude::*;
use camel_seda::SedaEndpoint;

/// The current queue size of the seda endpoint (Java `getCurrentQueueSize`).
pub fn queue_size(context: &DefaultCamelContext, uri: &str) -> usize {
    let e = context.endpoint(uri).unwrap();
    e.as_any()
        .downcast_ref::<SedaEndpoint>()
        .expect("a seda endpoint")
        .queue()
        .len()
}
