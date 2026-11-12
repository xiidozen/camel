//! Ports of `RecipientListInvalidEndpointErrorHandlerTest`,
//! `RecipientListInvalidEndpointIgnoreInvalidEndpointsTest`,
//! `RecipientListAggregationStrategyTest`, `RecipientListFunctionalTest`,
//! `RecipientListStopOnExceptionWithOnExceptionTest`, `RecipientListDoNotStopOnExceptionTest`,
//! `RecipientListWithArrayHeaderTest`, `RecipientListNoErrorHandlerTest`,
//! `RecipientListThrowExceptionFromExpressionTest`,
//! `RecipientListWithStringDelimitedPropertyTest`, `RecipientListWithSimpleExpressionTest` and
//! `RecipientListExchangePropertyTest`.

use crate::support::{body_in_aggregating_strategy, cause_of, ContextTestSupport};
use camel_core::api::error::ErrorType;
use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
use camel_core::api::processor::{expression_fn, processor_fn, SharedExpression};
use camel_core::prelude::*;
use camel_core::util::value::ObjectRef;
use std::sync::Arc;

async fn send_with_property(
    t: &ContextTestSupport,
    uri: &str,
    body: &str,
    key: &str,
    value: Value,
) {
    let mut e = t
        .template
        .create_exchange(uri, ExchangePattern::InOnly)
        .unwrap();
    e.message_mut().set_body(body);
    e.set_property(key, value);
    let e = t.template.send(uri, e).await.unwrap();
    assert!(e.exception().is_none(), "{:?}", e.exception());
}

#[tokio::test]
async fn test_invalid_endpoint() {
    for ignore in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.on_exception(on_exception(ErrorType::Any).handled(true).to("mock:dead"));
            let route = from("direct:start").recipient_list(constant("fail:endpoint"));
            let route = if ignore {
                route.ignore_invalid_endpoints()
            } else {
                route
            };
            r.add(route.to("mock:result"));
        })
        .await;
        t.mock("mock:result")
            .expected_message_count(if ignore { 1 } else { 0 });
        t.mock("mock:dead")
            .expected_message_count(if ignore { 0 } else { 1 });
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_aggregation_strategy() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .aggregation_strategy(body_in_aggregating_strategy())
                .to("mock:result"),
        );
        r.add(from("direct:a").transform(constant("a")));
        r.add(from("direct:b").transform(constant("b")));
        r.add(from("direct:c").transform(constant("c")));
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["a+b+c"]);
    t.template
        .send_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_functional_with_on_prepare() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        let endpoints: SharedExpression = Arc::new(expression_fn(|e: &Exchange| {
            let text = e
                .message()
                .header("Endpoints")
                .map(|v| v.to_string())
                .unwrap_or_default();
            Ok(Value::List(text.split(',').map(Value::from).collect()))
        }));
        r.add(
            from("direct:a")
                .recipient_list(endpoints)
                .on_prepare(Arc::new(processor_fn(|e: &mut Exchange| {
                    e.message_mut().set_header("OnPrepare", true);
                    Ok(())
                }))),
        );
    })
    .await;
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["answer"]);
        t.mock(m).expected_header_received("OnPrepare", true);
    }
    t.template
        .send_body_and_header("direct:a", "answer", "Endpoints", "mock:x,mock:y,mock:z")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

fn abc_routes(r: &mut Routes, message: &'static str) {
    r.add(from("direct:a").to("mock:a"));
    r.add(
        from("direct:b")
            .to("mock:b")
            .throw_exception(CamelError::illegal_argument(message)),
    );
    r.add(from("direct:c").to("mock:c"));
}

