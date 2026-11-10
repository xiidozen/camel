//! Port of `ToDynamicTest` and `ToDynamicEmptyTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;

#[tokio::test]
async fn test_to_dynamic() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to_d("mock:${header.foo}"));
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
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_to_dynamic_invalid() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to_d("mock:${header.foo}"));
    })
    .await;
    let err = t
        .template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap_err();
    assert!(err.is(ErrorKind::CamelExecution), "{err:?}");
    assert!(
        cause_of(&err).is(ErrorKind::ResolveEndpointFailed),
        "{err:?}"
    );
}

// ToDynamicEmptyTest
#[tokio::test]
async fn test_to_dynamic_empty() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to_d("${header.foo}"));
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Hello Camel"]);
    t.mock("mock:bar").expected_bodies_received(["Hello World"]);
    t.template
        .send_body_and_header("direct:start", "Hello Camel", "foo", "mock:foo")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", "mock:bar")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    // no header: nothing to send to
    t.template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap();
}
