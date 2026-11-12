//! Port of `IdempotentConsumerTest`, `IdempotentConsumerDslTest` and
//! `IdempotentConsumerNoMessageIdTest`.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::DUPLICATE_MESSAGE;
use camel_core::api::processor::processor_fn;
use camel_core::api::spi::idempotent_repository::SharedIdempotentRepository;
use camel_core::prelude::*;
use camel_core::support::idempotent::MemoryIdempotentRepository;
use std::sync::Arc;

fn memory(size: usize) -> SharedIdempotentRepository {
    Arc::new(MemoryIdempotentRepository::with_max_cache_size(size))
}

async fn send_messages(t: &ContextTestSupport) {
    for (id, body) in [
        ("1", "one"),
        ("2", "two"),
        ("1", "one"),
        ("2", "two"),
        ("1", "one"),
        ("3", "three"),
    ] {
        // a failing exchange is expected for some tests
        let _ = t
            .template
            .send_body_and_header("direct:start", body, "messageId", id)
            .await;
    }
}

async fn start(t: &ContextTestSupport, routes: impl RouteBuilder) {
    t.context.add_routes(routes).await.unwrap();
    t.context.start().await.unwrap();
}

#[tokio::test]
async fn test_duplicate_messages_are_filtered_out() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .idempotent_consumer(header("messageId"), memory(10), |b| b.to("mock:result")),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["one", "two", "three"]);
    send_messages(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_not_skip_duplicate() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .idempotent_consumer(header("messageId"), memory(200), |b| b.to("mock:result"))
                .skip_duplicate(false),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["one", "two", "one", "two", "one", "three"]);
    for i in [0, 1, 5] {
        mock.message_matches(i, exchange_property(DUPLICATE_MESSAGE).is_null());
    }
    for i in [2, 3, 4] {
        mock.message_matches(i, exchange_property(DUPLICATE_MESSAGE).is_equal_to(true));
    }
    send_messages(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_not_skip_duplicate_with_filter() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .idempotent_consumer(header("messageId"), memory(200), |b| {
                    b.filter(
                        exchange_property(DUPLICATE_MESSAGE).is_equal_to(true),
                        |f| f.to("mock:duplicate").stop(),
                    )
                    .to("mock:result")
                })
                .skip_duplicate(false),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["one", "two", "three"]);
    let duplicate = t.mock("mock:duplicate");
    duplicate.expected_bodies_received(["one", "two", "one"]);
    duplicate.all_messages_match(exchange_property(DUPLICATE_MESSAGE).is_equal_to(true));
    send_messages(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

fn fail_on_2() -> Arc<dyn Processor> {
    Arc::new(processor_fn(|e: &mut Exchange| {
        let id = e.message().header("messageId").map(|v| v.to_string());
        if id.as_deref() == Some("2") {
            return Err(CamelError::illegal_argument("Damm I cannot handle id 2"));
        }
        Ok(())
    }))
}

#[tokio::test]
async fn test_failed_exchanges_not_added_dead_letter_channel() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.error_handler(
            dead_letter_channel("mock:error")
                .maximum_redeliveries(2)
                .redelivery_delay(0),
        );
        r.add(
            from("direct:start").idempotent_consumer(header("messageId"), memory(200), |b| {
                b.process(fail_on_2()).to("mock:result")
            }),
        );
    })
    .await;
    // the 2 messages with id 2 fail
    t.mock("mock:error").expected_message_count(2);
    t.mock("mock:result")
        .expected_bodies_received(["one", "three"]);
    send_messages(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_failed_exchanges_not_added() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start").idempotent_consumer(header("messageId"), memory(200), |b| {
                b.process(fail_on_2()).to("mock:result")
            }),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["one", "three"]);
    send_messages(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_registry_repository_and_no_message_id() {
    let t = ContextTestSupport::new();
    let repo = memory(100);
    t.context
        .registry()
        .unwrap()
        .bind(
            "myRepo",
            camel_core::support::idempotent::repository_value(repo.clone()),
        )
        .unwrap();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .idempotent_consumer(header("messageId"), "myRepo", |b| b.to("mock:result")),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["one", "two", "three"]);
    send_messages(&t).await;
    t.assert_mock_endpoints_satisfied().await;
    assert!(repo.contains("3"));
    // Java IdempotentConsumerNoMessageIdTest
    let e = t
        .template
        .send_body("direct:start", "no id")
        .await
        .expect_err("no message id");
    let cause = crate::support::cause_of(&e);
    assert!(cause.is_subtype("NoMessageIdException"), "{cause:?}");
    assert!(
        cause
            .to_string()
            .contains("No message ID could be found using expression: header(messageId)"),
        "{cause}"
    );
}
