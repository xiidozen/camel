//! Port of `InterceptFromSimpleRouteTest`, `InterceptFromUriWildcardTest` and
//! `InterceptFromUriRegexTest`.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::INTERCEPTED_ENDPOINT;
use camel_core::prelude::*;

#[tokio::test]
async fn test_intercept_from_when_stop() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept_from(None, |i| {
            i.on_when(header("city").is_equal_to("London"))
                .to("mock:intercepted")
                .stop()
        });
        r.add(from("seda:a").to("mock:result"));
    })
    .await;
    t.mock("mock:intercepted")
        .expected_bodies_received(["Hello London"]);
    t.mock("mock:result")
        .expected_bodies_received(["Hello Paris"]);
    t.template
        .send_body_and_header("seda:a", "Hello London", "city", "London")
        .await
        .unwrap();
    t.template
        .send_body_and_header("seda:a", "Hello Paris", "city", "Paris")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn uri_context(pattern: &'static str) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.intercept_from(Some(pattern), |i| i.to("mock:intercept"));
        r.add(from("direct:start").to("mock:result"));
        r.add(from("seda:bar").to("mock:result"));
        r.add(from("seda:foo").to("mock:result"));
        r.add(from("seda:cheese").to("mock:result"));
    })
    .await
}

async fn send(t: &ContextTestSupport, uri: &str, intercepted: i32) {
    t.reset_mocks();
    t.mock("mock:intercept").expected_message_count(intercepted);
    t.mock("mock:result").expected_message_count(1);
    t.template.send_body(uri, "Hello World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_intercept_from_uri_wildcard() {
    let t = uri_context("seda*").await;
    send(&t, "direct:start", 0).await;
    t.reset_mocks();
    let intercept = t.mock("mock:intercept");
    intercept.expected_message_count(1);
    intercept.expected_property_received(INTERCEPTED_ENDPOINT, "seda://foo");
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("seda:foo", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    send(&t, "seda:bar", 1).await;
}

#[tokio::test]
async fn test_intercept_from_uri_regex() {
    let t = uri_context("seda:(bar|foo)").await;
    send(&t, "direct:start", 0).await;
    send(&t, "seda:foo", 1).await;
    send(&t, "seda:bar", 1).await;
    send(&t, "seda:cheese", 0).await;
}
