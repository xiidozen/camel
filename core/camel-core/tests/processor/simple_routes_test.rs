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

/// Ports of `ThrowExceptionTest`, `ThrowExceptionMessageTest` and
/// `ThrowExceptionNullMessageTest`.
#[tokio::test]
async fn test_throw_exception() {
    for (variant, message) in [
        (0, Some("Forced")),
        (1, Some("Darn Hello World is invalid")),
        (2, None),
    ] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let route = from("direct:start").to("mock:start");
            let route = match variant {
                0 => route.throw_exception(CamelError::illegal_argument("Forced")),
                1 => route.throw_exception_of(
                    ErrorKind::IllegalArgument,
                    Some("Darn ${body} is invalid"),
                ),
                _ => route.throw_exception_of(ErrorKind::IllegalArgument, None),
            };
            r.add(route.to("mock:result"));
        })
        .await;
        t.mock("mock:start").expected_message_count(1);
        t.mock("mock:result").expected_message_count(0);
        let e = t
            .template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap_err();
        let cause = crate::support::cause_of(&e);
        assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
        assert_eq!(message, cause.message(), "{variant}");
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `DeadLetterChannelSetHeaderTest`.
#[tokio::test]
async fn test_dead_letter_channel_set_header() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .error_handler(dead_letter_channel("direct:error"))
                .throw_exception(CamelError::illegal_argument("Damn")),
        );
        r.add(
            from("direct:error")
                .set_header("foo", constant("123"))
                .set_header("bar", constant("456"))
                .to("mock:error"),
        );
    })
    .await;
    let mock = t.mock("mock:error");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_header_received("foo", "123");
    mock.expected_header_received("bar", "456");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FilterNotMatchedTest` and `FilterNotTest`.
#[tokio::test]
async fn test_filter_not_matched_and_not() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .filter(header("foo").is_equal_to("bar"), |f| f.to("mock:result"))
                .to("mock:end"),
        );
        r.add(from("direct:not").filter(
            camel_core::support::builder::not(header("foo").is_not_null()),
            |f| f.to("mock:not"),
        ));
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    t.mock("mock:end").expected_message_count(2);
    t.template
        .send_body_and_header("direct:start", "<matched/>", "foo", "bar")
        .await
        .unwrap();
    t.template
        .send_body_and_header(
            "direct:start",
            "<notMatched/>",
            "foo",
            "notMatchedHeaderValue",
        )
        .await
        .unwrap();
    let not = t.mock("mock:not");
    not.expected_bodies_received(["<notMatched/>"]);
    t.template
        .send_body_and_header("direct:not", "<matched/>", "foo", "bar")
        .await
        .unwrap();
    t.template
        .send_body("direct:not", "<notMatched/>")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `SimpleTryThrowExceptionFinallyTest`.
#[tokio::test]
async fn test_simple_try_throw_exception_finally() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .do_try(|t| {
                    t.to("mock:try")
                        .throw_exception(CamelError::illegal_argument("Damn"))
                        .do_finally(|f| f.to("mock:finally"))
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:try").expected_message_count(1);
    t.mock("mock:finally").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    let e = t
        .template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap_err();
    assert_eq!(Some("Damn"), crate::support::cause_of(&e).message());
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `ExchangeCreatedTimestampTest`.
#[tokio::test]
async fn test_exchange_created_timestamp() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("seda:start").delay(constant(500)).to("direct:foo"));
        r.add(from("direct:foo").to("log:foo").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    t.template
        .send_body("seda:start", "Hello World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    assert!(mock.received_exchanges()[0].created() > 0);
}

/// The `SimpleSingleOutputMockTest` case (not `hasOut`: the result is the in message).
#[tokio::test]
async fn test_simple_single_output_mock() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to("mock:result"));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    let mut e = t
        .template
        .create_exchange("direct:start", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body("Hello World");
    let out = t.template.send("direct:start", e).await.unwrap();
    assert_eq!(ExchangePattern::InOut, out.pattern());
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `DisabledProcessorTest`.
#[tokio::test]
async fn test_disabled_processor() {
    let route = || {
        from("direct:start")
            .to("mock:foo")
            .disabled()
            .to("mock:bar")
            .set_disabled(false)
            .to("mock:baz")
            .set_disabled(true)
            .to("mock:result")
    };
    assert_eq!(4, route().build().steps.len());
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(route());
    })
    .await;
    t.mock("mock:foo").expected_message_count(0);
    t.mock("mock:bar").expected_message_count(1);
    t.mock("mock:baz").expected_message_count(0);
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Ports of `TryCatchContinueToRouteTest` and `OnExceptionContinueToRouteTest`: the caught
/// exception stays on the exchange in the next routes.
#[tokio::test]
async fn test_continue_to_route() {
    use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
    for on_exception in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from("direct:a").to("mock:a").to("direct:b").to("direct:c"));
            if on_exception {
                r.on_exception(
                    on_exception_kind(ErrorKind::IllegalArgument)
                        .continued(true)
                        .to("mock:catch"),
                );
                r.add(
                    from("direct:b")
                        .to("mock:b")
                        .throw_exception(CamelError::illegal_argument("Damn")),
                );
            } else {
                r.add(from("direct:b").do_try(|t| {
                    t.to("mock:b")
                        .throw_exception(CamelError::illegal_argument("Damn"))
                        .do_catch(ErrorType::Any, |c| c.to("mock:catch"))
                }));
            }
            r.add(from("direct:c").to("mock:c"));
        })
        .await;
        for m in ["mock:a", "mock:b", "mock:catch", "mock:c"] {
            t.mock(m).expected_bodies_received(["Hello World"]);
        }
        for m in ["mock:catch", "mock:c"] {
            t.mock(m)
                .message_matches(0, exchange_property(EXCEPTION_CAUGHT).is_not_null());
        }
        t.template
            .send_body("direct:a", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `FromToInOutTest`.
#[tokio::test]
async fn test_from_to_in_out() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to_with_pattern(ExchangePattern::InOut, "direct:foo")
                .to("mock:result"),
        );
        r.add(
            from("direct:foo")
                .process_fn(|e| {
                    assert_eq!(ExchangePattern::InOut, e.pattern());
                    Ok(())
                })
                .to("mock:foo"),
        );
    })
    .await;
    t.mock("mock:foo").expected_message_count(1);
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `CreateRouteWithNonExistingEndpointTest`.
#[tokio::test]
async fn test_create_route_with_non_existing_endpoint() {
    let t = ContextTestSupport::new();
    let r = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").to("thisUriDoesNotExist"));
        })
        .await;
    let e = match r {
        Err(e) => e,
        Ok(_) => t.context.start().await.unwrap_err(),
    };
    assert!(
        e.to_string()
            .contains("No endpoint could be found for: thisUriDoesNotExist"),
        "{e}"
    );
}

/// Ports of `DeadLetterChannelCatchThrowableTest` and `DefaultErrorHandlerCatchThrowableTest`
/// (a Java `Error` is any error here).
#[tokio::test]
async fn test_catch_throwable() {
    for dlc in [true, false] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            if dlc {
                r.error_handler(dead_letter_channel("mock:error"));
            }
            r.add(
                from("direct:start")
                    .process_fn(|_| Err(CamelError::runtime("This is an Error not an Exception"))),
            );
        })
        .await;
        if dlc {
            t.mock("mock:error").expected_message_count(1);
            t.template
                .send_body("direct:start", "Hello World")
                .await
                .unwrap();
            t.assert_mock_endpoints_satisfied().await;
        } else {
            let e = t
                .template
                .send_body("direct:start", "Hello World")
                .await
                .unwrap_err();
            assert_eq!(
                Some("This is an Error not an Exception"),
                crate::support::cause_of(&e).message()
            );
        }
    }
}

/// Port of `DeadLetterChannelOnPrepareTest`.
#[tokio::test]
async fn test_dead_letter_channel_on_prepare() {
    use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
    let prepare: camel_core::api::processor::SharedProcessor = std::sync::Arc::new(
        camel_core::api::processor::processor_fn(|e: &mut Exchange| {
            let cause = e
                .property(EXCEPTION_CAUGHT)
                .and_then(|v| {
                    v.downcast_ref::<CamelError>()
                        .and_then(|c| c.message().map(str::to_string))
                })
                .unwrap_or_default();
            e.message_mut().set_header("FailedBecause", cause);
            Ok(())
        }),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:dead").on_prepare_failure(prepare.clone()));
        r.add(
            from("direct:start")
                .log("Incoming ${body}")
                .throw_exception(CamelError::illegal_argument("Forced")),
        );
    })
    .await;
    let dead = t.mock("mock:dead");
    dead.expected_bodies_received(["Hello World"]);
    dead.expected_header_received("FailedBecause", "Forced");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    dead.assert_is_satisfied().await;
}

/// Ports of `RoutingSlipNoErrorHandlerTest` and `SendToNoErrorHandlerTest`: the failure of a
/// route without error handler is handled by the caller route.
#[tokio::test]
async fn test_no_error_handler_sub_route() {
    use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
    for slip in [true, false] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.error_handler(dead_letter_channel("mock:dead"));
            let route = from("direct:start");
            let route = if slip {
                route.routing_slip(constant("direct:foo"))
            } else {
                route.to("direct:foo")
            };
            r.add(route.to("mock:result"));
            r.add(
                from("direct:foo")
                    .error_handler(no_error_handler())
                    .to("mock:foo")
                    .process_fn(|_| Err(CamelError::illegal_argument("Forced"))),
            );
        })
        .await;
        t.mock("mock:foo").expected_message_count(1);
        t.mock("mock:result").expected_message_count(0);
        let dead = t.mock("mock:dead");
        dead.expected_message_count(1);
        dead.message_matches(0, exchange_property(EXCEPTION_CAUGHT).is_not_null());
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// The `DynamicRouter3Test` and `DynamicRouter4Test` slip: two endpoints, then one after
/// `mock://b`.
fn slip_after_b() -> camel_core::api::processor::SharedExpression {
    use camel_core::api::exchange_constants::SLIP_ENDPOINT;
    std::sync::Arc::new(camel_core::api::processor::expression_fn(|e: &Exchange| {
        let previous = e
            .message()
            .header(SLIP_ENDPOINT)
            .or_else(|| e.property(SLIP_ENDPOINT))
            .map(|v| v.to_string());
        Ok(match previous.as_deref() {
            None => Value::from("mock:a,mock:b"),
            Some("mock://b") => Value::from("mock:c"),
            Some(_) => Value::Null,
        })
    }))
}

