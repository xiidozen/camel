//! Ports of `RecipientListParallelTest`, `RecipientListParallelTimeoutTest` (the check of
//! the Java aggregate executor service is not applicable) and
//! `RecipientListParallelStreamingTest`.

use crate::support::{concat_strategy, ContextTestSupport};
use camel_core::prelude::*;

#[tokio::test]
async fn test_recipient_list_parallel() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .parallel_processing(),
        );
        for (name, delay) in [("a", 1000), ("b", 500), ("c", 0)] {
            r.add(
                from(&format!("direct:{name}"))
                    .delay(constant(delay))
                    .transform(constant(name))
                    .to("mock:result"),
            );
        }
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received_in_any_order(["c", "b", "a"]);
    t.template
        .send_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_recipient_list_parallel_timeout() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("slip"))
                .aggregation_strategy(concat_strategy())
                .parallel_processing()
                .timeout(500)
                .to("mock:result"),
        );
        for (name, delay) in [("a", 1000), ("b", 0), ("c", 100)] {
            r.add(
                from(&format!("direct:{name}"))
                    .delay(constant(delay))
                    .set_body(constant(name.to_uppercase().as_str())),
            );
        }
    })
    .await;
    t.mock("mock:result")
        .message_matches(0, not(body().contains("A")));
    t.template
        .send_body_and_header(
            "direct:start",
            "Hello",
            "slip",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn streaming() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .parallel_processing()
                .to("mock:result"),
        );
        r.add(
            from("direct:streaming")
                .recipient_list(header("foo"))
                .parallel_processing()
                .streaming()
                .to("mock:result"),
        );
        for (name, delay) in [("a", 500), ("b", 1000), ("c", 0)] {
            r.add(
                from(&format!("direct:{name}"))
                    .delay(constant(delay))
                    .transform(constant(name)),
            );
        }
    })
    .await
}

#[tokio::test]
async fn test_recipient_list_parallel_ordered() {
    let t = streaming().await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["c"]);
    t.template
        .send_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_recipient_list_parallel_streaming() {
    let t = streaming().await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["b"]);
    t.template
        .send_body_and_header(
            "direct:streaming",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}
