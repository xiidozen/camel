//! Port of `AggregateCompleteAllOnStopTest`.

use crate::support::{await_until, body_in_aggregating_strategy, ContextTestSupport};
use camel_core::api::spi::aggregation_repository::AggregationRepository;
use camel_core::prelude::*;
use camel_core::processor::aggregate::MemoryAggregationRepository;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_complete_all_on_stop() {
    let repo = Arc::new(MemoryAggregationRepository::default());
    let r2: Arc<dyn AggregationRepository> = repo.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("seda:start")
                .route_id("foo")
                .to("mock:input")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:aggregated")
                })
                .aggregation_repository(r2.clone())
                .completion_size(2)
                .completion_timeout(5000)
                .complete_all_on_stop()
                .completion_timeout_checker_interval(10),
        );
    })
    .await;
    let mock = t.mock("mock:aggregated");
    mock.expected_bodies_received(["A+B", "C"]);
    let input = t.mock("mock:input");
    input.expected_message_count(3);
    for b in ["A", "B", "C"] {
        t.template
            .send_body_and_header("seda:start", b, "id", "foo")
            .await
            .unwrap();
    }
    input.assert_is_satisfied().await;
    // the last message is in the aggregator
    await_until(Duration::from_secs(10), || repo.get("foo").is_some()).await;
    t.context.stop_route("foo").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