/// Ports of `DynamicRouter3Test` and `DynamicRouter4Test`.
#[tokio::test]
async fn test_dynamic_router_slip_endpoint() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start").dynamic_router(ExpressionDefinition::Expression(slip_after_b())),
        );
    })
    .await;
    for m in ["mock:a", "mock:b", "mock:c"] {
        t.mock(m).expected_message_count(1);
    }
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Ports of `DynamicRouterThrowExceptionFromExpressionTest` and
/// `RoutingSlipThrowExceptionFromExpressionTest`.
#[tokio::test]
async fn test_throw_exception_from_expression() {
    for slip in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.on_exception(
                on_exception_kind(ErrorKind::ExpressionEvaluation)
                    .handled(true)
                    .to("mock://error"),
            );
            let failing: camel_core::api::processor::SharedExpression =
                std::sync::Arc::new(camel_core::api::processor::expression_fn(|_| {
                    Err(CamelError::new(ErrorKind::ExpressionEvaluation, "Forced"))
                }));
            let route = from("direct://start").to("log:foo");
            let route = if slip {
                route.routing_slip(ExpressionDefinition::Expression(failing))
            } else {
                route.dynamic_router(ExpressionDefinition::Expression(failing))
            };
            r.add(route.to("mock://result"));
        })
        .await;
        t.mock("mock:error").expected_message_count(1);
        t.mock("mock:result").expected_message_count(0);
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `FailOverLoadBalanceNoProcessorsTest`.
#[tokio::test]
async fn test_fail_over_load_balance_no_processors() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        for (uri, round_robin, sticky) in [
            ("direct:default", false, false),
            ("direct:roundRobin", true, false),
            ("direct:sticky", false, true),
            ("direct:roundRobinSticky", true, true),
        ] {
            let failover = LoadBalancerDefinition::Failover {
                exceptions: Vec::new(),
                round_robin,
                sticky,
                maximum_failover_attempts: -1,
            };
            r.add(from(uri).load_balance(failover, |b| b).to("mock:result"));
        }
    })
    .await;
    for uri in [
        "direct:default",
        "direct:roundRobin",
        "direct:sticky",
        "direct:roundRobinSticky",
    ] {
        let mock = t.mock("mock:result");
        mock.reset();
        mock.expected_bodies_received(["Hello World"]);
        let reply = t.template.request_body(uri, "Hello World").await.unwrap();
        assert_eq!("Hello World", reply.to_string(), "{uri}");
        mock.assert_is_satisfied().await;
    }
}

