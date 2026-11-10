//! Port of `AggregateForceCompletionOnStopTest` (the shutdown strategy options of the Java test
//! do not apply: stopping completes the groups before returning).

use crate::support::{body_in_aggregating_strategy, ContextTestSupport};
use camel_core::api::processor::{processor_fn, SharedProcessor};
use camel_core::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The routes, and the counter of the Java `MyCompletionProcessor`.
async fn setup() -> (ContextTestSupport, Arc<AtomicUsize>) {
    let t = ContextTestSupport::new();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let completion: SharedProcessor = Arc::new(processor_fn(move |_e: &mut Exchange| {
        c.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    t.context
        .simple_registry()
        .bind("myCompletionProcessor", Value::object(completion))
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:forceCompletionTrue")
                    .route_id("foo")
                    .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                        a.delay(constant(100)).process_ref("myCompletionProcessor")
                    })
                    .force_completion_on_stop()
                    .completion_size(10),
            );
            r.add(
                from("direct:forceCompletionFalse")
                    .route_id("bar")
                    .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                        a.delay(constant(100)).process_ref("myCompletionProcessor")
                    })
                    .completion_size(10),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    (t, count)
}

async fn send4(t: &ContextTestSupport, uri: &str) {
    for (b, id) in [
        ("test1", "1"),
        ("test2", "2"),
        ("test3", "1"),
        ("test4", "2"),
    ] {
        t.template
            .send_body_and_header(uri, b, "id", id)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_force_completion_true() {
    let (t, count) = setup().await;
    send4(&t, "direct:forceCompletionTrue").await;
    assert_eq!(
        0,
        count.load(Ordering::SeqCst),
        "aggregation should not have completed yet"
    );
    t.context.stop().await.unwrap();
    assert_eq!(
        2,
        count.load(Ordering::SeqCst),
        "aggregation should have completed"
    );
}

#[tokio::test]
async fn test_force_completion_false() {
    let (t, count) = setup().await;
    send4(&t, "direct:forceCompletionFalse").await;
    assert_eq!(
        0,
        count.load(Ordering::SeqCst),
        "aggregation should not have completed yet"
    );
    t.context.stop().await.unwrap();
    assert_eq!(
        0,
        count.load(Ordering::SeqCst),
        "aggregation should not have completed yet"
    );
}

#[tokio::test]
async fn test_stop_route_force_completion_true() {
    let (t, count) = setup().await;
    send4(&t, "direct:forceCompletionTrue").await;
    assert_eq!(
        0,
        count.load(Ordering::SeqCst),
        "aggregation should not have completed yet"
    );
    t.context.stop_route("foo").await.unwrap();
    assert_eq!(
        2,
        count.load(Ordering::SeqCst),
        "aggregation should have completed"
    );
}

#[tokio::test]
async fn test_stop_route_force_completion_false() {
    let (t, count) = setup().await;
    send4(&t, "direct:forceCompletionFalse").await;
    assert_eq!(
        0,
        count.load(Ordering::SeqCst),
        "aggregation should not have completed yet"
    );
    t.context.stop_route("bar").await.unwrap();
    assert_eq!(
        0,
        count.load(Ordering::SeqCst),
        "aggregation should not have completed yet"
    );
}
