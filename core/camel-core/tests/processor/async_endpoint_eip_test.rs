//! Ports of the async endpoint tests of `processor/async` in the EIPs, with the `async:` test
//! component: `AsyncEndpointCBRTest`, `AsyncEndpointLoopTest`, `AsyncEndpointMulticastTest`,
//! `AsyncEndpointRecipientList2Test`, `AsyncEndpointRoutingSlipTest`,
//! `AsyncEndpointDynamicRouterTest`, `AsyncEndpointSingleTopicLoadBalanceTest` and
//! `AsyncEndpointFailedAfterTest` (the thread name assertions have no tokio equivalent).

use crate::support::cause_of;
use crate::support::my_async::async_context;
use camel_bean::MethodBean;
use camel_core::api::error::ErrorKind;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

fn noop(_: &mut Exchange) -> CamelResult<()> {
    Ok(())
}

#[tokio::test]
async fn test_async_endpoint_in_eips() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:cbr")
                .to("mock:before")
                .to("log:before")
                .choice(|c| {
                    c.when(body().contains("Camel"), |w| {
                        w.process_fn(noop)
                            .to("async:bye:camel")
                            .process_fn(noop)
                            .to("log:after")
                            .to("mock:after")
                    })
                })
                .to("mock:result"),
        );
        r.add(
            from("direct:multicast")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .multicast(|m| m.to("async:hi:moon").to("direct:foo"))
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(from("direct:foo").set_body(constant("Bye Camel")));
        r.add(
            from("direct:recipients")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("direct:bar")),
        );
        r.add(
            from("direct:bar")
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
    })
    .await;
    for uri in ["direct:cbr", "direct:multicast", "direct:recipients"] {
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
async fn test_async_endpoint_loop() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .loop_count(constant(2), |l| l.to("mock:loop").to("async:bye:camel"))
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:before")
        .expected_bodies_received(["Hello Camel"]);
    t.mock("mock:loop")
        .expected_bodies_received(["Hello Camel", "Bye Camel"]);
    t.mock("mock:after").expected_bodies_received(["Bye Camel"]);
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
async fn test_async_endpoint_routing_slip_and_dynamic_router() {
    // Java AsyncEndpointDynamicRouterTest.slip(String body)
    let bodies: Arc<Mutex<Vec<String>>> = Arc::default();
    let b = bodies.clone();
    let slip = MethodBean::new("AsyncEndpointDynamicRouterTest").method("slip", move |inv| {
        let mut bodies = b.lock().unwrap();
        bodies.push(inv.parameter(0).to_string());
        Ok(match bodies.len() {
            1 => Value::from("async:bye:camel"),
            2 => Value::from("direct:foo"),
            3 => Value::from("mock:result"),
            _ => Value::Null,
        })
    });
    let t = crate::support::ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("router", slip.into_value())
        .unwrap();
    t.context
        .add_component(
            "async",
            Arc::new(crate::support::my_async::MyAsyncComponent),
        )
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:slip").routing_slip(constant("async:bye:camel,mock:result")));
            r.add(from("direct:router").dynamic_router(method("router", "slip")));
            r.add(from("direct:foo").transform(constant("Bye World")));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["Bye Camel"]);
    assert_eq!(
        Value::from("Bye Camel"),
        t.template
            .request_body("direct:slip", "Hello Camel")
            .await
            .unwrap()
    );
    t.assert_mock_endpoints_satisfied().await;
    t.mock("mock:result").reset();
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    assert_eq!(
        Value::from("Bye World"),
        t.template
            .request_body("direct:router", "Hello Camel")
            .await
            .unwrap()
    );
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        vec!["Hello Camel", "Bye Camel", "Bye World", "Bye World"],
        *bodies.lock().unwrap()
    );
}

