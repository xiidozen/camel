//! Port of `PipelineTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

/// Copies the in message to the out message and counts the copies in a header.
fn in_to_out(e: &mut Exchange) -> CamelResult<()> {
    let counter = e
        .in_message()
        .header_as::<i64>("copy-counter")?
        .unwrap_or(0);
    let copy = e.in_message().clone();
    *e.out_message_mut() = copy;
    e.out_message_mut().set_header("copy-counter", counter + 1);
    Ok(())
}

/// Increments the number in the body (into the out message).
fn increment(e: &mut Exchange) -> CamelResult<()> {
    let number = e.in_message().body_as::<i64>()?.unwrap_or(0) + 1;
    e.out_message_mut().set_body(number);
    Ok(())
}

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").pipeline(|p| {
            p.to("direct:x")
                .to("direct:y")
                .to("direct:z")
                .to("mock:result")
        }));
        r.add(from("direct:x").process_fn(increment));
        r.add(from("direct:y").process_fn(increment));
        r.add(from("direct:z").process_fn(increment));
        r.add(
            from("direct:b")
                .process_fn(in_to_out)
                .process_fn(in_to_out)
                .process_fn(in_to_out),
        );
    })
    .await
}

#[tokio::test]
async fn test_send_message_through_a_pipeline() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_bodies_received([4]);
    let mut e = t
        .template
        .create_exchange("direct:a", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body(1);
    e.message_mut().set_header("foo", "bar");
    let results = t.template.send("direct:a", e).await.unwrap();
    result.assert_is_satisfied().await;
    assert_eq!(
        Some(4),
        results.message().body_as::<i64>().unwrap(),
        "Result body"
    );
}

#[tokio::test]
async fn test_results_returned() {
    let t = setup().await;
    let mut e = t
        .template
        .create_exchange("direct:b", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body("Hello World");
    let e = t.template.send("direct:b", e).await.unwrap();
    assert_eq!(
        Some("Hello World".to_string()),
        e.message().body_as::<String>().unwrap()
    );
    assert_eq!(
        Some(3),
        e.message().header_as::<i64>("copy-counter").unwrap()
    );
}

#[tokio::test]
async fn test_only_properties() {
    let t = setup().await;
    let mut e = t
        .template
        .create_exchange("direct:b", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_header("header", "headerValue");
    let e = t.template.send("direct:b", e).await.unwrap();
    assert_eq!(
        Some("headerValue".to_string()),
        e.message().header_as::<String>("header").unwrap()
    );
    assert_eq!(
        Some(3),
        e.message().header_as::<i64>("copy-counter").unwrap()
    );
}
