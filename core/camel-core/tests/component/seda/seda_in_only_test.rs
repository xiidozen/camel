//! Port of `SedaInOnlyTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_in_only() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to("seda:foo"));
        r.add(from("seda:foo").to("mock:result"));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
