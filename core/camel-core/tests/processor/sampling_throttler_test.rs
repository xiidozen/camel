//! Port of the `SamplingThrottlerTest` cases (per number of messages and per period).

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use std::time::Duration;

#[tokio::test]
async fn test_sampling_per_message_count() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:sample-messageFrequency")
                .sample_messages(10)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(10);
    for i in 0..100 {
        t.template
            .send_body(
                "direct:sample-messageFrequency",
                format!("<message>{i}</message>"),
            )
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_sampling_per_period() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:sample").sample_period(200).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    // the first message, then one per period
    for _ in 0..5 {
        t.template.send_body("direct:sample", "x").await.unwrap();
    }
    assert_eq!(1, mock.received_counter());
    tokio::time::sleep(Duration::from_millis(250)).await;
    t.template.send_body("direct:sample", "x").await.unwrap();
    t.template.send_body("direct:sample", "x").await.unwrap();
    assert_eq!(2, mock.received_counter());
    assert_eq!(
        "Sample[1 Exchange per 10 messages received -> []]",
        from("direct:x").sample_messages(10).build().steps[0].to_string()
    );
}
