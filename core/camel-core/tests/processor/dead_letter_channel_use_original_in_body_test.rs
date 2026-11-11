//! Ports of `DeadLetterChannelUseOriginalInBodyTest` and `DeadLetterChannelNoRedeliveryTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

/// Java `MyThrowProcessor`.
fn my_throw_processor(e: &mut Exchange) -> CamelResult<()> {
    assert_eq!("Hello World", e.in_message().body().to_string());
    Err(CamelError::illegal_argument("Forced"))
}

async fn use_original_in_body() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        // will use original
        let a = dead_letter_channel("mock:a")
            .maximum_redeliveries(2)
            .redelivery_delay(0)
            .log_stack_trace(false)
            .use_original_message();
        // will NOT use original
        let b = dead_letter_channel("mock:b")
            .maximum_redeliveries(2)
            .redelivery_delay(0)
            .log_stack_trace(false);
        r.add(
            from("direct:a")
                .error_handler(a)
                .set_body(body().append(" World"))
                .process_fn(my_throw_processor),
        );
        r.add(
            from("direct:b")
                .error_handler(b)
                .set_body(body().append(" World"))
                .process_fn(my_throw_processor),
        );
    })
    .await
}

#[tokio::test]
async fn test_use_original_in_body() {
    let t = use_original_in_body().await;
    t.mock("mock:a").expected_bodies_received(["Hello"]);
    t.template.send_body("direct:a", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_do_not_use_original_in_body() {
    let t = use_original_in_body().await;
    t.mock("mock:b").expected_bodies_received(["Hello World"]);
    t.template.send_body("direct:b", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// DeadLetterChannelNoRedeliveryTest
#[tokio::test]
async fn test_dlc_no_redelivery() {
    let counter = Arc::new(AtomicI32::new(0));
    let c = counter.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.error_handler(
            dead_letter_channel("mock:dead")
                .use_original_message()
                .maximum_redeliveries(0),
        );
        r.add(
            from("direct:start")
                .to("mock:a")
                .process_fn({
                    let c = c.clone();
                    move |_| {
                        c.fetch_add(1, Ordering::SeqCst);
                        Err(CamelError::illegal_argument("Forced"))
                    }
                })
                .to("mock:b"),
        );
    })
    .await;
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:b").expected_message_count(0);
    t.mock("mock:dead").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        1,
        counter.load(Ordering::SeqCst),
        "Only the original attempt"
    );
}
