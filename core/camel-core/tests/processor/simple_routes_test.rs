//! Ports of small route tests: `StaticRecipientListTest`, `VerySimpleDirectTest`,
//! `SimpleDirectTest`, `SimpleTryFinallyTest`, `RegExRouteTest`,
//! `CBRSimplePredicateEmptyBodyTest` and `SendToNonExistingEndpointTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_static_recipient_list() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").to("mock:x").to("mock:y").to("mock:z"));
    })
    .await;
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["answer"]);
    }
    t.template.send_body("direct:a", "answer").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_very_simple_and_simple_direct() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:very").to("mock:very"));
        r.add(
            from("direct:start")
                .to("direct:foo")
                .to("direct:bar")
                .to("mock:result"),
        );
        r.add(from("direct:foo").to("mock:foo"));
        r.add(from("direct:bar").to("mock:bar"));
    })
    .await;
    for m in ["mock:very", "mock:foo", "mock:bar", "mock:result"] {
        t.mock(m).expected_message_count(1);
    }
    t.template
        .send_body("direct:very", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_simple_try_finally() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .do_try(|t| t.to("mock:try").do_finally(|f| f.to("mock:finally")))
                .to("mock:result"),
        );
    })
    .await;
    for m in ["mock:try", "mock:finally", "mock:result"] {
        t.mock(m).expected_message_count(1);
    }
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_reg_ex_route() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_body(body().regex_replace_all("Foo", "Bar"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Bar and another Bar"]);
    t.template
        .send_body("direct:start", "Foo and another Foo")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_cbr_simple_predicate_empty_body() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").choice(|c| {
            c.when(simple("${header.name} == null"), |w| w.to("mock:unknown"))
                .otherwise(|o| o.to("mock:known"))
        }));
    })
    .await;
    let unknown = t.mock("mock:unknown");
    unknown.expected_message_count(1);
    unknown.message_matches(0, header("name").is_null());
    let known = t.mock("mock:known");
    known.expected_message_count(1);
    known.message_matches(0, header("name").is_equal_to("Camel"));
    t.template
        .send_body_and_header("direct:start", Value::Null, "name", Value::Null)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", Value::Null, "name", "Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_send_to_non_existing_endpoint() {
    let t = ContextTestSupport::new();
    let e = t
        .template
        .send_body("thisUriDoesNotExist", "<hello>world!</hello>")
        .await
        .unwrap_err();
    assert!(e.is(ErrorKind::NoSuchEndpoint), "{e:?}");
    assert!(e.to_string().contains("thisUriDoesNotExist"), "{e}");
}
