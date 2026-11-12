//! Ports of small route tests: `StaticRecipientListTest`, `VerySimpleDirectTest`,
//! `SimpleDirectTest`, `SimpleTryFinallyTest`, `RegExRouteTest`,
//! `CBRSimplePredicateEmptyBodyTest` and `SendToNonExistingEndpointTest`.

use crate::support::ContextTestSupport;
use camel_core::api::error::ErrorType;
use camel_core::model::expression::ExpressionDefinition;
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

/// The destination of `DynamicRouterOnExceptionTest.whereTo`: `mock:route` once, then the end.
fn where_to() -> camel_core::api::processor::SharedExpression {
    std::sync::Arc::new(camel_core::api::processor::expression_fn(|e: &Exchange| {
        Ok(if e.property("invoked").is_none() {
            Value::from("mock:route")
        } else {
            Value::Null
        })
    }))
}

/// Ports of `DynamicRouterOnExceptionTest` and `RoutingSlipOnExceptionTest`: a failed
/// destination is redelivered.
#[tokio::test]
async fn test_dynamic_router_and_routing_slip_on_exception() {
    for (slip, case) in [
        (false, 0),
        (false, 1),
        (false, 2),
        (true, 0),
        (true, 1),
        (true, 2),
    ] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.on_exception(
                on_exception_kind(ErrorKind::IllegalArgument)
                    .redelivery_delay(0)
                    .maximum_redeliveries(5),
            );
            let route = from("direct:start");
            let route = if slip {
                route.routing_slip(ExpressionDefinition::Expression(where_to()))
            } else {
                route.dynamic_router(ExpressionDefinition::Expression(where_to()))
            };
            r.add(route.to("mock:end"));
        })
        .await;
        let route = t.mock("mock:route");
        // the destination marks the exchange as invoked
        let invoked = |body: Option<&'static str>,
                       fail: bool|
         -> camel_core::api::processor::SharedProcessor {
            std::sync::Arc::new(camel_core::api::processor::processor_fn(
                move |e: &mut Exchange| {
                    e.set_property("invoked", true);
                    if let Some(b) = body {
                        e.message_mut().set_body(b);
                    }
                    if fail {
                        return Err(CamelError::illegal_argument("Forced"));
                    }
                    Ok(())
                },
            ))
        };
        route.when_any_exchange_received(invoked(None, false));
        match case {
            0 => {
                t.mock("mock:end").expected_message_count(1);
                route.expected_message_count(1);
                t.template
                    .send_body("direct:start", "Hello World")
                    .await
                    .unwrap();
            }
            1 => {
                t.mock("mock:end").expected_message_count(1);
                route.when_exchange_received(1, invoked(None, true));
                route.when_exchange_received(2, invoked(Some("Bye World"), false));
                route.expected_message_count(2);
                t.template
                    .send_body("direct:start", "Hello World")
                    .await
                    .unwrap();
            }
            _ => {
                t.mock("mock:end").expected_message_count(2);
                route.when_exchange_received(1, invoked(None, true));
                route.when_exchange_received(2, invoked(Some("Bye World"), false));
                route.when_exchange_received(3, invoked(None, true));
                route.when_exchange_received(4, invoked(Some("Bye World"), false));
                route.expected_message_count(4);
                t.template
                    .send_body("direct:start", "Hello World")
                    .await
                    .unwrap();
                t.template
                    .send_body("direct:start", "Bye World")
                    .await
                    .unwrap();
            }
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// The `testOk` and `testWithError` cases of `DefaultErrorHandlerTest` and
/// `DefaultErrorHandlerExplicitConfiguredTest`.
#[tokio::test]
async fn test_default_error_handler() {
    for explicit in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            if explicit {
                r.error_handler(default_error_handler());
            }
            r.add(
                from("direct:start")
                    .process_fn(|e| {
                        if matches!(e.message().body(), Value::Text(b) if b == "Kaboom") {
                            return Err(CamelError::illegal_argument("Boom"));
                        }
                        e.message_mut().set_body("Bye World");
                        Ok(())
                    })
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Bye World"]);
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        assert!(t
            .template
            .send_body("direct:start", "Kaboom")
            .await
            .is_err());
        mock.assert_is_satisfied().await;
    }
}

/// Port of `TryCatchMustHaveExceptionConfiguredTest`.
#[tokio::test]
async fn test_try_catch_must_have_exception_configured() {
    let t = ContextTestSupport::new();
    let r = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:a").do_try(|t| {
                t.to("mock:b")
                    .throw_exception(CamelError::illegal_argument("Damn"))
                    .do_catch(Vec::<ErrorType>::new(), |c| c.to("mock:catch"))
            }));
        })
        .await;
    let e = match r {
        Err(e) => e,
        Ok(_) => t.context.start().await.unwrap_err(),
    };
    assert!(
        e.to_string()
            .contains("At least one Exception must be configured to catch"),
        "{e}"
    );
}

