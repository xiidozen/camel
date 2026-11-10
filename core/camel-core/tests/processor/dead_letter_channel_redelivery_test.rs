//! Port of `DeadLetterChannelRedeliveryTest`.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::REDELIVERY_COUNTER;
use camel_core::api::spi::registry::Registry;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

struct Counters {
    counter: Arc<AtomicI64>,
    redelivery: Arc<AtomicI64>,
}

async fn setup() -> (ContextTestSupport, Counters) {
    let counter = Arc::new(AtomicI64::new(0));
    let redelivery = Arc::new(AtomicI64::new(0));
    let t = ContextTestSupport::new();
    let r = redelivery.clone();
    let on_redelivery: Arc<dyn Processor> = Arc::new(camel_core::api::processor::processor_fn(
        move |e: &mut Exchange| {
            r.store(
                e.in_message()
                    .header_as::<i64>(REDELIVERY_COUNTER)?
                    .unwrap_or(0),
                Ordering::SeqCst,
            );
            Ok(())
        },
    ));
    t.context
        .simple_registry()
        .bind("redeliveryProcessor", Value::object(on_redelivery))
        .unwrap();
    let c = counter.clone();
    t.context
        .add_routes(move |routes: &mut Routes| {
            for (uri, max) in [("direct:two", 2), ("direct:no", 0), ("direct:one", 1)] {
                let c = c.clone();
                let mut eh = dead_letter_channel("mock:error")
                    .maximum_redeliveries(max)
                    .on_redelivery_ref("redeliveryProcessor");
                if max > 0 {
                    eh = eh.redelivery_delay(0);
                }
                routes.add(from(uri).error_handler(eh).process_fn(move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(CamelError::new(
                        ErrorKind::General,
                        "Forced exception by unit test",
                    ))
                }));
            }
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    (
        t,
        Counters {
            counter,
            redelivery,
        },
    )
}

async fn run(uri: &str, counter: i64, redelivery: i64) {
    let (t, c) = setup().await;
    t.mock("mock:error").expected_message_count(1);
    t.template.send_body(uri, "Hello World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(counter, c.counter.load(Ordering::SeqCst));
    assert_eq!(redelivery, c.redelivery.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_two_redelivery_test() {
    run("direct:two", 3, 2).await;
}

#[tokio::test]
async fn test_no_redeliveries_test() {
    run("direct:no", 1, 0).await;
}

#[tokio::test]
async fn test_one_redeliveries_test() {
    run("direct:one", 2, 1).await;
}
