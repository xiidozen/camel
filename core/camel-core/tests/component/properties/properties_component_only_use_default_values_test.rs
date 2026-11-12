//! Port of `PropertiesComponentOnlyUseDefaultValuesTest` (the routes fail when added in
//! Rust, Java when the context starts).

use super::start;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_only_defaults() {
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("{{foo:mock:foo}}")
                .to("{{bar:mock:bar}}"),
        );
    })
    .await;
    t.mock("mock:foo").expected_message_count(1);
    t.mock("mock:bar").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_one_missing() {
    let t = ContextTestSupport::new();
    let result = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").to("{{foo:mock:foo}}").to("{{bar}}"));
        })
        .await;
    let failed = match result {
        Err(_) => true,
        Ok(_) => t.context.start().await.is_err(),
    };
    assert!(failed, "Should have thrown exception");
}