#[tokio::test]
async fn test_stop_on_exception_with_on_exception() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorType::Any)
                .handled(true)
                .to("mock:handled")
                .transform(simple("Damn ${exception.message}")),
        );
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .stop_on_exception()
                .to("mock:result"),
        );
        abc_routes(r, "Forced");
    })
    .await;
    t.mock("mock:result").expected_message_count(0);
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:b").expected_message_count(1);
    t.mock("mock:c").expected_message_count(0);
    let out = t
        .template
        .request_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    assert_eq!(Value::from("Damn Forced"), out);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_do_not_stop_on_exception() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .to("mock:result"),
        );
        abc_routes(r, "Damn");
    })
    .await;
    t.mock("mock:result").expected_message_count(0);
    for m in ["mock:a", "mock:b", "mock:c"] {
        t.mock(m).expected_message_count(1);
    }
    let e = t
        .template
        .send_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
    assert_eq!(Some("Damn"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_array_header_and_delimited_property() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").recipient_list(header("recipientListHeader")));
        r.add(from("direct:b").recipient_list(exchange_property("myProperty")));
    })
    .await;
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["answer"]);
    }
    let uris = Value::List(vec!["mock:x".into(), "mock:y".into(), "mock:z".into()]);
    t.template
        .send_body_and_header("direct:a", "answer", "recipientListHeader", uris)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let property = "mock:x, mock:y, mock:z";
    for m in ["mock:x", "mock:y", "mock:z"] {
        let mock = t.mock(m);
        mock.reset();
        mock.expected_bodies_received(["answer"]);
        mock.message_matches(0, exchange_property("myProperty").is_equal_to(property));
    }
    send_with_property(&t, "direct:b", "answer", "myProperty", property.into()).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_no_error_handler_in_sub_route() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:dead"));
        r.add(
            from("direct:start")
                .recipient_list(constant("direct:foo"))
                .to("mock:result"),
        );
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
    dead.message_matches(
        0,
        camel_core::support::builder::named_predicate("caught IllegalArgumentException", |e| {
            Ok(matches!(e.property(EXCEPTION_CAUGHT), Some(v) if v
                .downcast_ref::<CamelError>()
                .is_some_and(|c| c.is(ErrorKind::IllegalArgument))))
        }),
    );
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_throw_exception_from_expression() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::ExpressionEvaluation)
                .handled(true)
                .to("mock://error"),
        );
        let send_to: SharedExpression = Arc::new(expression_fn(|_: &Exchange| {
            Err(CamelError::expression_evaluation(
                CamelError::illegal_state("sendTo"),
            ))
        }));
        r.add(
            from("direct://start")
                .to("log:foo")
                .recipient_list(send_to)
                .to("mock://result"),
        );
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

