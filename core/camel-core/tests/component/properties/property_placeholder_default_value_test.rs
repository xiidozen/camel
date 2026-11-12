//! Port of `PropertyPlaceholderDefaultValueTest`.

use super::start;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_using_default_value() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(from("direct:start").to("mock:{{foo:bar}}"));
    })
    .await;
    t.mock("mock:bar").expected_bodies_received(["Hello Camel"]);
    t.mock("mock:foo").expected_message_count(0);
    t.template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
