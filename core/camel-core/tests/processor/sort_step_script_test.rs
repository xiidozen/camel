//! Port of `SortBodyTest`, `SortExpressionTest`, `StepTest`, `ScriptTest`,
//! `RemovePropertyTest`, `RemoveVariableTest` and Rust tests of setHeaders/setVariables.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::STEP_ID;
use camel_core::prelude::*;
use camel_core::support::object_helper::type_coerce_compare;

fn texts(v: &Value) -> Vec<String> {
    match v {
        Value::List(l) => l.iter().map(|v| v.to_string()).collect(),
        other => panic!("not a list: {other:?}"),
    }
}

#[tokio::test]
async fn test_sort_body() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").sort_body().to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    let names: Vec<Value> = ["Hadrian", "William", "Claus"].map(Value::from).to_vec();
    t.template
        .send_body("direct:start", Value::List(names))
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let body = mock.received_exchanges()[0].message().body().clone();
    assert_eq!(vec!["Claus", "Hadrian", "William"], texts(&body));
}

#[tokio::test]
async fn test_sort_expression() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").sort(body()).to("mock:result"));
        r.add(
            from("direct:reverse")
                .sort_with(body(), |a, b| type_coerce_compare(a, b).reverse())
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    t.template
        .send_body("direct:start", "Hadrian,William,Claus")
        .await
        .unwrap();
    t.template
        .send_body("direct:reverse", "Hadrian,William,Claus")
        .await
        .unwrap();
    let received = mock.received_exchanges();
    assert_eq!(
        vec!["Claus", "Hadrian", "William"],
        texts(received[0].message().body())
    );
    assert_eq!(
        vec!["William", "Hadrian", "Claus"],
        texts(received[1].message().body())
    );
}

#[tokio::test]
async fn test_step() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .step("foo", |s| s.to("log:foo").to("mock:foo"))
                .step("bar", |s| s.to("log:bar").to("mock:bar"))
                .to("mock:result"),
        );
    })
    .await;
    let foo = t.mock("mock:foo");
    foo.expected_message_count(1);
    foo.expected_property_received(STEP_ID, "foo");
    let bar = t.mock("mock:bar");
    bar.expected_message_count(1);
    bar.expected_property_received(STEP_ID, "bar");
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    result.message_matches(0, exchange_property(STEP_ID).is_null());
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_script() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .script(body().append(" World!"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["Hello"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_remove_properties_and_variables() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_property("foo", constant("a"))
                .set_property("fooBar", constant("b"))
                .set_property("bar", constant("c"))
                .set_variables([("v1", constant(1)), ("v2", constant(2))])
                .remove_property("bar")
                .remove_properties_excluding("foo*", &["fooBar"])
                .remove_variable("v1")
                .to("mock:result")
                .remove_variable("*")
                .to("mock:all"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.message_matches(0, exchange_property("foo").is_null());
    mock.message_matches(0, exchange_property("bar").is_null());
    mock.expected_property_received("fooBar", "b");
    mock.message_matches(0, variable("v1").is_null());
    mock.expected_variable_received("v2", 2);
    let all = t.mock("mock:all");
    all.message_matches(0, variable("v2").is_null());
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_set_headers() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_headers([
                    ("a", simple("${body}")),
                    ("b", simple("${header.a} and more")),
                ])
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_header_received("a", "Hello");
    mock.expected_header_received("b", "Hello and more");
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
