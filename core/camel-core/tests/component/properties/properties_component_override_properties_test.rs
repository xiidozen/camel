//! Port of `PropertiesComponentOverridePropertiesTest` and
//! `PropertiesComponentOverridePropertiesNonStringTest`.

use super::{start, with_location};
use camel_core::prelude::*;

#[tokio::test]
async fn test_properties_component_endpoint() {
    let t = with_location();
    t.context
        .properties()
        .set_override_properties([("cool.result", "extra"), ("hey", "mock:hey")]);
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("{{hey}}")
                .to("mock:{{cool.result}}"),
        );
    })
    .await;
    t.mock("mock:extra").expected_message_count(1);
    t.mock("mock:hey").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[test]
fn test_non_string_values() {
    let t = with_location();
    t.context
        .properties()
        .add_override_property("myQueueSize", 20);
    t.context
        .properties()
        .add_override_property("enabled", true);
    assert_eq!(
        Some("20".to_string()),
        super::resolve(&t, "{{myQueueSize}}")
    );
    assert_eq!(
        Some("false".to_string()),
        super::resolve(&t, "{{!enabled}}")
    );
}
