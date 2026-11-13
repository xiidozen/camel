//! Ports of `OnExceptionContinueTest` and `OnExceptionContinuePredicateTest`.

use crate::support::caught_is;
use crate::support::ContextTestSupport;
use camel_core::model::expression::ExpressionDefinition;
use camel_core::prelude::*;

#[tokio::test]
async fn test_continued() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(on_exception(ErrorKind::IllegalArgument).continued(true));
        r.add(
            from("direct:start")
                .to("mock:start")
                .throw_exception(CamelError::illegal_argument("Forced"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:start").expected_message_count(1);
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.message_matches(0, caught_is(ErrorKind::IllegalArgument));
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `OnExceptionContinuePredicateTest`: a custom continued predicate, and steps.
#[tokio::test]
async fn test_continued_predicate() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let predicate_invoked = Arc::new(AtomicUsize::new(0));
    let processor_invoked = Arc::new(AtomicUsize::new(0));
    let (pi, pr) = (predicate_invoked.clone(), processor_invoked.clone());
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let pi = pi.clone();
        let predicate: camel_core::api::processor::SharedPredicate = Arc::new(
            camel_core::api::processor::predicate_fn(move |_: &Exchange| {
                pi.fetch_add(1, Ordering::SeqCst);
                Ok(true)
            }),
        );
        let pr = pr.clone();
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .continued_when(ExpressionDefinition::Predicate(predicate))
                .process_fn(move |_| {
                    pr.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .to("mock:me"),
        );
        r.add(
            from("direct:start")
                .to("mock:start")
                .throw_exception(CamelError::illegal_argument("Forced"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:me").expected_message_count(1);
    t.mock("mock:start").expected_message_count(1);
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.message_matches(0, caught_is(ErrorKind::IllegalArgument));
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(1, predicate_invoked.load(Ordering::SeqCst));
    assert_eq!(1, processor_invoked.load(Ordering::SeqCst));
}
