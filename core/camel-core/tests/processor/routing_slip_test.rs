//! Port of `RoutingSlipDslTest`, `DynamicRouterTest`, `RoutingSlipThrowExceptionFromExpressionTest`
//! and Rust tests of the routing slip options.

use crate::support::ContextTestSupport;
use camel_bean::MethodBean;
use camel_core::api::exchange_constants::SLIP_ENDPOINT;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_routing_slip_dsl() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").routing_slip(header("recipientListHeader")));
    })
    .await;
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["foo", "bar"]);
    }
    for body in ["foo", "bar"] {
        t.template
            .send_body_and_header(
                "direct:a",
                body,
                "recipientListHeader",
                "mock:x,mock:y,mock:z",
            )
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_dynamic_router() {
    // Java DynamicRouterTest.slip(String body)
    let bodies: Arc<Mutex<Vec<String>>> = Arc::default();
    let b = bodies.clone();
    let slip = MethodBean::new("DynamicRouterTest").method("slip", move |inv| {
        let mut bodies = b.lock().unwrap();
        bodies.push(inv.parameter(0).to_string());
        Ok(match bodies.len() {
            1 => Value::from("mock:a"),
            2 => Value::from("mock:b,mock:c"),
            3 => Value::from("direct:foo"),
            4 => Value::from("mock:result"),
            _ => Value::Null,
        })
    });
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("router", slip.into_value())
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").dynamic_router(method("router", "slip")));
            r.add(from("direct:foo").transform(constant("Bye World")));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    for m in ["mock:a", "mock:b", "mock:c"] {
        t.mock(m).expected_bodies_received(["Hello World"]);
    }
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let bodies = bodies.lock().unwrap();
    assert_eq!(
        vec![
            "Hello World",
            "Hello World",
            "Hello World",
            "Bye World",
            "Bye World"
        ],
        *bodies
    );
}

#[tokio::test]
async fn test_routing_slip_throw_exception_from_expression() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .do_try(|b| {
                    b.routing_slip(simple("${body.wrong}"))
                        .do_catch(ErrorKind::RuntimeExpression, |c| c.to("mock:error"))
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:error").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_routing_slip_options() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .routing_slip(header("slip"))
                .uri_delimiter("#")
                .ignore_invalid_endpoints()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:x").expected_message_count(1);
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    result.expected_property_received(SLIP_ENDPOINT, "mock://y");
    t.template
        .send_body_and_header("direct:start", "Hello", "slip", "mock:x#nosuch:foo#mock:y")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        "RoutingSlip[header(slip)]",
        from("direct:x").routing_slip(header("slip")).build().steps[0].to_string()
    );
}
