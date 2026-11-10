//! Port of `SedaInOutTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_in_out() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to("seda:foo"));
        r.add(
            from("seda:foo")
                .transform(constant("Bye World"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    let out = t
        .template
        .request_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!("Bye World", out.to_string());
    t.assert_mock_endpoints_satisfied().await;
}