/// Port of `FailoverRoundRobinGoodBadTest`.
#[tokio::test]
async fn test_failover_round_robin_good_bad() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        let failover = LoadBalancerDefinition::Failover {
            exceptions: Vec::new(),
            round_robin: true,
            sticky: false,
            maximum_failover_attempts: 1,
        };
        r.add(
            from("direct:start").load_balance(failover, |b| b.to("direct:good").to("direct:bad")),
        );
        r.add(from("direct:good").to("mock:good"));
        r.add(
            from("direct:bad")
                .to("mock:bad")
                .throw_exception(CamelError::illegal_argument("Damn")),
        );
    })
    .await;
    t.mock("mock:good")
        .expected_bodies_received(["Hello World", "Bye World"]);
    t.mock("mock:bad").expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("direct:start", "Bye World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `SimpleProcessorTest`.
#[tokio::test]
async fn test_simple_processor() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").process_fn(|e| {
            e.message_mut().set_body("Bye World");
            Ok(())
        }));
    })
    .await;
    let out = t
        .template
        .request_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!("Bye World", out.to_string());
}

/// Port of `RouteWithConstantFieldFromExchangeTest`: an `Exchange.XXX` header name is the
/// constant.
#[tokio::test]
async fn test_route_with_constant_field_from_exchange() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header("Exchange.FILE_NAME", constant("foo.txt"))
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_header_received(camel_core::api::exchange_constants::FILE_NAME, "foo.txt");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `ProcessRefBeanIdTest`: `#bean:` refers to a processor bean.
#[tokio::test]
async fn test_process_ref_bean_id() {
    let t = ContextTestSupport::new();
    let echo = camel_bean::processor_value(std::sync::Arc::new(
        camel_core::api::processor::processor_fn(|e: &mut Exchange| {
            let body = e.message().body().to_string();
            e.message_mut().set_body(format!("{body}{body}"));
            Ok(())
        }),
    ));
    t.context.registry().unwrap().bind("myEcho", echo).unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .process_ref("#bean:myEcho")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["CamelCamel"]);
    t.template.send_body("direct:start", "Camel").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `TryProcessorOnWhenTest`.
#[tokio::test]
async fn test_try_processor_on_when() {
    for (body, result, catch, catch_camel, fails) in [
        ("Damn IO", 0, 1, 0, false),
        ("Damn State", 0, 1, 0, false),
        ("Camel", 0, 0, 1, false),
        ("Other Bug", 0, 0, 0, true),
        ("Hello World", 1, 0, 0, false),
    ] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(from("direct:start").do_try(|t| {
                t.process_fn(|e| match e.message().body().to_string().as_str() {
                    "Damn IO" => Err(CamelError::new(ErrorKind::Io, "Damn IO")),
                    "Damn State" => Err(CamelError::illegal_state("Damn State")),
                    "Other Bug" => Err(CamelError::illegal_state("Other Bug")),
                    "Camel" => Err(CamelError::camel_exchange("Sorry old Camel", e)),
                    _ => Ok(()),
                })
                .to("mock:result")
                .do_catch_when(
                    [
                        ErrorType::Kind(ErrorKind::Io),
                        ErrorType::Kind(ErrorKind::IllegalState),
                    ],
                    simple("${exception.message} contains 'Damn'"),
                    |c| c.to("mock:catch"),
                )
                .do_catch(ErrorKind::CamelExchange, |c| c.to("mock:catchCamel"))
                .do_finally(|f| f.to("mock:finally"))
            }));
        })
        .await;
        t.mock("mock:result").expected_message_count(result);
        t.mock("mock:catch").expected_message_count(catch);
        t.mock("mock:catchCamel")
            .expected_message_count(catch_camel);
        t.mock("mock:finally").expected_message_count(1);
        let r = t.template.send_body("direct:start", body).await;
        assert_eq!(fails, r.is_err(), "{body}");
        if let Err(e) = r {
            let cause = crate::support::cause_of(&e);
            assert!(cause.is(ErrorKind::IllegalState), "{e}");
            assert_eq!(Some("Other Bug"), cause.message());
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}
