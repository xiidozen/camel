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

/// Ports of `DeadLetterChannelFailureRouteIdTest` and `OnExceptionFailureRouteIdTest`: the
/// failure route id is the route that failed.
#[tokio::test]
async fn test_failure_route_id() {
    use camel_core::api::exchange_constants::FAILURE_ROUTE_ID;
    for on_exception in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            if on_exception {
                r.on_exception(
                    on_exception_kind(ErrorKind::IllegalArgument)
                        .handled(true)
                        .to("direct:dead"),
                );
            } else {
                r.error_handler(dead_letter_channel("direct:dead"));
            }
            r.add(
                from("direct:foo")
                    .route_id("foo")
                    .to("mock:foo")
                    .to("direct:bar")
                    .to("mock:result"),
            );
            r.add(
                from("direct:bar")
                    .route_id("bar")
                    .to("mock:bar")
                    .throw_exception(CamelError::illegal_argument("Forced")),
            );
            r.add(
                from("direct:dead")
                    .log("Failed at route ${exchangeProperty.CamelFailureRouteId}")
                    .to("mock:dead"),
            );
        })
        .await;
        t.mock("mock:foo").expected_message_count(1);
        t.mock("mock:bar").expected_message_count(1);
        t.mock("mock:result").expected_message_count(0);
        let dead = t.mock("mock:dead");
        dead.expected_message_count(1);
        dead.expected_property_received(FAILURE_ROUTE_ID, "bar");
        t.template
            .send_body("direct:foo", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

fn on_exception_kind(kind: ErrorKind) -> OnExceptionDsl {
    on_exception(camel_core::api::error::ErrorType::Kind(kind))
}

/// Ports of `DeadLetterChannelExceptionCausePropagatedTest` and
/// `DeadLetterChannelExceptionCausePropagatedWithDefaultErrorHandlerTest`.
#[tokio::test]
async fn test_exception_cause_propagated() {
    use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
    for dlc in [true, false] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            if dlc {
                r.error_handler(
                    dead_letter_channel("mock:error")
                        .redelivery_delay(0)
                        .maximum_redeliveries(3),
                );
            }
            r.on_exception(
                on_exception_kind(ErrorKind::RuntimeCamel)
                    .handled(true)
                    .to("mock:failed"),
            );
            r.add(
                from("direct:start")
                    .process_fn(|_| Err(CamelError::runtime("Expected exception.")))
                    .to("mock:success"),
            );
        })
        .await;
        let failed = t.mock("mock:failed");
        failed.expected_bodies_received(["<hello>world!</hello>"]);
        failed.message_matches(0, exchange_property(EXCEPTION_CAUGHT).is_not_null());
        t.mock("mock:success").expected_message_count(0);
        t.template
            .send_body("direct:start", "<hello>world!</hello>")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
        let received = &failed.received_exchanges()[0];
        assert!(received.exception().is_none());
        let caught = received
            .property(EXCEPTION_CAUGHT)
            .map(|v| v.to_string())
            .unwrap_or_default();
        assert!(caught.contains("Expected exception."), "{caught}");
    }
}

/// Port of `MulticastCorrelationIdTest`.
#[tokio::test]
async fn test_multicast_correlation_id() {
    use camel_core::api::exchange_constants::CORRELATION_ID;
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").multicast(|s| s.to("direct:a").to("direct:b")));
        r.add(from("direct:a").log("Route a"));
        r.add(from("direct:b").log("Route b"));
    })
    .await;
    let mut exchange = t
        .template
        .create_exchange("direct:start", ExchangePattern::InOut)
        .unwrap();
    exchange.set_property(CORRELATION_ID, "SOME_ID");
    let exchange = t.template.send("direct:start", exchange).await.unwrap();
    assert_eq!(
        Some("SOME_ID".to_string()),
        exchange.property(CORRELATION_ID).map(|v| v.to_string())
    );
}

/// Port of `CorrectRouteIdTest`.
#[tokio::test]
async fn test_correct_route_id() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:foo")
                .route_id("foo")
                .set_header("foo", simple("${routeId}"))
                .to("mock:foo")
                .to("seda:bar")
                .to("mock:result"),
        );
        r.add(
            from("seda:bar")
                .route_id("bar")
                .set_header("bar", simple("${routeId}"))
                .to("mock:bar")
                .to("direct:baz"),
        );
        r.add(
            from("direct:baz")
                .route_id("baz")
                .set_header("baz", simple("${routeId}"))
                .to("mock:baz"),
        );
    })
    .await;
    t.mock("mock:foo").expected_header_received("foo", "foo");
    t.mock("mock:bar").expected_header_received("bar", "bar");
    t.mock("mock:baz").expected_header_received("baz", "baz");
    t.template
        .request_body("direct:foo", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
