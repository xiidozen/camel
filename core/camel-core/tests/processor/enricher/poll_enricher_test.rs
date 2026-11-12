//! Port of `PollEnricherTest`, `PollEnricherNoResourceTest` and `PollEnrichNullBodyTest`
//! style cases (polling seda queues).

use super::sample_aggregator;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use std::time::{Duration, Instant};

async fn context() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:enricher-test-1")
                .poll_enrich("seda:foo1", -1)
                .aggregation_strategy(sample_aggregator())
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-2")
                .poll_enrich("seda:foo2", 1000)
                .aggregation_strategy(sample_aggregator())
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-3")
                .poll_enrich("seda:foo3", -1)
                .aggregation_strategy(sample_aggregator())
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-4")
                .poll_enrich("seda:foo4", -1)
                .aggregation_strategy(sample_aggregator()),
        );
        r.add(
            from("direct:no-resource")
                .poll_enrich("seda:empty", 0)
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_poll_enrich_in_only() {
    let t = context().await;
    t.template.send_body("seda:foo1", "blah").await.unwrap();
    let mock = t.mock("mock:mock");
    mock.expected_bodies_received(["test:blah"]);
    t.template
        .send_body("direct:enricher-test-1", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_poll_enrich_in_only_wait_with_timeout() {
    let t = context().await;
    let mock = t.mock("mock:mock");
    mock.expected_bodies_received(["test:blah"]);
    mock.set_result_wait_time(100);
    // nothing to poll within the timeout: the body is the original one
    t.template
        .send_body("direct:enricher-test-2", "test")
        .await
        .unwrap();
    mock.assert_is_not_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["test:blah"]);
    t.template.send_body("seda:foo2", "blah").await.unwrap();
    t.template
        .send_body("direct:enricher-test-2", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_poll_enrich_in_only_wait_no_timeout() {
    let t = context().await;
    let template = t.context.create_producer_template();
    let sender = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        template.send_body("seda:foo3", "blah").await.unwrap();
    });
    let mock = t.mock("mock:mock");
    mock.expected_bodies_received(["test:blah"]);
    let watch = Instant::now();
    t.template
        .send_body("direct:enricher-test-3", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    let delta = watch.elapsed().as_millis();
    assert!(delta > 150, "Should take approx 0.25 sec: was {delta}");
    sender.await.unwrap();
}

#[tokio::test]
async fn test_poll_enrich_in_out_plus_header() {
    let t = context().await;
    t.template.send_body("seda:foo4", "blah").await.unwrap();
    let mut e = Exchange::with_body(t.context.as_shared(), "test");
    e.set_pattern(ExchangePattern::InOut);
    e.message_mut().set_header("foo", "bar");
    let result = t.template.send("direct:enricher-test-4", e).await.unwrap();
    assert_eq!(Some(&Value::from("bar")), result.message().header("foo"));
    assert_eq!(&Value::from("test:blah"), result.message().body());
    assert!(result.exception().is_none());
}

#[tokio::test]
async fn test_no_resource_sets_null_body_keeping_headers() {
    let t = context().await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.message_matches(0, body().is_null());
    mock.expected_header_received("foo", "bar");
    t.template
        .send_body_and_header("direct:no-resource", "Hello", "foo", "bar")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
