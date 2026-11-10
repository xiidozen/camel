//! Port of `DefaultErrorHandlerRedeliveryTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

async fn run(uri: &str, expected: i64) {
    let counter = Arc::new(AtomicI64::new(0));
    let c = counter.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let fail = |c: Arc<AtomicI64>| {
            move |_: &mut Exchange| {
                c.fetch_add(1, Ordering::SeqCst);
                Err(CamelError::new(
                    ErrorKind::General,
                    "Forced exception by unit test",
                ))
            }
        };
        r.add(
            from("direct:start")
                .error_handler(
                    default_error_handler()
                        .redelivery_delay(0)
                        .maximum_redeliveries(2),
                )
                .process_fn(fail(c.clone())),
        );
        r.add(from("direct:no").process_fn(fail(c.clone())));
        r.add(
            from("direct:one")
                .error_handler(
                    default_error_handler()
                        .redelivery_delay(0)
                        .maximum_redeliveries(1),
                )
                .process_fn(fail(c.clone())),
        );
    })
    .await;
    let err = t.template.send_body(uri, "Hello World").await.unwrap_err();
    assert!(err.is(ErrorKind::RuntimeCamel), "{err:?}");
    assert_eq!(expected, counter.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_redelivery_test() {
    run("direct:start", 3).await;
}

#[tokio::test]
async fn test_no_redeliveries_test() {
    run("direct:no", 1).await;
}

#[tokio::test]
async fn test_one_redelivery_test() {
    run("direct:one", 2).await;
}
