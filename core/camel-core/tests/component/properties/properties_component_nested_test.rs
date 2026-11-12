//! Port of `PropertiesComponentNestedTest` and `PropertiesComponentNestedFalseTest`.

use super::{constant_text, start, with_location};
use camel_core::prelude::*;

fn routes(r: &mut Routes) {
    r.add(
        from("direct:start")
            .set_body(constant_text("{{cool.concat}}"))
            .set_header(
                "foo",
                constant_text("Hello {{cool.concat}} and {{cool.other.name}} how are you?"),
            )
            .to("mock:result"),
    );
}

#[tokio::test]
async fn test_nested() {
    let t = with_location();
    t.context.properties().set_nested_placeholder(true);
    start(&t, routes).await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["mock:result"]);
    mock.expected_header_received("foo", "Hello mock:result and Cheese how are you?");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_nested_false() {
    let t = with_location();
    t.context.properties().set_nested_placeholder(false);
    start(&t, routes).await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["mock:{{cool.result}}"]);
    mock.expected_header_received("foo", "Hello mock:{{cool.result}} and Cheese how are you?");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
