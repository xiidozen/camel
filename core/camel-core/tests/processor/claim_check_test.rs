//! Ports of the `ClaimCheckEip*Test`s (push/pop, get/set, getAndRemove, the filters and the
//! dynamic key).

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_push_pop() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:a")
                .claim_check("Push", None, None)
                .transform(constant("Bye World"))
                .to("mock:b")
                .claim_check("Pop", None, None)
                .to("mock:c"),
        );
    })
    .await;
    t.mock("mock:a").expected_bodies_received(["Hello World"]);
    t.mock("mock:b").expected_bodies_received(["Bye World"]);
    t.mock("mock:c").expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn get_set(operation: &str, last: &str) {
    let op = operation.to_string();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:a")
                .claim_check("Set", Some("foo"), None)
                .transform(constant("Bye World"))
                .to("mock:b")
                .claim_check(&op, Some("foo"), None)
                .to("mock:c")
                .transform(constant("Hi World"))
                .to("mock:d")
                .claim_check(&op, Some("foo"), None)
                .to("mock:e"),
        );
    })
    .await;
    t.mock("mock:a").expected_bodies_received(["Hello World"]);
    t.mock("mock:b").expected_bodies_received(["Bye World"]);
    t.mock("mock:c").expected_bodies_received(["Hello World"]);
    t.mock("mock:d").expected_bodies_received(["Hi World"]);
    t.mock("mock:e").expected_bodies_received([last]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_get_set() {
    get_set("Get", "Hello World").await;
}

#[tokio::test]
async fn test_get_and_remove_set() {
    get_set("GetAndRemove", "Hi World").await;
}

#[tokio::test]
async fn test_dynamic_key_get_set() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:a")
                .claim_check("Set", Some("${header.myKey}"), None)
                .transform(constant("Bye World"))
                .to("mock:b")
                .claim_check("Get", Some("${header.myKey}"), None)
                .to("mock:c")
                .transform(constant("Hi World"))
                .to("mock:d")
                .claim_check("Get", Some("${header.myKey}"), None)
                .to("mock:e"),
        );
    })
    .await;
    t.mock("mock:a")
        .expected_bodies_received(["Hello World", "Bonjour World"]);
    t.mock("mock:b")
        .expected_bodies_received(["Bye World", "Bye World"]);
    t.mock("mock:c")
        .expected_bodies_received(["Hello World", "Bonjour World"]);
    t.mock("mock:d")
        .expected_bodies_received(["Hi World", "Hi World"]);
    t.mock("mock:e")
        .expected_bodies_received(["Hello World", "Bonjour World"]);
    t.template
        .send_body_and_header("direct:start", "Hello World", "myHeader", "foo")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Bonjour World", "myHeader", "bar")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn push_pop_filter(filter: &str) -> ContextTestSupport {
    let filter = filter.to_string();
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header("bar", constant("Moes"))
                .to("mock:a")
                .claim_check("Push", None, None)
                .transform(constant("Bye World"))
                .set_header("foo", constant(456))
                .set_header("bar", constant("Jacks"))
                .to("mock:b")
                .claim_check("Pop", None, Some(&filter))
                .to("mock:c"),
        );
    })
    .await
}

#[tokio::test]
async fn test_push_pop_body() {
    let t = push_pop_filter("body").await;
    let c = t.mock("mock:c");
    c.expected_bodies_received(["Hello World"]);
    c.expected_header_received("foo", 456);
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_push_pop_headers() {
    let t = push_pop_filter("headers").await;
    let c = t.mock("mock:c");
    c.expected_bodies_received(["Bye World"]);
    c.expected_header_received("foo", 123);
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_push_pop_exclude_body() {
    let t = push_pop_filter("-body,-header:bar").await;
    let c = t.mock("mock:c");
    c.expected_bodies_received(["Bye World"]);
    c.expected_header_received("foo", 123);
    c.expected_header_received("bar", "Jacks");
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_push_pop_remove_header() {
    let t = push_pop_filter("-body,--header:bar").await;
    let c = t.mock("mock:c");
    c.expected_bodies_received(["Bye World"]);
    c.expected_header_received("foo", 123);
    c.message_matches(0, header("bar").is_null());
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_push_pop_headers_pattern() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:a")
                .claim_check("Push", None, None)
                .transform(constant("Bye World"))
                .set_header("foo", constant(456))
                .remove_header("bar")
                .to("mock:b")
                .claim_check("Pop", None, Some("header:(foo|bar)"))
                .to("mock:c"),
        );
    })
    .await;
    let b = t.mock("mock:b");
    b.message_matches(0, header("bar").is_null());
    let c = t.mock("mock:c");
    c.expected_bodies_received(["Bye World"]);
    c.expected_header_received("foo", 123);
    c.expected_header_received("bar", "Moes");
    c.expected_header_received("car", "Toyota");
    let mut e = Exchange::with_body(t.context.as_shared(), "Hello World");
    for (k, v) in [
        ("foo", Value::Int(123)),
        ("bar", "Moes".into()),
        ("car", "Toyota".into()),
    ] {
        e.message_mut().set_header(k, v);
    }
    t.template.send("direct:start", e).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
