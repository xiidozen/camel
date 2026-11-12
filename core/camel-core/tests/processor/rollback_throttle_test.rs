//! Ports of `MarkRollbackOnlyTest`, `RollbackCustomMessageTest`, `RollbackDefaultMessageTest`
//! and Rust tests of the throttle and convertHeaderTo/convertVariableTo EIPs.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_mark_rollback_only() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:start")
                .transform(constant("We cannot do this"))
                .mark_rollback_only()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:start").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    let out = t
        .template
        .request_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!(Value::from("We cannot do this"), out);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_rollback_messages() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:custom").rollback_message("boo"));
        r.add(from("direct:default").rollback());
    })
    .await;
    let e = t
        .template
        .send_body("direct:custom", "Hello World")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::RollbackExchange), "{cause:?}");
    assert!(
        cause.message().unwrap_or_default().starts_with("boo"),
        "{cause}"
    );
    let e = t
        .template
        .send_body("direct:default", "Hello World")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::RollbackExchange), "{cause:?}");
    assert!(cause.message().is_some());
}

#[tokio::test]
async fn test_throttle_total_requests() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .throttle(constant(3))
                .time_period_millis(200)
                .to("mock:result"),
        );
        r.add(
            from("direct:reject")
                .throttle(constant(1))
                .time_period_millis(10_000)
                .reject_execution(true)
                .to("mock:rejected"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(6);
    let start = Instant::now();
    for i in 0..6 {
        t.template.send_body("direct:start", i).await.unwrap();
    }
    // 3 requests per 200 millis: the last 3 wait for the next period
    assert!(
        start.elapsed() >= Duration::from_millis(180),
        "{:?}",
        start.elapsed()
    );
    t.assert_mock_endpoints_satisfied().await;
    t.template.send_body("direct:reject", "a").await.unwrap();
    let e = t
        .template
        .send_body("direct:reject", "b")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(
        cause.is_subtype("ThrottlerRejectedExecutionException"),
        "{cause:?}"
    );
    assert!(
        cause
            .to_string()
            .contains("Exceeded the max throttle rate of 1 within 10000ms"),
        "{cause}"
    );
}

#[tokio::test]
async fn test_throttle_concurrent_requests() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:start?concurrentConsumers=4")
                .throttle(constant(1))
                .concurrent_requests_mode()
                .delay(constant(50))
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(4);
    let start = Instant::now();
    for i in 0..4 {
        t.template.send_body("seda:start", i).await.unwrap();
    }
    mock.assert_is_satisfied().await;
    // one at a time: 4 x 50 millis
    assert!(
        start.elapsed() >= Duration::from_millis(180),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(
        "Throttle[constant{1} maximum concurrent requests]",
        from("direct:x")
            .throttle(language("constant", "1"))
            .concurrent_requests_mode()
            .build()
            .steps[0]
            .to_string()
    );
}

#[tokio::test]
async fn test_convert_header_and_variable() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_variable("v", constant("42"))
                .convert_header_to("num", "Integer")
                .convert_variable_to("v", "Integer")
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_header_received("num", 123);
    mock.expected_variable_received("v", 42);
    t.template
        .send_body_and_header("direct:start", "x", "num", "123")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let e = t
        .template
        .send_body_and_header("direct:start", "x", "num", "abc")
        .await;
    assert!(e.is_err(), "abc is not an Integer");
}