/// Port of `LogEipBodyWithSimpleTest`.
#[tokio::test]
async fn test_log_eip_body_with_simple() {
    #[derive(Default)]
    struct Listener(std::sync::Mutex<Option<String>>);
    impl camel_core::api::spi::log_listener::LogListener for Listener {
        fn on_log(&self, _: &Exchange, _: &str, message: &str) -> String {
            *self.0.lock().unwrap() = Some(message.to_string());
            message.to_string()
        }
    }
    let t = ContextTestSupport::new();
    let listener = std::sync::Arc::new(Listener::default());
    t.context.add_log_listener(listener.clone());
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .set_body(constant("REAL DATA"))
                    .set_property("body", constant("hello"))
                    .log("Response from API is ==> ${body}"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.template
        .send_body("direct:start", Value::Null)
        .await
        .unwrap();
    assert_eq!(
        Some("Response from API is ==> REAL DATA".to_string()),
        listener.0.lock().unwrap().clone()
    );
}

/// Port of `FilterStatusPropertyTest`.
#[tokio::test]
async fn test_filter_status_property() {
    for matching in [true, false] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .filter(header("foo").is_equal_to("bar"), |f| f.to("mock:bar"))
                    .status_property_name("myBar")
                    .to("mock:result"),
            );
        })
        .await;
        t.mock("mock:bar")
            .expected_message_count(i32::from(matching));
        if matching {
            t.mock("mock:bar")
                .message_matches(0, exchange_property("myBar").is_equal_to(true));
        }
        t.mock("mock:result").expected_message_count(1);
        t.mock("mock:result")
            .message_matches(0, exchange_property("myBar").is_equal_to(matching));
        let (body, foo) = if matching {
            ("<matched/>", "bar")
        } else {
            ("<notMatched/>", "notMatchedHeaderValue")
        };
        t.template
            .send_body_and_header("direct:start", body, "foo", foo)
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Ports of `SimpleMockTest` and `SimpleMockSampleTest`.
#[tokio::test]
async fn test_simple_mock() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("log:foo")
                .to("log:bar")
                .to("mock:result"),
        );
    })
    .await;
    assert_eq!(1, t.context.routes().len());
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World", "Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("direct:start", "Bye World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_message_count(1000);
    for _ in 0..1000 {
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
    }
    mock.assert_is_satisfied().await;
}

