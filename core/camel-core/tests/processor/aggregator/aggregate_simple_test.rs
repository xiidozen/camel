//! Ports of `AggregateSimpleSizeTest`, `AggregateSimplePredicateTest` and
//! `AggregateSimpleTimeoutTest`.

use crate::support::{body_in_aggregating_strategy, ContextTestSupport};
use camel_core::prelude::*;

async fn send_abc(t: &ContextTestSupport) {
    t.mock("mock:aggregated")
        .expected_bodies_received(["A+B+C"]);
    for b in ["A", "B", "C"] {
        t.template
            .send_body_and_header("direct:start", b, "id", 123)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_aggregate_simple_size() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:aggregated")
                })
                .completion_size(3),
        );
    })
    .await;
    send_abc(&t).await;
}

#[tokio::test]
async fn test_aggregate_simple_predicate() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:aggregated")
                })
                .completion_predicate(body().contains("A+B+C")),
        );
    })
    .await;
    send_abc(&t).await;
}

#[tokio::test]
async fn test_aggregate_simple_timeout() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:aggregated")
                })
                .completion_timeout(100)
                .completion_timeout_checker_interval(10),
        );
    })
    .await;
    send_abc(&t).await;
}

// AggregateReifier / AggregateProcessor validation
#[tokio::test]
async fn test_completion_option_required() {
    let ctx = camel_core::default_context();
    let err = ctx
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").aggregate(
                header("id"),
                body_in_aggregating_strategy(),
                |a| a.to("mock:aggregated"),
            ));
        })
        .await
        .unwrap_err();
    assert!(
        format!("{err}").contains("At least one of the completions options"),
        "{err}"
    );
}
