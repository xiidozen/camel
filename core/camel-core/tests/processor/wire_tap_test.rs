//! Ports of `WireTapTest`, `WireTapLogTest`, `WireTapExpressionTest`, `WireTapPatternTest`
//! and `WireTapIgnoreInvalidEndpointTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

async fn routes() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("log:foo")
                .wire_tap("direct:tap")
                .to("mock:result"),
        );
        r.add(
            from("direct:tap")
                .delay(constant(1000))
                .set_body(constant("Tapped"))
                .to("mock:result")
                .to("mock:tap"),
        );
        r.add(
            from("direct:test")
                .wire_tap("direct:a")
                .id("wiretap_1")
                .to("mock:a"),
        );
        r.add(from("direct:a").to("mock:b"));
    })
    .await
}

#[tokio::test]
async fn test_send() {
    let t = routes().await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World", "Tapped"]);
    t.mock("mock:tap").expected_bodies_received(["Tapped"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_wire_tap_id() {
    let t = routes().await;
    t.mock("mock:a").expected_bodies_received(["Hello"]);
    t.mock("mock:b").expected_bodies_received(["Hello"]);
    t.template.send_body("direct:test", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// WireTapLogTest
#[tokio::test]
async fn test_wire_tap_log() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .wire_tap("log:com.foo.MyApp?level=WARN")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// WireTapExpressionTest
#[tokio::test]
async fn test_wire_tap_expression() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").wire_tap("mock:${header.tap}"));
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Hello Camel"]);
    t.mock("mock:bar").expected_bodies_received(["Hello World"]);
    t.template
        .send_body_and_header("direct:start", "Hello Camel", "tap", "foo")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Hello World", "tap", "bar")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// WireTapPatternTest
#[tokio::test]
async fn test_wire_tap_pattern() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_exchange_pattern(ExchangePattern::InOut)
                .wire_tap("direct:tap")
                .to("mock:result"),
        );
        r.add(from("direct:tap").to("mock:tap"));
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    t.mock("mock:tap").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        ExchangePattern::InOut,
        t.mock("mock:result").received_exchanges()[0].pattern()
    );
    assert_eq!(
        ExchangePattern::InOnly,
        t.mock("mock:tap").received_exchanges()[0].pattern()
    );
}

// WireTapIgnoreInvalidEndpointTest
#[tokio::test]
async fn test_ignore_invalid() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .wire_tap("xxx:invalid")
                .ignore_invalid_endpoints()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
