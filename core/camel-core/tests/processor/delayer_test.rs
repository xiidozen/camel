//! Port of `DelayerTest` (the bean cases wait for the bean language).

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("seda:a").delay(header("MyDelay")).to("mock:result"));
        r.add(from("seda:b").delay(constant(100)).to("mock:result"));
    })
    .await
}

#[tokio::test]
async fn test_sending_message_gets_delayed() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_message_count(0);
    result.set_result_wait_time(10);
    t.template
        .send_body_and_header("seda:a", "<hello>world!</hello>", "MyDelay", 100)
        .await
        .unwrap();
    result.assert_is_satisfied().await;
    result.reset();
    result.expected_message_count(1);
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_delay_constant() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    result.set_result_minimum_wait_time(90);
    t.template
        .send_body("seda:b", "<hello>world!</hello>")
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}
