//! Port of `SameSedaQueueSizeAndNoSizeTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:foo?size=100")
                .route_id("foo")
                .auto_startup(false)
                .to("mock:foo"),
        );
        r.add(
            from("seda:bar")
                .route_id("bar")
                .auto_startup(false)
                .to("mock:bar"),
        );
    })
    .await
}

fn cause(e: &CamelError) -> &CamelError {
    std::error::Error::source(e)
        .and_then(|c| c.downcast_ref::<CamelError>())
        .expect("a cause")
}

#[tokio::test]
async fn test_same_queue() {
    let t = setup().await;
    for i in 0..100 {
        t.template
            .send_body("seda:foo", i.to_string())
            .await
            .unwrap();
    }
    let e = t
        .template
        .send_body("seda:foo", "Should be full now")
        .await
        .unwrap_err();
    assert!(e.is(ErrorKind::CamelExecution));
    assert!(cause(&e).is(ErrorKind::IllegalState));
    assert_eq!(Some("Queue full"), cause(&e).message());
}

#[tokio::test]
async fn test_same_queue_different_size() {
    let t = setup().await;
    let e = t
        .template
        .send_body("seda:foo?size=200", "Should fail")
        .await
        .unwrap_err();
    assert!(e.is(ErrorKind::ResolveEndpointFailed), "{e:?}");
    assert!(cause(&e).is(ErrorKind::IllegalArgument));
    assert_eq!(
        Some("Cannot use existing queue seda://foo as the existing queue size 100 does not match given queue size 200"),
        cause(&e).message()
    );
}

#[tokio::test]
async fn test_same_queue_different_size_bar() {
    let t = setup().await;
    let e = t
        .template
        .send_body("seda:bar?size=200", "Should fail")
        .await
        .unwrap_err();
    assert!(e.is(ErrorKind::ResolveEndpointFailed));
    assert_eq!(
        Some(format!(
            "Cannot use existing queue seda://bar as the existing queue size {} does not match given queue size 200",
            camel_seda::QUEUE_SIZE
        ))
        .as_deref(),
        cause(&e).message()
    );
}
