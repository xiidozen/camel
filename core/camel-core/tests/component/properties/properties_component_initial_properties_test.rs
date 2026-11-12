//! Port of `PropertiesComponentInitialPropertiesTest`.

use super::{start, with_location};
use camel_core::prelude::*;

#[tokio::test]
async fn test_properties_component_endpoint() {
    let t = with_location();
    t.context
        .properties()
        .set_initial_properties([("first", "mock:first"), ("second", "second")]);
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("{{first}}")
                .to("mock:{{second}}")
                .to("{{cool.end}}"),
        );
    })
    .await;
    t.mock("mock:first").expected_message_count(1);
    t.mock("mock:second").expected_message_count(1);
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