/// Port of `SetBodyLanguagePlaceholderTest`: the language and its expression from properties.
#[tokio::test]
async fn test_set_body_language_placeholder() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("mylan", "simple");
    t.context
        .properties()
        .add_initial_property("myexp", "Hello ${body}");
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .set_body(ExpressionDefinition::language("{{mylan}}", "{{myexp}}"))
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Ports of `SendExchangePatternOptionTest` and `RecipientListExchangePatternOptionTest`: the
/// `exchangePattern` endpoint option.
#[tokio::test]
async fn test_exchange_pattern_option() {
    for recipient_list in [false, true] {
        for in_out in [false, true] {
            let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
                let route = from("direct:start");
                let route = if recipient_list {
                    route.recipient_list(constant("stub:foo?exchangePattern=InOnly"))
                } else {
                    route.to("stub:foo?exchangePattern=InOnly")
                };
                r.add(route.to("mock:result"));
                r.add(from("stub:foo").to("mock:stub"));
            })
            .await;
            let pattern = if in_out {
                ExchangePattern::InOut
            } else {
                ExchangePattern::InOnly
            };
            let result = t.mock("mock:result");
            result.expected_message_count(1);
            result.message_matches(0, exchange_pattern().is_equal_to(pattern.to_string()));
            let stub = t.mock("mock:stub");
            stub.expected_message_count(1);
            stub.message_matches(0, exchange_pattern().is_equal_to("InOnly"));
            if in_out {
                t.template
                    .request_body("direct:start", "Hello World")
                    .await
                    .unwrap();
            } else {
                t.template
                    .send_body("direct:start", "Hello World")
                    .await
                    .unwrap();
            }
            t.assert_mock_endpoints_satisfied().await;
        }
    }
}

/// Port of `RecipientListMEPWithRawTest`.
#[tokio::test]
async fn test_recipient_list_mep_with_raw() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(constant(
                    "seda:foo?exchangePattern=InOut&blockWhenFull=RAW(true)",
                ))
                .to("mock:result"),
        );
        r.add(
            from("seda:foo")
                .to("mock:foo")
                .transform(constant("Bye World")),
        );
    })
    .await;
    t.mock("mock:foo")
        .expected_bodies_received(["Hello World", "Hello Again"]);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World", "Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("direct:start", "Hello Again")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `DeadLetterChannelNewExceptionTest`: a failing dead letter bean is handled.
