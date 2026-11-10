//! Port of `AggregateIgnoreInvalidCorrelationKeysTest`.

use crate::support::{body_in_aggregating_strategy, cause_of, ContextTestSupport};
use camel_core::prelude::*;

#[tokio::test]
async fn test_aggregate_ignore_invalid_correlation_keys() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:result")
                })
                .completion_size(2)
                .ignore_invalid_correlation_keys(),
        );
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["A+C"]);
    t.template
        .send_body_and_header("direct:start", "A", "id", 1)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "B", "id", Value::Null)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "C", "id", 1)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_aggregate_not_ignore_invalid_correlation_keys() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:result")
                })
                .completion_size(2),
        );
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["A+C"]);
    t.template
        .send_body_and_header("direct:start", "A", "id", 1)
        .await
        .unwrap();
    let err = t
        .template
        .send_body_and_header("direct:start", "B", "id", Value::Null)
        .await
        .unwrap_err();
    assert!(err.is(ErrorKind::CamelExecution), "{err:?}");
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::CamelExchange), "{err:?}");
    assert!(
        cause.to_string().starts_with("Invalid correlation key"),
        "{cause}"
    );
    t.template
        .send_body_and_header("direct:start", "C", "id", 1)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
