//! Port of `PropertiesComponentDefaultFunctionsTest`.

use super::{constant_text, start};
use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::util::system_properties;

#[tokio::test]
async fn test_function() {
    system_properties::set_property("FOO", "mock:foo");
    let (key, value) = std::env::vars()
        .find(|(_, v)| !v.is_empty())
        .expect("an environment variable");
    let t = ContextTestSupport::new();
    let env = format!("{{{{env:{key}}}}}");
    start(&t, move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("{{sys:FOO}}")
                .transform(constant_text(&env))
                .to("mock:bar"),
        );
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Hello World"]);
    t.mock("mock:bar").expected_bodies_received([value]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    system_properties::clear_property("FOO");
}

#[tokio::test]
async fn test_function_get_or_else() {
    system_properties::set_property("FOO2", "mock:foo");
    let t = ContextTestSupport::new();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("{{sys:FOO2}}")
                .to("{{env:BAR2:mock:bar}}"),
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
    system_properties::clear_property("FOO2");
}
