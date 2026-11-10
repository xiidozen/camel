//! Port of `SedaDiscardWhenFullTest`.

use crate::support::{await_until, ContextTestSupport};
use camel_core::prelude::*;
use std::time::Duration;

#[tokio::test]
async fn test_discard_when_full() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:foo?size=2")
                .route_id("foo")
                .auto_startup(false)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(["Hello World", "Bye World", "Camel World"]);
    for body in ["Hello World", "Bye World", "Hi World"] {
        t.template
            .send_body("seda:foo?discardWhenFull=true", body)
            .await
            .unwrap();
    }
    t.context.start_route("foo").await.unwrap();
    await_until(Duration::from_secs(10), || mock.received_counter() >= 1).await;
    t.template
        .send_body("seda:foo?discardWhenFull=true", "Camel World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
