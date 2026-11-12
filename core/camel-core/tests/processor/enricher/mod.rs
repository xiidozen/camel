//! Tests of `org.apache.camel.processor.enricher`.

mod enricher_test;
mod poll_enricher_test;

use camel_core::api::processor::{aggregation_fn, SharedAggregationStrategy};
use std::sync::Arc;

/// Java `SampleAggregator`: `old:new` bodies.
pub fn sample_aggregator() -> SharedAggregationStrategy {
    Arc::new(aggregation_fn(|old, new| match (old, new) {
        (Some(mut o), Some(n)) => {
            let body = format!("{}:{}", o.message().body(), n.message().body());
            o.in_message_mut().set_body(body);
            Ok(Some(o))
        }
        (old, _) => Ok(old),
    }))
}