#[tokio::test]
async fn test_simple_expression_concurrently() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").recipient_list(simple("mock:${in.header.queue}")));
    })
    .await;
    for i in 0..10 {
        t.mock(&format!("mock:{i}")).expected_message_count(50);
    }
    let template = Arc::new(t.context.create_producer_template());
    let mut tasks = Vec::new();
    for _ in 0..50 {
        let template = template.clone();
        tasks.push(tokio::spawn(async move {
            for i in 0..10 {
                let _ = template
                    .send_body_and_header("direct:start", format!("Hello {i}"), "queue", i)
                    .await;
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_exchange_property_is_the_same_instance() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .recipient_list(constant("mock:x,mock:y,mock:z"))
                .to("mock:result"),
        );
    })
    .await;
    let stuff = Arc::new("Blah".to_string());
    let value = Value::Object(ObjectRef::new(stuff.clone()));
    for m in ["mock:x", "mock:y", "mock:z", "mock:result"] {
        t.mock(m).expected_bodies_received(["Hello World"]);
        t.mock(m)
            .message_matches(0, exchange_property("foo").is_equal_to(value.clone()));
    }
    send_with_property(&t, "direct:a", "Hello World", "foo", value).await;
    t.assert_mock_endpoints_satisfied().await;
    let received = t.mock("mock:result").received_exchanges();
    let same = received[0]
        .property("foo")
        .and_then(|v| v.downcast_ref::<Arc<String>>())
        .is_some_and(|s| Arc::ptr_eq(s, &stuff));
    assert!(same, "Should be same instance");
}

/// Port of `RecipientListMEPTest`.
#[tokio::test]
async fn test_recipient_list_mep() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(constant("seda:foo?exchangePattern=InOut"))
                .to("mock:result"),
        );
        r.add(
            from("seda:foo")
                .to("mock:foo")
                .transform(constant("Bye World")),
        );
    })
    .await;
    for request in [false, true] {
        let foo = t.mock("mock:foo");
        let result = t.mock("mock:result");
        foo.reset();
        result.reset();
        foo.expected_bodies_received(["Hello World", "Hello Again"]);
        result.expected_bodies_received(["Bye World", "Bye World"]);
        for body in ["Hello World", "Hello Again"] {
            if request {
                let out = t.template.request_body("direct:start", body).await.unwrap();
                assert_eq!(Value::from("Bye World"), out);
            } else {
                t.template.send_body("direct:start", body).await.unwrap();
            }
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `RecipientListWithDelimiterTest`.
#[tokio::test]
async fn test_recipient_list_with_delimiter() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .recipient_list(header("myHeader"))
                .uri_delimiter("#"),
        );
        r.add(
            from("direct:b")
                .recipient_list(header("myHeader"))
                .uri_delimiter("false"),
        );
        r.add(from("direct:c").recipient_list(header("myHeader").tokenize("#")));
    })
    .await;
    for uri in ["direct:a", "direct:c"] {
        for m in ["mock:x", "mock:y", "mock:z"] {
            let mock = t.mock(m);
            mock.reset();
            mock.expected_bodies_received(["answer"]);
        }
        t.template
            .send_body_and_header(uri, "answer", "myHeader", "mock:x#mock:y#mock:z")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
    let m = t.mock("mock:falseDelimiterTest");
    m.expected_bodies_received(["answer"]);
    t.template
        .send_body_and_header("direct:b", "answer", "myHeader", "mock:falseDelimiterTest")
        .await
        .unwrap();
    m.assert_is_satisfied().await;
}

/// Port of `RecipientListOnPrepareTest` (the Java `Animal` is a map, and each copy has its
/// own body in Rust, which the Java deep clone `onPrepare` gives).
#[tokio::test]
async fn test_recipient_list_on_prepare() {
    fn animal(name: &str) -> Value {
        let mut m = ValueMap::new();
        m.insert("id".into(), Value::from(1));
        m.insert("name".into(), Value::from(name));
        Value::Map(m)
    }
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .on_prepare(Arc::new(processor_fn(|e: &mut Exchange| {
                    let copy = e.message().body().clone();
                    e.message_mut().set_body(copy);
                    Ok(())
                }))),
        );
        r.add(
            from("direct:a")
                .process_fn(|e| {
                    assert_eq!(&animal("Tiger"), e.message().body());
                    e.message_mut().set_body(animal("Tony the Tiger"));
                    Ok(())
                })
                .to("mock:a"),
        );
        r.add(
            from("direct:b")
                .process_fn(|e| {
                    assert_eq!(&animal("Tiger"), e.message().body());
                    Ok(())
                })
                .to("mock:b"),
        );
    })
    .await;
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:a")
        .message_matches(0, body().is_equal_to(animal("Tony the Tiger")));
    t.mock("mock:b").expected_message_count(1);
    t.mock("mock:b")
        .message_matches(0, body().is_equal_to(animal("Tiger")));
    t.template
        .send_body_and_header("direct:start", animal("Tiger"), "foo", "direct:a,direct:b")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `RecipientListParallelStopOnExceptionWithOnExceptionTest`.
#[tokio::test]
async fn test_parallel_stop_on_exception_with_on_exception() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorType::Any)
                .handled(true)
                .to("mock:handled")
                .transform(simple("Damn ${exception.message}")),
        );
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .stop_on_exception()
                .parallel_processing()
                .to("mock:result"),
        );
        abc_routes(r, "Forced");
    })
    .await;
    t.mock("mock:result").expected_message_count(0);
    t.mock("mock:b").expected_message_count(1);
    let out = t
        .template
        .request_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap();
    assert_eq!(Value::from("Damn Forced"), out);
    t.assert_mock_endpoints_satisfied().await;
}
