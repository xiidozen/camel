//! Port of `EnricherTest`, `EnricherDefaultAggregationStrategyTest`,
//! `EnricherNoResourceTest` and `EnrichExpressionIgnoreInvalidEndpointTest` (the uri as a
//! simple expression).

use super::sample_aggregator;
use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::TO_ENDPOINT;
use camel_core::api::processor::processor_fn;
use camel_core::prelude::*;
use std::sync::Arc;

async fn context() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:enricher-test-1")
                .enrich("direct:enricher-constant-resource")
                .aggregation_strategy(sample_aggregator())
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-4")
                .error_handler(no_error_handler())
                .enrich("direct:enricher-error-resource")
                .aggregation_strategy(sample_aggregator())
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-5")
                .enrich("direct:enricher-constant-resource")
                .aggregation_strategy(sample_aggregator()),
        );
        r.add(
            from("direct:enricher-test-8")
                .error_handler(no_error_handler())
                .enrich("direct:enricher-error-resource")
                .aggregation_strategy(sample_aggregator()),
        );
        r.add(from("direct:enricher-constant-resource").transform(constant("blah")));
        // Java FailureProcessor
        r.add(
            from("direct:enricher-error-resource")
                .error_handler(no_error_handler())
                .process(Arc::new(processor_fn(|_| {
                    Err(CamelError::runtime("failed"))
                }))),
        );
    })
    .await
}

#[tokio::test]
async fn test_enrich_in_only() {
    let t = context().await;
    let mock = t.mock("mock:mock");
    mock.expected_bodies_received(["test:blah"]);
    mock.message_matches(0, exchange_property(TO_ENDPOINT).is_equal_to("mock://mock"));
    t.template
        .send_body("direct:enricher-test-1", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_enrich_error_in_only() {
    let t = context().await;
    let mock = t.mock("mock:mock");
    mock.expected_message_count(0);
    let e = Exchange::with_body(t.context.as_shared(), "test");
    let result = t.template.send("direct:enricher-test-4", e).await.unwrap();
    mock.assert_is_satisfied().await;
    assert_eq!(&Value::from("test"), result.message().body());
    assert_eq!(Some("failed"), result.exception().and_then(|e| e.message()));
}

#[tokio::test]
async fn test_enrich_in_out() {
    let t = context().await;
    let result = t
        .template
        .request_body("direct:enricher-test-5", "test")
        .await
        .unwrap();
    assert_eq!(Value::from("test:blah"), result);
}

#[tokio::test]
async fn test_enrich_in_out_plus_header() {
    let t = context().await;
    let mut e = Exchange::with_body(t.context.as_shared(), "test");
    e.set_pattern(ExchangePattern::InOut);
    e.message_mut().set_header("foo", "bar");
    let result = t.template.send("direct:enricher-test-5", e).await.unwrap();
    assert_eq!(Some(&Value::from("bar")), result.message().header("foo"));
    assert_eq!(&Value::from("test:blah"), result.message().body());
    assert!(result.exception().is_none());
}

#[tokio::test]
async fn test_enrich_error_in_out() {
    let t = context().await;
    let mut e = Exchange::with_body(t.context.as_shared(), "test");
    e.set_pattern(ExchangePattern::InOut);
    let result = t.template.send("direct:enricher-test-8", e).await.unwrap();
    assert_eq!(&Value::from("test"), result.message().body());
    assert_eq!(Some("failed"), result.exception().and_then(|e| e.message()));
}

#[tokio::test]
async fn test_default_aggregation_strategy() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").enrich("direct:foo").to("mock:result"));
        r.add(from("direct:foo").transform(body().prepend("Bye ")));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Bye World", "Bye World"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    let out = t
        .template
        .request_body("direct:start", "World")
        .await
        .unwrap();
    assert_eq!(Value::from("Bye World"), out);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_no_resource() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").enrich("direct:foo").to("mock:result"));
        r.add(from("direct:foo").set_body(constant(Value::Null)));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.message_matches(0, body().is_null());
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_ignore_invalid_endpoint_and_dynamic_uri() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .enrich("${header.source}")
                .ignore_invalid_endpoints()
                .to("mock:result"),
        );
        r.add(from("direct:foo").transform(constant("Hello from foo")));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello from foo", "Unknown"]);
    t.template
        .send_body_and_header("direct:start", "Camel", "source", "direct:foo")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Unknown", "source", "unknown:foo")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
