//! Ports of `ToVariableTest`, `ToVariableReceiveTest`, `ToDynamicIgnoreTest`,
//! `ToDynamicRawTest` and `ToDynamicPropertyPlaceholderTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;

#[tokio::test]
async fn test_to_variable() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:send")
                .set_variable("hello", simple("Camel"))
                .to("mock:before")
                .to_v("direct:foo", Some("hello"), None)
                .to("mock:result"),
        );
        r.add(
            from("direct:receive")
                .to_v("direct:foo", None, Some("bye"))
                .to("mock:after")
                .set_body(variable("bye"))
                .to("mock:result"),
        );
        r.add(
            from("direct:sendAndReceive")
                .set_variable("hello", simple("Camel"))
                .to("mock:before")
                .to_v("direct:foo", Some("hello"), Some("bye"))
                .to("mock:result"),
        );
        r.add(
            from("direct:foo")
                .set_header("foo", constant("Foo was here"))
                .transform(simple("Bye ${body}")),
        );
    })
    .await;
    let reset = |t: &ContextTestSupport| {
        for m in ["mock:before", "mock:after", "mock:result"] {
            t.mock(m).reset();
        }
    };
    // send
    t.mock("mock:before").expected_bodies_received(["World"]);
    t.mock("mock:before")
        .expected_variable_received("hello", "Camel");
    t.mock("mock:result")
        .expected_bodies_received(["Bye Camel"]);
    t.mock("mock:result")
        .expected_variable_received("hello", "Camel");
    t.template.send_body("direct:send", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    // receive
    reset(&t);
    t.mock("mock:after").expected_bodies_received(["World"]);
    t.mock("mock:after")
        .expected_variable_received("bye", "Bye World");
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.mock("mock:result")
        .expected_variable_received("bye", "Bye World");
    t.mock("mock:result")
        .message_matches(0, header("foo").is_null());
    t.template
        .send_body("direct:receive", "World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let received = t.mock("mock:result").received_exchanges();
    assert_eq!(
        Some(&Value::from("Foo was here")),
        received[0].variable("header:bye.foo")
    );
    // send and receive
    reset(&t);
    t.mock("mock:before").expected_bodies_received(["World"]);
    t.mock("mock:before")
        .expected_variable_received("hello", "Camel");
    t.mock("mock:result").expected_bodies_received(["World"]);
    t.mock("mock:result")
        .expected_variable_received("bye", "Bye Camel");
    t.template
        .send_body("direct:sendAndReceive", "World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_to_dynamic_ignore_invalid() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to_d("mock:${header.foo}")
                .ignore_invalid_endpoints(),
        );
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Hello Camel"]);
    t.mock("mock:bar").expected_bodies_received(["Hello World"]);
    t.template
        .send_body_and_header("direct:start", "Hello Camel", "foo", "foo")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", "bar")
        .await
        .unwrap();
    // an invalid uri (mock:) is ignored
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_to_dynamic_raw() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to_d("mock:RAW(se+ret)"));
    })
    .await;
    t.mock("mock:RAW(se+ret)")
        .expected_bodies_received(["Hello Camel"]);
    t.template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_to_dynamic_property_placeholder() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("foo", "${header.myHeader}");
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").to_d("mock:{{foo}}"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:cheese")
        .expected_bodies_received(["Hello Camel"]);
    t.mock("mock:cake")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body_and_header("direct:start", "Hello Camel", "myHeader", "cheese")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Hello World", "myHeader", "cake")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let e = t
        .template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::ResolveEndpointFailed), "{e}");
    assert!(cause.to_string().contains("mock:"), "{cause}");
}
