//! Ports of more async endpoint tests of `processor/async` with the `async:` test component:
//! the `AsyncEndpointRecipientList*Test`s (sequential and parallel), `AsyncEndpointRoutingSlip3Test`,
//! `AsyncEndpointSynchronousTest`, `AsyncEndpointSynchronousFalseTest`, `AsyncEndpointUoWTest`,
//! `AsyncEndpointUoWFailedTest`, `AsyncEndpointSplitUseLatestAggregationStrategyTest`,
//! `AsyncFailureProcessorWithRedeliveryTest` and `AsyncNestedTripleChoiceIssueTest` (the
//! assertions on the threads have no equivalent on a tokio runtime).

use crate::support::my_async::async_context;
use crate::support::{cause_of, ContextTestSupport};
use camel_core::api::error::ErrorKind;
use camel_core::api::spi::synchronization::synchronization_fn;
use camel_core::prelude::*;
use camel_core::processor::aggregate::UseLatestAggregationStrategy;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn noop(_: &mut Exchange) -> CamelResult<()> {
    Ok(())
}

fn expect_standard(t: &ContextTestSupport) {
    for (m, b) in [
        ("mock:before", "Hello Camel"),
        ("mock:after", "Bye Camel"),
        ("mock:result", "Bye Camel"),
    ] {
        t.mock(m).reset();
        t.mock(m).expected_bodies_received([b]);
    }
}

fn bye_camel(e: &mut Exchange) -> CamelResult<()> {
    e.message_mut().set_body("Bye Camel");
    Ok(())
}

