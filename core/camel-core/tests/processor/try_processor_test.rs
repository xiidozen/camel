//! Ports of `TryProcessorTest`, `TryProcessorMultipleExceptionTest` and
//! `SimpleTryCatchFinallyTest`.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
use camel_core::api::processor::{expression_fn, predicate_fn};
use camel_core::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Java `new RuntimeCamelException(new CamelException("Force to fail"))`.
fn force_to_fail() -> CamelError {
    CamelError::wrap(
        ErrorKind::RuntimeCamel,
        CamelError::new(ErrorKind::Camel, "Force to fail"),
    )
}

fn handle(
    handled: Arc<AtomicBool>,
) -> impl Fn(&mut Exchange) -> CamelResult<()> + Send + Sync + 'static {
    move |e: &mut Exchange| {
        handled.store(true, Ordering::SeqCst);
        assert!(!e.is_failed(), "Should not be marked as failed");
        let caught = e
            .property(EXCEPTION_CAUGHT)
            .and_then(|v| v.downcast_ref::<CamelError>())
            .cloned();
        let caught = caught.expect("There should be an exception");
        let cause = std::error::Error::source(&caught)
            .and_then(|c| c.downcast_ref::<CamelError>())
            .unwrap();
        assert!(cause.is(ErrorKind::Camel));
        assert_eq!(Some("Force to fail"), cause.message());
        Ok(())
    }
}

async fn try_catch_finally(endpoint: &str) {
    let handled = Arc::new(AtomicBool::new(false));
    let h = handled.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let catch = |h: Arc<AtomicBool>| move |c: StepsDsl| c.process_fn(handle(h));
        r.add(
            from("direct:processor")
                .do_try(|t| {
                    t.process_fn(|_| Err(force_to_fail()))
                        .to("mock:result")
                        .do_catch(ErrorKind::Camel, catch(h.clone()))
                        .do_finally(|f| f.to("mock:finally"))
                })
                .to("mock:last"),
        );
        let fail_expression = Arc::new(expression_fn(|_| Err(force_to_fail())));
        r.add(
            from("direct:expression")
                .do_try(|t| {
                    t.set_body(ExpressionDefinition::Expression(fail_expression))
                        .to("mock:result")
                        .do_catch(ErrorKind::Camel, catch(h.clone()))
                        .do_finally(|f| f.to("mock:finally"))
                })
                .to("mock:last"),
        );
        r.add(
            from("direct:predicate")
                .do_try(|t| {
                    t.to("direct:sub-predicate")
                        .do_catch(ErrorKind::Camel, catch(h.clone()))
                        .do_finally(|f| f.to("mock:finally"))
                })
                .to("mock:last"),
        );
        let fail_predicate: Arc<dyn Predicate> = Arc::new(predicate_fn(|_| Err(force_to_fail())));
        r.add(from("direct:sub-predicate").filter(fail_predicate, |f| f.to("mock:result")));
    })
    .await;
    t.mock("mock:result").expected_message_count(0);
    t.mock("mock:last").expected_message_count(1);
    t.mock("mock:finally").expected_message_count(1);
    t.template
        .send_body(endpoint, "<test>Hello World!</test>")
        .await
        .unwrap();
    assert!(handled.load(Ordering::SeqCst), "Should have been handled");
    t.assert_mock_endpoints_satisfied().await;
}

use camel_core::model::expression::ExpressionDefinition;

#[tokio::test]
async fn test_try_catch_finally_processor() {
    try_catch_finally("direct:processor").await;
}

#[tokio::test]
async fn test_try_catch_finally_expression() {
    try_catch_finally("direct:expression").await;
}

#[tokio::test]
async fn test_try_catch_finally_predicate() {
    try_catch_finally("direct:predicate").await;
}

async fn multiple() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").do_try(|t| {
            t.process_fn(|e| match e.message().body_as::<String>()?.as_deref() {
                Some("Damn IO") => Err(CamelError::from(std::io::Error::other("Damn IO"))),
                Some("Damn State") => Err(CamelError::illegal_state("Damn State")),
                _ => Ok(()),
            })
            .to("mock:result")
            .do_catch(
                ["java.io.IOException", "java.lang.IllegalStateException"],
                |c| c.to("mock:catch"),
            )
            .do_finally(|f| f.to("mock:finally"))
        }));
    })
    .await
}

async fn expect(t: &ContextTestSupport, result: i32, catch: i32, body: &str) {
    t.mock("mock:result").expected_message_count(result);
    t.mock("mock:catch").expected_message_count(catch);
    t.mock("mock:finally").expected_message_count(1);
    t.template.send_body("direct:start", body).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_io_exception() {
    expect(&multiple().await, 0, 1, "Damn IO").await;
}

#[tokio::test]
async fn test_illegal_state_exception() {
    expect(&multiple().await, 0, 1, "Damn State").await;
}

#[tokio::test]
async fn test_ok() {
    expect(&multiple().await, 1, 0, "Hello World").await;
}

// SimpleTryCatchFinallyTest
#[tokio::test]
async fn test_simple_try_throw_exception_finally() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .do_try(|t| {
                    t.to("mock:try")
                        .throw_exception(CamelError::illegal_argument("Damn"))
                        .do_catch(ErrorKind::IllegalArgument, |c| c.to("mock:catch"))
                        .do_finally(|f| f.to("mock:finally"))
                })
                .to("mock:result"),
        );
    })
    .await;
    for m in ["mock:try", "mock:catch", "mock:finally", "mock:result"] {
        t.mock(m).expected_message_count(1);
    }
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
