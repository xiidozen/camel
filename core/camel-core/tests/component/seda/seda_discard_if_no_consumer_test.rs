//! Port of `SedaDiscardIfNoConsumerTest` (`testDiscardUoW` waits for the unit of work).

use super::queue_size;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_discard() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("seda:bar?discardIfNoConsumers=true")
                .to("mock:result"),
        );
    })
    .await;
    assert_eq!(0, queue_size(&t.context, "seda:bar"));
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(0, queue_size(&t.context, "seda:bar"));
}
