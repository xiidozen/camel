//! Port of `InterceptSimpleRouteTest`, `InterceptSimpleRouteStopTest`,
//! `InterceptSimpleRouteWhenTest` and `InterceptSimpleRouteWhenStopTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_intercept() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept(|i| i.to("mock:intercepted"));
        r.add(
            from("direct:start")
                .to("mock:foo")
                .to("mock:bar")
                .to("mock:result"),
        );
    })
    .await;
    for m in ["mock:foo", "mock:bar", "mock:result"] {
        t.mock(m).expected_message_count(1);
    }
    t.mock("mock:intercepted").expected_message_count(3);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_intercept_with_stop() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept(|i| i.to("mock:intercepted").stop());
        r.add(
            from("direct:start")
                .to("mock:foo")
                .to("mock:bar")
                .to("mock:result"),
        );
    })
    .await;
    for m in ["mock:foo", "mock:bar", "mock:result"] {
        t.mock(m).expected_message_count(0);
    }
    t.mock("mock:intercepted").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_intercept_when() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept(|i| i.on_when(body().contains("Hello")).to("mock:intercepted"));
        r.add(
            from("direct:start")
                .to("mock:foo")
                .to("mock:bar")
                .to("mock:result"),
        );
    })
    .await;
    for m in ["mock:foo", "mock:bar", "mock:result"] {
        t.mock(m).expected_bodies_received(["Hello World", "Hi"]);
    }
    t.mock("mock:intercepted").expected_bodies_received([
        "Hello World",
        "Hello World",
        "Hello World",
    ]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.template.send_body("direct:start", "Hi").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn when_stop() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept(|i| {
            i.on_when(body().contains("Hello"))
                .to("mock:intercepted")
                .stop()
        });
        r.add(
            from("direct:start")
                .to("mock:foo")
                .to("mock:bar")
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_intercept_when_stop() {
    let t = when_stop().await;
    for m in ["mock:foo", "mock:bar", "mock:result"] {
        t.mock(m).expected_message_count(0);
    }
    t.mock("mock:intercepted").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_intercept_when_no_stop() {
    let t = when_stop().await;
    for m in ["mock:foo", "mock:bar", "mock:result"] {
        t.mock(m).expected_message_count(1);
    }
    t.mock("mock:intercepted").expected_message_count(0);
    t.template.send_body("direct:start", "Hi").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