#[tokio::test]
async fn test_dead_letter_channel_new_exception() {
    let t = ContextTestSupport::new();
    let bad = camel_bean::processor_value(std::sync::Arc::new(
        camel_core::api::processor::processor_fn(|_: &mut Exchange| {
            Err(CamelError::runtime("error in errorhandler"))
        }),
    ));
    t.context
        .registry()
        .unwrap()
        .bind("badErrorHandler", bad)
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.error_handler(dead_letter_channel("bean:badErrorHandler"));
            r.add(
                from("direct:start")
                    .log("Incoming ${body}")
                    .throw_exception(CamelError::illegal_argument("Forced")),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
}

/// Port of `ProcessRefHashTest`: `#` refers to a processor bean.
#[tokio::test]
async fn test_process_ref_hash() {
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
                    .process_ref("#myEcho")
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

/// Ports of `UnitOfWorkTest`, `UnitOfWorkWithAsyncFlowTest` and
/// `UnitOfWorkSynchronizationAdapterTest` (two synchronizations of one callback each): a
/// synchronization added by a processor is called on completion or failure, with the headers
/// of the exchange.
#[tokio::test]
async fn test_unit_of_work() {
    use camel_core::api::processor::BoxFuture;
    use camel_core::api::spi::synchronization::Synchronization;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Sync {
        foo: Mutex<Option<String>>,
        baz: Mutex<Option<String>>,
        done: tokio::sync::Notify,
    }
    impl Synchronization for Sync {
        fn on_complete<'a>(&'a self, e: &'a mut Exchange) -> BoxFuture<'a, ()> {
            *self.foo.lock().unwrap() = e.message().header("foo").map(|v| v.to_string());
            self.done.notify_one();
            Box::pin(std::future::ready(()))
        }
        fn on_failure<'a>(&'a self, e: &'a mut Exchange) -> BoxFuture<'a, ()> {
            *self.baz.lock().unwrap() = e.message().header("baz").map(|v| v.to_string());
            self.done.notify_one();
            Box::pin(std::future::ready(()))
        }
    }
    for (uri, adapters) in [
        ("direct:foo", false),
        ("seda:async", false),
        ("direct:foo", true),
    ] {
        for fail in [false, true] {
            let sync = Arc::new(Sync::default());
            let s = sync.clone();
            let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
                r.add(from("seda:async").to("direct:foo"));
                let s = s.clone();
                r.add(from("direct:foo").process_fn(move |e| {
                    if adapters {
                        let (c, f) = (s.clone(), s.clone());
                        e.add_on_completion(Arc::new(
                            camel_core::api::spi::synchronization::synchronization_fn(
                                move |e: &mut Exchange| {
                                    *c.foo.lock().unwrap() =
                                        e.message().header("foo").map(|v| v.to_string());
                                    c.done.notify_one();
                                },
                                |_: &mut Exchange| {},
                            ),
                        ));
                        e.add_on_completion(Arc::new(
                            camel_core::api::spi::synchronization::synchronization_fn(
                                |_: &mut Exchange| {},
                                move |e: &mut Exchange| {
                                    *f.baz.lock().unwrap() =
                                        e.message().header("baz").map(|v| v.to_string());
                                    f.done.notify_one();
                                },
                            ),
                        ));
                    } else {
                        e.add_on_completion(s.clone());
                    }
                    if fail {
                        return Err(CamelError::runtime("Failing test!"));
                    }
                    Ok(())
                }));
            })
            .await;
            let mut e = t
                .template
                .create_exchange(uri, ExchangePattern::InOnly)
                .unwrap();
            e.message_mut().set_header("foo", "bar");
            e.message_mut().set_header("baz", "bat");
            e.message_mut().set_body("<hello>world!</hello>");
            let notified = sync.done.notified();
            let _ = t.template.send(uri, e).await;
            tokio::time::timeout(std::time::Duration::from_secs(5), notified)
                .await
                .expect("Exchange did not complete.");
            let (foo, baz) = (
                sync.foo.lock().unwrap().clone(),
                sync.baz.lock().unwrap().clone(),
            );
            if fail {
                assert_eq!((None, Some("bat".to_string())), (foo, baz), "{uri}");
            } else {
                assert_eq!((Some("bar".to_string()), None), (foo, baz), "{uri}");
            }
        }
    }
}

