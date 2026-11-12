//! Port of `LoopTest`, `LoopTestProcessor`, `LoopCopyTest`, `LoopNoCopyTest`,
//! `LoopDoWhileTest` and `LoopExceptionTest` (the xpath loop of `LoopTest` waits for the
//! xpath language).

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::{LOOP_INDEX, LOOP_SIZE};
use camel_core::api::processor::processor_fn;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// Java `LoopTestProcessor`: checks the loop size and index properties.
fn loop_test_processor(count: i64) -> Arc<dyn Processor> {
    let index = Arc::new(AtomicI64::new(0));
    Arc::new(processor_fn(move |e: &mut Exchange| {
        let c = e.property_as::<i64>(LOOP_SIZE)?;
        let i = e.property_as::<i64>(LOOP_INDEX)?;
        if c != Some(count) {
            return Err(CamelError::runtime(format!(
                "Invalid count value.  Expected {count} but was {c:?}"
            )));
        }
        let expected = index.fetch_add(1, Ordering::SeqCst);
        if i != Some(expected) {
            return Err(CamelError::runtime(format!(
                "Invalid index value.  Expected {expected} but was {i:?}"
            )));
        }
        Ok(())
    }))
}

async fn loop_context() -> ContextTestSupport {
    let loop_test = loop_test_processor(10);
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:a").loop_count(constant(8), |l| l.to("mock:result")));
        r.add(from("direct:b").loop_count(header("loop"), |l| l.to("mock:result")));
        r.add(
            from("direct:d")
                .loop_count(constant(2), |l| l.to("mock:result"))
                .to("mock:last"),
        );
        r.add(from("direct:e").loop_count(constant(10), |l| {
            l.process(loop_test.clone()).to("mock:result")
        }));
    })
    .await
}

async fn perform_loop_test(t: &ContextTestSupport, uri: &str, iterations: i32, header: &str) {
    let mock = t.mock("mock:result");
    mock.reset();
    mock.expected_message_count(iterations);
    t.template
        .send_body_and_header(uri, "<hello times='4'>world!</hello>", "loop", header)
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_counter_loop() {
    let t = loop_context().await;
    perform_loop_test(&t, "direct:a", 8, "6").await;
}

#[tokio::test]
async fn test_expression_loop() {
    let t = loop_context().await;
    perform_loop_test(&t, "direct:b", 6, "6").await;
}

#[tokio::test]
async fn test_loop_as_block() {
    let t = loop_context().await;
    t.mock("mock:last").expected_message_count(1);
    perform_loop_test(&t, "direct:d", 2, "6").await;
    t.mock("mock:last").assert_is_satisfied().await;
}

#[tokio::test]
async fn test_loop_with_invalid_expression() {
    let t = loop_context().await;
    let r = t
        .template
        .send_body_and_header("direct:b", "x", "loop", "invalid")
        .await;
    assert!(r.is_err(), "Exception expected for invalid expression");
}

#[tokio::test]
async fn test_loop_properties() {
    let t = loop_context().await;
    perform_loop_test(&t, "direct:e", 10, "6").await;
}

#[tokio::test]
async fn test_loop_copy() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .loop_count(constant(3), |l| {
                    l.transform(body().append("B")).to("mock:loop")
                })
                .copy()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:loop")
        .expected_bodies_received(["AB", "AB", "AB"]);
    t.mock("mock:result").expected_bodies_received(["AB"]);
    t.template.send_body("direct:start", "A").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_loop_no_copy() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .loop_count(constant(3), |l| {
                    l.transform(body().append("B")).to("mock:loop")
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:loop")
        .expected_bodies_received(["AB", "ABB", "ABBB"]);
    t.mock("mock:result").expected_bodies_received(["ABBB"]);
    t.template.send_body("direct:start", "A").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_loop_do_while() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:simple")
                .loop_do_while(simple("${body.length} <= 5"), |l| {
                    l.to("mock:loop").transform(body().append("A"))
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["AAAAAA"]);
    t.mock("mock:loop")
        .expected_bodies_received(["A", "AA", "AAA", "AAAA", "AAAAA"]);
    t.template.send_body("direct:simple", "A").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_loop_exception() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:dead"));
        r.add(
            from("direct:start")
                .loop_count(constant(3), |l| {
                    l.to("mock:loop")
                        .throw_exception(CamelError::illegal_argument("Forced"))
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:dead").expected_message_count(1);
    t.mock("mock:loop").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
