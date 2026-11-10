//! Port of `SedaRouteTest` (both Java tests are the same route).

use camel_core::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[tokio::test]
async fn test_seda_queue() {
    let context = camel_core::default_context();
    let latch = Arc::new(Notify::new());
    let l = latch.clone();
    context
        .add_routes(move |r: &mut Routes| {
            r.add(from("seda:test.a").to("seda:test.b"));
            let l = l.clone();
            r.add(from("seda:test.b").process_fn(move |_| {
                l.notify_one();
                Ok(())
            }));
        })
        .await
        .unwrap();
    context.start().await.unwrap();
    let endpoint = context.endpoint("seda:test.a").unwrap();
    let mut exchange = endpoint.create_exchange();
    exchange.message_mut().set_header("cheese", 123);
    let producer = endpoint.create_producer().unwrap();
    producer.process(&mut exchange).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), latch.notified())
            .await
            .is_ok()
    );
    context.stop().await.unwrap();
}
