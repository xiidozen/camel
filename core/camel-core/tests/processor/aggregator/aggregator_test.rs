//! Port of `AggregatorTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::processor::aggregate::UseLatestAggregationStrategy;
use std::sync::Arc;

async fn routes() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        let latest = || Arc::new(UseLatestAggregationStrategy);
        r.add(
            from("direct:many")
                .aggregate(header("cheese"), latest(), |a| a.to("mock:result"))
                .completion_timeout(2000)
                .completion_timeout_checker_interval(100),
        );
        r.add(
            from("direct:start")
                .aggregate(header("cheese"), latest(), |a| a.to("mock:result"))
                .completion_timeout(100)
                .completion_timeout_checker_interval(10),
        );
        r.add(
            from("seda:header")
                .set_header("visited", constant(true))
                .aggregate(header("cheese"), latest(), |a| a.to("mock:result"))
                .completion_timeout(100)
                .completion_timeout_checker_interval(10),
        );
        r.add(
            from("direct:predicate")
                .aggregate(header("cheese"), latest(), |a| a.to("mock:result"))
                .completion_timeout(100)
                .completion_timeout_checker_interval(10)
                .completion_predicate(header("cheese").is_equal_to(123)),
        );
    })
    .await
}

#[tokio::test]
async fn test_sending_lots_of_messages_get_aggregated_to_the_latest_message() {
    let t = routes().await;
    let result = t.mock("mock:result");
    result.expected_bodies_received(["message:100"]);
    for i in 1..=100 {
        t.template
            .send_body_and_header("direct:many", format!("message:{i}"), "cheese", 123)
            .await
            .unwrap();
    }
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_one_message() {
    let t = routes().await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    let headers = vec![
        ("cheese".to_string(), Value::from(123)),
        ("bar".to_string(), Value::from("viper bar")),
    ];
    t.template
        .send_body_and_headers("direct:predicate", "test", headers)
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_batch_timeout_expiry() {
    let t = routes().await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    t.template
        .send_body_and_header("direct:start", "message:1", "cheese", 123)
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_aggregator_not_at_start() {
    let t = routes().await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    result.message_matches(0, header("visited").is_not_null());
    t.template
        .send_body_and_header("seda:header", "message:1", "cheese", 123)
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}