#[tokio::test]
async fn test_async_endpoint_topic_load_balance_and_failed_after() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:topic")
                .to("mock:before")
                .to("log:before")
                .load_balance(LoadBalancerDefinition::Topic, |b| b.to("async:bye:camel"))
                .to("log:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:failed")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .throw_exception(CamelError::illegal_argument("Damn"))
                .to("mock:result"),
        );
    })
    .await;
    // AsyncEndpointSingleTopicLoadBalanceTest: the topic sends a copy
    t.mock("mock:before")
        .expected_bodies_received(["Hello Camel"]);
    t.mock("mock:result")
        .expected_bodies_received(["Hello Camel"]);
    t.template
        .send_body("direct:topic", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    // AsyncEndpointFailedAfterTest
    for m in ["mock:before", "mock:after", "mock:result"] {
        t.mock(m).reset();
    }
    t.mock("mock:before")
        .expected_bodies_received(["Hello Camel"]);
    t.mock("mock:after").expected_bodies_received(["Bye Camel"]);
    t.mock("mock:result").expected_message_count(0);
    let e = t
        .template
        .request_body("direct:failed", "Hello Camel")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::IllegalArgument), "{cause:?}");
    assert_eq!(Some("Damn"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}

/// `AsyncEndpointRecipientList3Test`, `AsyncEndpointRecipientList4Test`,
/// `AsyncEndpointRoutingSlip2Test` and `AsyncEndpointEnricherTest`.
#[tokio::test]
async fn test_async_endpoint_recipients_and_enricher() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:list3")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:hi:camel,direct:foo")),
        );
        r.add(
            from("direct:list4")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:hi:camel,async:hi:world,direct:foo")),
        );
        r.add(
            from("direct:foo")
                .process_fn(|e| {
                    e.message_mut().set_body("Bye Camel");
                    Ok(())
                })
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:enrich")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .enrich("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(from("direct:slip2").routing_slip(constant("async:bye:camel,async:bye:world")));
    })
    .await;
    for uri in ["direct:list3", "direct:list4", "direct:enrich"] {
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
    assert_eq!(
        Value::from("Bye World"),
        t.template
            .request_body("direct:slip2", "Hello Camel")
            .await
            .unwrap()
    );
}

/// `AsyncEndpointIdempotentConsumerTest`.
#[tokio::test]
async fn test_async_endpoint_idempotent_consumer() {
    let t = async_context(|r: &mut Routes| {
        let repo: camel_core::api::spi::idempotent_repository::SharedIdempotentRepository =
            Arc::new(
                camel_core::support::idempotent::MemoryIdempotentRepository::with_max_cache_size(
                    200,
                ),
            );
        r.add(
            from("direct:start")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .idempotent_consumer(header("myId"), repo, |s| {
                    s.to("async:bye:camel")
                        .process_fn(noop)
                        .to("log:after")
                        .to("mock:after")
                        .to("mock:result")
                }),
        );
    })
    .await;
    t.mock("mock:before")
        .expected_bodies_received(["A", "B", "C"]);
    for m in ["mock:after", "mock:result"] {
        t.mock(m)
            .expected_bodies_received(["Bye Camel", "Bye Camel"]);
    }
    for (b, id) in [("A", 123), ("B", 123), ("C", 456)] {
        t.template
            .send_body_and_header("direct:start", b, "myId", Value::Int(id))
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
    for m in ["mock:after", "mock:result"] {
        let ids: Vec<Value> = t
            .mock(m)
            .received_exchanges()
            .iter()
            .map(|e| e.message().header("myId").cloned().unwrap())
            .collect();
        assert_eq!(vec![Value::Int(123), Value::Int(456)], ids, "{m}");
    }
}

/// `AsyncEndpointFailOverLoadBalanceMixedTest` and `AsyncEndpointFailOverLoadBalanceOnlyAsyncTest`.
#[tokio::test]
async fn test_async_endpoint_failover() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:mixed")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .load_balance(LoadBalancerDefinition::failover(), |b| {
                    b.to("async:bye:camel?failFirstAttempts=5")
                        .to("direct:fail")
                        .to("async:bye:moon?failFirstAttempts=5")
                        .to("async:bye:world")
                })
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:onlyAsync")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .load_balance(LoadBalancerDefinition::failover(), |b| {
                    b.to("async:bye:camel?failFirstAttempts=5")
                        .to("async:bye:moon?failFirstAttempts=5")
                        .to("async:bye:world")
                })
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:fail")
                .to("log:fail")
                .to("mock:fail")
                .throw_exception(CamelError::illegal_argument("Damn")),
        );
    })
    .await;
    for (uri, fail) in [("direct:mixed", 1), ("direct:onlyAsync", 0)] {
        for m in ["mock:before", "mock:fail", "mock:after", "mock:result"] {
            t.mock(m).reset();
        }
        t.mock("mock:before")
            .expected_bodies_received(["Hello Camel"]);
        t.mock("mock:fail").expected_message_count(fail);
        t.mock("mock:after").expected_bodies_received(["Bye World"]);
        t.mock("mock:result")
            .expected_bodies_received(["Bye World"]);
        let reply = t.template.request_body(uri, "Hello Camel").await.unwrap();
        assert_eq!(Value::from("Bye World"), reply, "{uri}");
        t.assert_mock_endpoints_satisfied().await;
    }
}

