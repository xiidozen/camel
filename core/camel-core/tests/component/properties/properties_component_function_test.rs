//! Port of `PropertiesComponentFunctionTest` (Rust functions have no lifecycle, so the
//! started/stopped checks are n/a).

use super::start;
use crate::support::ContextTestSupport;
use camel_core::api::spi::properties::PropertiesFunction;
use camel_core::prelude::*;
use std::sync::Arc;

/// Java `MyFunction`: `{{beer:FOO}}` is `mock:foo`.
struct MyFunction;

impl PropertiesFunction for MyFunction {
    fn name(&self) -> &str {
        "beer"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        Some(format!("mock:{}", remainder.to_lowercase()))
    }
}

#[tokio::test]
async fn test_function() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_properties_function(Arc::new(MyFunction));
    assert!(t.context.properties().has_properties_function("beer"));
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("log:{{sys:os.name:linux}}")
                .to("{{beer:FOO}}")
                .to("{{beer:BAR}}"),
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