#[tokio::test]
async fn test_async_endpoint_recipient_lists() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:list")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:bye:camel"))
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:parallel")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:bye:camel"))
                .parallel_processing()
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:parallel2")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("direct:foo2"))
                .parallel_processing(),
        );
        r.add(
            from("direct:foo2")
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:parallel3")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:hi:camel?delay=200,direct:foo"))
                .parallel_processing(),
        );
        r.add(
            from("direct:parallel4")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:hi:camel,async:hi:world,direct:foo"))
                .parallel_processing(),
        );
        r.add(
            from("direct:foo")
                .process_fn(bye_camel)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:parallel5")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .recipient_list(constant("async:hi:camel,async:hi:world,direct:foo5"))
                .parallel_processing()
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:foo5")
                .transform(constant("Bye Camel"))
                .to("mock:foo"),
        );
        r.add(
            from("direct:slip3")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .routing_slip(constant("async:hi:world, direct:slipFoo")),
        );
        r.add(
            from("direct:slipFoo")
                .process_fn(|e| {
                    assert_eq!(&Value::from("Hi World"), e.message().body());
                    bye_camel(e)
                })
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:sync")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:bye:camel?synchronous=true")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:syncFalse")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:bye:camel?synchronous=false")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
    })
    .await;
    for uri in [
        "direct:list",
        "direct:parallel",
        "direct:parallel2",
        "direct:parallel3",
        "direct:parallel4",
        "direct:parallel5",
        "direct:slip3",
        "direct:sync",
        "direct:syncFalse",
    ] {
        expect_standard(&t);
        let reply = t.template.request_body(uri, "Hello Camel").await.unwrap();
        assert_eq!(Value::from("Bye Camel"), reply, "{uri}");
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_async_endpoint_unit_of_work() {
    for failed in [false, true] {
        let (complete, failure) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (c, f) = (complete.clone(), failure.clone());
        let t = async_context(move |r: &mut Routes| {
            let (c, f) = (c.clone(), f.clone());
            let route = from("direct:start")
                .process_fn(move |e| {
                    let (c, f) = (c.clone(), f.clone());
                    e.add_on_completion(Arc::new(synchronization_fn(
                        move |_: &mut Exchange| {
                            c.fetch_add(1, Ordering::SeqCst);
                        },
                        move |_: &mut Exchange| {
                            f.fetch_add(1, Ordering::SeqCst);
                        },
                    )));
                    Ok(())
                })
                .to("mock:before")
                .to("log:before")
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after");
            let route = if failed {
                route.throw_exception(CamelError::illegal_argument("Damn"))
            } else {
                route
            };
            r.add(route.to("mock:result"));
        })
        .await;
        t.mock("mock:before")
            .expected_bodies_received(["Hello Camel"]);
        t.mock("mock:after").expected_bodies_received(["Bye Camel"]);
        if failed {
            // AsyncEndpointUoWFailedTest
            t.mock("mock:result").expected_message_count(0);
            let e = t
                .template
                .request_body("direct:start", "Hello Camel")
                .await
                .unwrap_err();
            assert!(cause_of(&e).is(ErrorKind::IllegalArgument));
            assert_eq!(Some("Damn"), cause_of(&e).message());
        } else {
            // AsyncEndpointUoWTest
            t.mock("mock:result")
                .expected_bodies_received(["Bye Camel"]);
            assert_eq!(
                Value::from("Bye Camel"),
                t.template
                    .request_body("direct:start", "Hello Camel")
                    .await
                    .unwrap()
            );
        }
        t.assert_mock_endpoints_satisfied().await;
        assert_eq!(
            (usize::from(!failed), usize::from(failed)),
            (
                complete.load(Ordering::SeqCst),
                failure.load(Ordering::SeqCst)
            )
        );
    }
}

#[tokio::test]
async fn test_async_endpoint_split_use_latest_and_redelivery() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:split")
                .split(body(), |s| {
                    s.to("mock:before")
                        .to("log:before")
                        .process_fn(noop)
                        .to("async:bye:camel")
                        .process_fn(noop)
                        .to("log:after")
                        .to("mock:after")
                })
                .aggregation_strategy(Arc::new(UseLatestAggregationStrategy))
                .to("mock:result"),
        );
    })
    .await;
    // AsyncEndpointSplitUseLatestAggregationStrategyTest
    t.mock("mock:before").expected_bodies_received(["A", "B"]);
    t.mock("mock:after")
        .expected_bodies_received(["Bye Camel", "Bye Camel"]);
    t.mock("mock:result")
        .expected_bodies_received(["Bye Camel"]);
    assert_eq!(
        Value::from("Bye Camel"),
        t.template
            .request_body("direct:split", "A,B")
            .await
            .unwrap()
    );
    t.assert_mock_endpoints_satisfied().await;
    // AsyncFailureProcessorWithRedeliveryTest
    let t = async_context(|r: &mut Routes| {
        r.error_handler(default_error_handler().maximum_redeliveries(5));
        r.add(
            from("direct:start")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:bye:camel?failFirstAttempts=2")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
    })
    .await;
    expect_standard(&t);
    assert_eq!(
        Value::from("Bye Camel"),
        t.template
            .request_body("direct:start", "Hello Camel")
            .await
            .unwrap()
    );
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_async_nested_triple_choice() {
    let t = async_context(|r: &mut Routes| {
        r.add(from("direct:start").choice(|c| {
            c.when(header("foo").is_greater_than(1), |w| {
                w.to("async:bye:camel").choice(|c| {
                    c.when(header("foo").is_greater_than(5), |w| {
                        w.to("async:bye:camel2").choice(|c| {
                            c.when(header("foo").is_greater_than(7), |w| w.to("mock:verybig"))
                                .otherwise(|o| o.to("mock:big"))
                        })
                    })
                    .otherwise(|o| o.to("mock:med"))
                })
            })
            .otherwise(|o| o.to("mock:low"))
        }));
    })
    .await;
    for (foo, target) in [
        (10, "mock:verybig"),
        (7, "mock:big"),
        (4, "mock:med"),
        (1, "mock:low"),
    ] {
        for m in ["mock:low", "mock:med", "mock:big", "mock:verybig"] {
            t.mock(m).reset();
            t.mock(m).expected_message_count(i32::from(m == target));
        }
        t.template
            .send_body_and_header("direct:start", "Hello World", "foo", Value::Int(foo))
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}
