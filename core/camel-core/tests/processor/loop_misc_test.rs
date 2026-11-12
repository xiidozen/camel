//! Ports of `LoopThousandTest`, `LoopAsyncCopyTest`, `LoopAsyncNoCopyTest`,
//! `LoopCopyPropagateVariableAsResultTestTest`, `LoopCopyErrorTest` and
//! `LoopDoWhileDelayTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn test_loop_thousand() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let c = c.clone();
        r.add(
            from("direct:start")
                .loop_count(constant(1000), move |l| {
                    let c = c.clone();
                    l.process_fn(move |_| {
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .to("log:loop")
                })
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
    assert_eq!(1000, count.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_loop_async_copy_and_no_copy() {
    for copy in [true, false] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let route = from("direct:start").loop_count(constant(3), |l| {
                l.threads_pool(1)
                    .transform(body().append("B"))
                    .to("mock:loop")
            });
            let route = if copy { route.copy() } else { route };
            r.add(route.to("mock:result"));
        })
        .await;
        if copy {
            t.mock("mock:loop")
                .expected_bodies_received(["AB", "AB", "AB"]);
            t.mock("mock:result").expected_bodies_received(["AB"]);
        } else {
            t.mock("mock:loop")
                .expected_bodies_received(["AB", "ABB", "ABBB"]);
            t.mock("mock:result").expected_bodies_received(["ABBB"]);
        }
        t.template.send_body("direct:start", "A").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_loop_copy_propagate_variable() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .loop_count(constant(3), |l| {
                    l.transform(body().append("B"))
                        .set_variable("foo", simple("${body}"))
                        .to("mock:loop")
                })
                .copy()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:loop")
        .expected_bodies_received(["AB", "AB", "AB"]);
    t.mock("mock:loop").expected_variable_received("foo", "AB");
    t.mock("mock:result").expected_bodies_received(["AB"]);
    t.mock("mock:result")
        .expected_variable_received("foo", "AB");
    t.template.send_body("direct:start", "A").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_loop_copy_error() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        let invoked = Arc::new(AtomicUsize::new(0));
        r.add(
            from("direct:start")
                .loop_count(constant(3), move |l| {
                    let invoked = invoked.clone();
                    l.process_fn(move |_| {
                        if invoked.fetch_add(1, Ordering::SeqCst) > 1 {
                            return Err(CamelError::illegal_argument("Forced"));
                        }
                        Ok(())
                    })
                    .to("mock:loop")
                })
                .copy()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:loop").expected_message_count(2);
    t.mock("mock:result").expected_message_count(0);
    let e = t.template.send_body("direct:start", "A").await.unwrap_err();
    assert_eq!(Some("Forced"), cause_of(&e).message());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_loop_do_while_delay() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:simple")
                .loop_do_while(simple("${body.length} <= 5"), |l| {
                    l.to("mock:loop")
                        .transform(body().append("A"))
                        .delay(constant(10))
                })
                .to("mock:result"),
        );
        let short = camel_core::support::builder::named_predicate("length <= 5", |e| {
            Ok(e.message().body().to_string().len() <= 5)
        });
        r.add(
            from("direct:functional")
                .loop_do_while(short, |l| {
                    l.to("mock:loop")
                        .process_fn(|e| {
                            let b = format!("{}A", e.message().body());
                            e.message_mut().set_body(b);
                            Ok(())
                        })
                        .delay(constant(10))
                })
                .to("mock:result"),
        );
    })
    .await;
    for uri in ["direct:simple", "direct:functional"] {
        t.mock("mock:result").reset();
        t.mock("mock:loop").reset();
        t.mock("mock:result").expected_bodies_received(["AAAAAA"]);
        t.mock("mock:loop")
            .expected_bodies_received(["A", "AA", "AAA", "AAAA", "AAAAA"]);
        t.template.send_body(uri, "A").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}
