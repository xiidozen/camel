//! Port of `AggregateCompletionIntervalTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::processor::aggregate::UseLatestAggregationStrategy;
use std::sync::Arc;

#[tokio::test]
async fn test_aggregate_interval() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:start")
                .aggregate(header("id"), Arc::new(UseLatestAggregationStrategy), |a| {
                    a.to("mock:result")
                })
                .completion_interval(2000),
        );
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_bodies_received(["Message 9"]);
    for i in 0..10 {
        t.template
            .send_body_and_header("seda:start", format!("Message {i}"), "id", "1")
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}