fn failing_after_async(from_uri: &str) -> RouteDsl {
    from(from_uri)
        .to("mock:before")
        .to("log:before")
        .process_fn(noop)
        .to("async:bye:camel")
        .process_fn(noop)
        .to("log:after")
        .to("mock:after")
        .throw_exception(CamelError::illegal_argument("Damn"))
        .to("mock:result")
}

/// `AsyncEndpointFailedAfterDeadLetterChannelTest`, `AsyncEndpointFailedAfterOnExceptionHandledTest`
/// and `AsyncEndpointFailedAfterOnExceptionContinueTest`.
#[tokio::test]
async fn test_async_endpoint_failed_after() {
    let dlc = async_context(|r: &mut Routes| {
        r.error_handler(
            dead_letter_channel("mock:dead")
                .maximum_redeliveries(2)
                .use_original_message(),
        );
        r.add(failing_after_async("direct:start"));
    })
    .await;
    dlc.mock("mock:before")
        .expected_bodies_received(["Hello Camel"]);
    dlc.mock("mock:after")
        .expected_bodies_received(["Bye Camel"]);
    dlc.mock("mock:dead")
        .expected_bodies_received(["Hello Camel"]);
    dlc.mock("mock:result").expected_message_count(0);
    dlc.template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap();
    dlc.assert_mock_endpoints_satisfied().await;
    let handled = async_context(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .handled(true)
                .to("mock:error")
                .transform(constant("FAIL")),
        );
        r.add(failing_after_async("direct:start"));
    })
    .await;
    handled
        .mock("mock:before")
        .expected_bodies_received(["Hello Camel"]);
    handled
        .mock("mock:after")
        .expected_bodies_received(["Bye Camel"]);
    handled
        .mock("mock:error")
        .expected_bodies_received(["Bye Camel"]);
    handled.mock("mock:result").expected_message_count(0);
    assert_eq!(
        Value::from("FAIL"),
        handled
            .template
            .request_body("direct:start", "Hello Camel")
            .await
            .unwrap()
    );
    handled.assert_mock_endpoints_satisfied().await;
    let continued = async_context(|r: &mut Routes| {
        r.on_exception(on_exception(ErrorKind::IllegalArgument).continued(true));
        r.add(failing_after_async("direct:start"));
    })
    .await;
    continued
        .mock("mock:before")
        .expected_bodies_received(["Hello Camel"]);
    continued
        .mock("mock:after")
        .expected_bodies_received(["Bye Camel"]);
    continued
        .mock("mock:result")
        .expected_bodies_received(["Bye Camel"]);
    assert_eq!(
        Value::from("Bye Camel"),
        continued
            .template
            .request_body("direct:start", "Hello Camel")
            .await
            .unwrap()
    );
    continued.assert_mock_endpoints_satisfied().await;
    let caught = continued.mock("mock:result").received_exchanges()[0]
        .property(camel_core::api::exchange_constants::EXCEPTION_CAUGHT)
        .cloned();
    assert!(caught.is_some_and(|c| !c.is_null()), "the caught exception");
}