/// Ports of `LogProcessorTest` and `LogGlobalLogNameTest` (the slf4j marker of the `wombat`
/// route has no Rust equivalent), with the logger names checked by a log listener.
#[tokio::test]
async fn test_log_processor() {
    use camel_core::api::exchange_constants::LOG_EIP_NAME;
    #[derive(Default)]
    struct Names(std::sync::Mutex<Vec<String>>);
    impl camel_core::api::spi::log_listener::LogListener for Names {
        fn on_log(&self, _: &Exchange, logger_name: &str, message: &str) -> String {
            self.0
                .lock()
                .unwrap()
                .push(format!("{logger_name}: {message}"));
            message.to_string()
        }
    }
    for global in [false, true] {
        let t = ContextTestSupport::new();
        if global {
            t.context.set_global_option(LOG_EIP_NAME, "com.foo.myapp");
        }
        let names = std::sync::Arc::new(Names::default());
        t.context.add_log_listener(names.clone());
        t.context
            .add_routes(|r: &mut Routes| {
                r.add(
                    from("direct:foo")
                        .route_id("foo")
                        .log("Got ${body}")
                        .to("mock:foo"),
                );
                r.add(
                    from("direct:bar")
                        .route_id("bar")
                        .log_level(LoggingLevel::WARN, "Also got ${body}")
                        .to("mock:bar"),
                );
                r.add(
                    from("direct:baz")
                        .route_id("baz")
                        .log_with(LoggingLevel::ERROR, "cool", "Me got ${body}")
                        .to("mock:baz"),
                );
                r.add(
                    from("direct:wombat")
                        .route_id("wombat")
                        .log_with(LoggingLevel::INFO, "cool", "Me got ${body}")
                        .to("mock:wombat"),
                );
                r.add(
                    from("direct:nolog")
                        .route_id("nolog")
                        .log_level(LoggingLevel::TRACE, "Should not log ${body}")
                        .to("mock:bar"),
                );
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        for (uri, mock, body) in [
            ("direct:foo", "mock:foo", "Hello World"),
            ("direct:bar", "mock:bar", "Bye World"),
            ("direct:baz", "mock:baz", "Hi World"),
            ("direct:wombat", "mock:wombat", "Hi Wombat"),
            ("direct:nolog", "mock:bar", "Hi World"),
        ] {
            let m = t.mock(mock);
            m.reset();
            m.expected_message_count(1);
            t.template.send_body(uri, body).await.unwrap();
            m.assert_is_satisfied().await;
        }
        let (foo, bar) = if global {
            ("com.foo.myapp", "com.foo.myapp")
        } else {
            ("foo", "bar")
        };
        let names = names.0.lock().unwrap().clone();
        assert_eq!(
            vec![
                format!("{foo}: Got Hello World"),
                format!("{bar}: Also got Bye World"),
                "cool: Me got Hi World".to_string(),
                "cool: Me got Hi Wombat".to_string(),
            ],
            names[..4.min(names.len())].to_vec(),
            "global={global}"
        );
    }
}

/// Port of `LogCustomLanguageTest`: the message in the constant language.
#[tokio::test]
async fn test_log_custom_language() {
    #[derive(Default)]
    struct Last(std::sync::Mutex<Option<String>>);
    impl camel_core::api::spi::log_listener::LogListener for Last {
        fn on_log(&self, _: &Exchange, _: &str, message: &str) -> String {
            *self.0.lock().unwrap() = Some(message.to_string());
            message.to_string()
        }
    }
    let t = ContextTestSupport::new();
    let last = std::sync::Arc::new(Last::default());
    t.context.add_log_listener(last.clone());
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .log_language("constant", "Hello ${body}")
                    .to("mock:foo"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:foo").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        Some("Hello ${body}".to_string()),
        last.0.lock().unwrap().clone()
    );
}

/// Port of `ProcessRefBeanTypeTest`: `#type:` refers to the single processor bean.
#[tokio::test]
async fn test_process_ref_bean_type() {
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
                    .process_ref("#type:org.apache.camel.Processor")
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

/// Port of `LanguageBuilderTest`.
#[tokio::test]
async fn test_language_builder() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").filter(
            ExpressionDefinition::language("simple", "${body} contains 'Camel'"),
            |f| f.to("mock:camel"),
        ));
    })
    .await;
    t.mock("mock:camel")
        .expected_bodies_received(["Hello Camel"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
