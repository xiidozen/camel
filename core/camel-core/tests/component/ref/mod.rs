//! Ports of the ref component tests: `RefComponentTest`, `RefInvalidTest` and
//! `RefFileEndpointTest`.

use crate::support::{cause_of, ContextTestSupport, TestDirectory};
use camel_core::api::endpoint::endpoint_value;
use camel_core::api::exchange_constants::FILE_NAME;
use camel_core::prelude::*;
use std::sync::Arc;

/// `RefComponentTest`: a started direct consumer bound as `foo`.
#[tokio::test]
async fn test_ref() {
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    let endpoint = t.context.endpoint("direct:somename").unwrap();
    let template = Arc::new(t.context.create_producer_template());
    let consumer = endpoint
        .clone()
        .create_consumer(Arc::new(camel_core::api::processor::processor_fn(
            move |e: &mut Exchange| {
                let template = template.clone();
                let body = e.message().body().clone();
                tokio::spawn(async move { template.send_body("mock:result", body).await });
                Ok(())
            },
        )))
        .unwrap();
    consumer.start().await.unwrap();
    t.context
        .registry()
        .unwrap()
        .bind("foo", endpoint_value(endpoint))
        .unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("ref:foo", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// `RefInvalidTest`: the endpoint bound as `foo`, and an unknown one.
#[tokio::test]
async fn test_ref_invalid() {
    let t = ContextTestSupport::new();
    let seda = t.context.endpoint("seda:foo").unwrap();
    t.context
        .registry()
        .unwrap()
        .bind("foo", endpoint_value(seda))
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("ref:foo").route_id("foo").to("mock:result"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("ref:foo", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;

    let e = t
        .template
        .send_body("ref:xxx", "Hello World")
        .await
        .unwrap_err();
    assert_eq!(ErrorKind::ResolveEndpointFailed, e.kind());
    assert_eq!(
        Some("Failed to resolve endpoint: ref://xxx due to: No bean could be found in the registry for: xxx of type: org.apache.camel.Endpoint"),
        e.message()
    );
    assert_eq!(ErrorKind::NoSuchBean, cause_of(&e).kind());
}

/// `RefFileEndpointTest`: the route consumes from the referenced file endpoint (the Java
/// consumer delays are checked on its uri).
#[tokio::test]
async fn test_ref_file_endpoint() {
    let dir = TestDirectory::new("ref-file");
    let t = ContextTestSupport::new();
    let file = t
        .context
        .endpoint(&dir.file_uri("?initialDelay=250&delay=3000&delete=true"))
        .unwrap();
    t.context
        .registry()
        .unwrap()
        .bind("foo", endpoint_value(file))
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("ref:foo").route_id("foo").to("mock:result"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body_and_header(&dir.file_uri(""), "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let route = t.context.route("foo").unwrap();
    let uri = camel_core::api::route::Route::endpoint(route.as_ref())
        .endpoint_uri()
        .to_string();
    assert!(
        uri.contains("delay=3000") && uri.contains("initialDelay=250"),
        "{uri}"
    );
}
