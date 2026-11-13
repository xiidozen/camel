//! Ports of the async endpoint tests of `processor/async` with the `async:` test component
//! (`support::my_async`): `AsyncEndpointPipelineTest`, `AsyncEndpointDualMulticastTest`,
//! `AsyncEndpointDoWhileLoopTest`, `AsyncEndpointFilterTest`, `AsyncDefaultErrorHandlerTest`,
//! `AsyncDeadLetterChannelTest` and `AsyncRouteWithErrorTest`. The Java assertions that the
//! routing continues on another thread have no equivalent on a tokio runtime.

use crate::support::my_async::async_context;
use crate::support::{cause_of, ContextTestSupport};
use camel_core::api::exchange_constants::LOOP_INDEX;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_async_endpoint_pipeline() {
    let t = async_context(|r: &mut Routes| {
        r.add(from("direct:start").pipeline(|p| p.to("async:bye:camel").to("mock:result")));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Bye Camel"]);
    let reply = t
        .template
        .request_body("direct:start", "Hello Camel")
        .await
        .unwrap();
    assert_eq!(Value::from("Bye Camel"), reply);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_async_endpoint_dual_multicast_and_filter() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:multicast")
                .to("mock:before")
                .to("log:before")
                .multicast(|m| m.to("async:hi:camel").to("async:bye:camel"))
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:filter")
                .to("mock:before")
                .to("log:before")
                .filter(body().contains("Camel"), |f| {
                    f.to("async:bye:camel").to("log:after").to("mock:after")
                })
                .to("mock:result"),
        );
    })
    .await;
    for uri in ["direct:multicast", "direct:filter"] {
        for (m, b) in [
            ("mock:before", "Hello Camel"),
            ("mock:after", "Bye Camel"),
            ("mock:result", "Bye Camel"),
        ] {
            t.mock(m).reset();
            t.mock(m).expected_bodies_received([b]);
        }
        let reply = t.template.request_body(uri, "Hello Camel").await.unwrap();
        assert_eq!(Value::from("Bye Camel"), reply, "{uri}");
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_async_endpoint_do_while_loop() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .loop_do_while(body().is_not_equal_to("done"), |l| {
                    l.to("async:bye:camel")
                        .to("mock:line")
                        .filter(exchange_property(LOOP_INDEX).is_equal_to(3), |f| {
                            f.set_body(constant("done"))
                        })
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:line")
        .expected_bodies_received(["Bye Camel"; 4]);
    t.mock("mock:result").expected_bodies_received(["done"]);
    t.template
        .request_body("direct:start", "World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

fn forced(_: &mut Exchange) -> CamelResult<()> {
    Err(CamelError::new(
        camel_core::api::error::ErrorKind::General,
        "Forced exception by unit test",
    ))
}

#[tokio::test]
async fn test_async_error_handlers() {
    // AsyncDefaultErrorHandlerTest
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:in")
                .threads_pool(2)
                .to("mock:foo")
                .process_fn(forced),
        );
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Hello World"]);
    let e = t
        .template
        .request_body("direct:in", "Hello World")
        .await
        .unwrap_err();
    assert_eq!(
        Some("Forced exception by unit test"),
        cause_of(&e).message()
    );
    t.assert_mock_endpoints_satisfied().await;
    // AsyncDeadLetterChannelTest
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        let eh = dead_letter_channel("mock:dead")
            .maximum_redeliveries(2)
            .redelivery_delay(0)
            .log_stack_trace(false);
        r.add(
            from("direct:in")
                .error_handler(eh)
                .threads_pool(2)
                .to("mock:foo")
                .process_fn(forced),
        );
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Hello World"]);
    t.mock("mock:dead").expected_message_count(1);
    t.template
        .request_body("direct:in", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_async_route_with_error() {
    let route = Arc::new(Mutex::new(String::new()));
    let r2 = route.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let route = r2.clone();
        r.add(
            from("direct:start")
                .transform(body().append(" World"))
                .threads()
                .to("mock:foo")
                .delay(constant(100))
                .process_fn(move |e| {
                    route.lock().unwrap().push('B');
                    assert_eq!(&Value::from("Hello World"), e.message().body());
                    Err(CamelError::illegal_argument("Damn forced by unit test"))
                })
                .to("mock:result"),
        );
    })
    .await;
    for _ in 0..2 {
        route.lock().unwrap().clear();
        t.mock("mock:foo").reset();
        t.mock("mock:result").reset();
        t.mock("mock:foo").expected_bodies_received(["Hello World"]);
        t.mock("mock:result").expected_message_count(0);
        let e = t
            .template
            .request_body("direct:start", "Hello")
            .await
            .unwrap_err();
        assert_eq!(Some("Damn forced by unit test"), cause_of(&e).message());
        route.lock().unwrap().push('A');
        t.assert_mock_endpoints_satisfied().await;
        assert_eq!("BA", route.lock().unwrap().as_str());
    }
}
