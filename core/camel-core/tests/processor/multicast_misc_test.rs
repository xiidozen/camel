//! Ports of `MulticastWithOnExceptionTest` (with its `First` and `Last` variants),
//! `MulticastStopOnExceptionWithOnExceptionTest` (with its `Last` variant),
//! `MulticastStopOnExceptionTest`, `MulticastNoStopOnExceptionTest`,
//! `MulticastCompletedPropertyTest`, `MulticastKeepOriginalMessageUnchangedTest` and
//! `MulticastDslTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::api::error::ErrorType;
use camel_core::api::exchange_constants::MULTICAST_COMPLETE;
use camel_core::api::processor::{aggregation_fn, processor_fn};
use camel_core::prelude::*;
use camel_core::processor::aggregate::UseOriginalAggregationStrategy;
use std::sync::Arc;

fn my_processor(e: &mut Exchange) -> CamelResult<()> {
    if matches!(e.message().body(), Value::Text(t) if t == "Kaboom") {
        return Err(CamelError::illegal_argument("Forced"));
    }
    Ok(())
}

/// The routes: a multicast to the uris (in order), with or without stopOnException and the
/// onException handling the failure.
async fn context(
    uris: [&'static str; 3],
    stop: bool,
    on_exception_handled: bool,
) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        if on_exception_handled {
            r.on_exception(
                on_exception(ErrorType::Any)
                    .handled(true)
                    .to("mock:handled")
                    .transform(simple("Damn ${exception.message}")),
            );
        }
        let route = from("direct:start").multicast(|m| m.to(uris[0]).to(uris[1]).to(uris[2]));
        let route = if stop {
            route.stop_on_exception()
        } else {
            route
        };
        r.add(route.to("mock:result"));
        r.add(from("direct:foo").to("mock:foo"));
        r.add(from("direct:bar").process_fn(my_processor).to("mock:bar"));
        r.add(from("direct:baz").to("mock:baz"));
    })
    .await
}

const ORDERS: [[&str; 3]; 3] = [
    ["direct:foo", "direct:bar", "direct:baz"],
    ["direct:bar", "direct:foo", "direct:baz"],
    ["direct:foo", "direct:baz", "direct:bar"],
];

async fn assert_ok(t: &ContextTestSupport, handled: bool) {
    for m in ["mock:foo", "mock:bar", "mock:baz", "mock:result"] {
        t.mock(m).expected_bodies_received(["Hello"]);
    }
    if handled {
        t.mock("mock:handled").expected_message_count(0);
    }
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_with_on_exception() {
    for uris in ORDERS {
        let t = context(uris, false, true).await;
        assert_ok(&t, true).await;
        let t = context(uris, false, true).await;
        t.mock("mock:foo").expected_bodies_received(["Kaboom"]);
        t.mock("mock:baz").expected_bodies_received(["Kaboom"]);
        t.mock("mock:bar").expected_message_count(0);
        t.mock("mock:handled").expected_message_count(1);
        t.mock("mock:result").expected_message_count(0);
        let out = t
            .template
            .request_body("direct:start", "Kaboom")
            .await
            .unwrap();
        assert_eq!(Value::from("Damn Forced"), out, "{uris:?}");
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_multicast_stop_on_exception_with_on_exception() {
    let t = context(ORDERS[0], true, true).await;
    assert_ok(&t, true).await;
    // the failing endpoint in the middle: the next one is not called
    let t = context(ORDERS[0], true, true).await;
    t.mock("mock:foo").expected_bodies_received(["Kaboom"]);
    t.mock("mock:bar").expected_message_count(0);
    t.mock("mock:handled").expected_message_count(1);
    t.mock("mock:baz").expected_message_count(0);
    t.mock("mock:result").expected_message_count(0);
    let out = t
        .template
        .request_body("direct:start", "Kaboom")
        .await
        .unwrap();
    assert_eq!(Value::from("Damn Forced"), out);
    t.assert_mock_endpoints_satisfied().await;
    // the failing endpoint last
    let t = context(ORDERS[2], true, true).await;
    t.mock("mock:foo").expected_bodies_received(["Kaboom"]);
    t.mock("mock:baz").expected_bodies_received(["Kaboom"]);
    t.mock("mock:bar").expected_message_count(0);
    t.mock("mock:handled").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    let out = t
        .template
        .request_body("direct:start", "Kaboom")
        .await
        .unwrap();
    assert_eq!(Value::from("Damn Forced"), out);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_stop_and_no_stop_on_exception() {
    let t = context(ORDERS[0], true, false).await;
    assert_ok(&t, false).await;
    let t = context(ORDERS[0], true, false).await;
    t.mock("mock:foo").expected_bodies_received(["Kaboom"]);
    t.mock("mock:bar").expected_message_count(0);
    t.mock("mock:baz").expected_message_count(0);
    t.mock("mock:result").expected_message_count(0);
    let e = t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::CamelExchange), "{e}");
    assert!(cause
        .message()
        .unwrap_or_default()
        .starts_with("Multicast processing failed for number 1."));
    assert_eq!(Some("Forced"), cause_of(cause).message());
    t.assert_mock_endpoints_satisfied().await;

    let t = context(ORDERS[0], false, false).await;
    assert_ok(&t, false).await;
    let t = context(ORDERS[0], false, false).await;
    t.mock("mock:foo").expected_bodies_received(["Kaboom"]);
    t.mock("mock:bar").expected_message_count(0);
    t.mock("mock:baz").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    let e = t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
    assert_eq!(Some("Forced"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_completed_property() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").multicast(|m| m.to("mock:a").to("mock:b").to("mock:c")));
    })
    .await;
    for (m, complete) in [("mock:a", false), ("mock:b", false), ("mock:c", true)] {
        t.mock(m).expected_message_count(1);
        t.mock(m).message_matches(
            0,
            exchange_property(MULTICAST_COMPLETE).is_equal_to(complete),
        );
    }
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_keep_original_message_unchanged() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .set_header("bar", constant("no"))
                .to("mock:a")
                .multicast(|m| m.to("direct:foo"))
                .aggregation_strategy(Arc::new(UseOriginalAggregationStrategy::new(true)))
                .to("mock:result"),
        );
        r.add(
            from("direct:foo")
                .set_header("foo", constant("yes"))
                .remove_header("bar")
                .transform(simple("Foo was here ${body}"))
                .to("mock:foo"),
        );
    })
    .await;
    for m in ["mock:a", "mock:result"] {
        t.mock(m).expected_bodies_received(["Hello World"]);
        t.mock(m).message_matches(0, header("foo").is_null());
        t.mock(m)
            .message_matches(0, header("bar").is_equal_to("no"));
    }
    let foo = t.mock("mock:foo");
    foo.expected_bodies_received(["Foo was here Hello World"]);
    foo.expected_header_received("foo", "yes");
    foo.message_matches(0, header("bar").is_null());
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_dsl() {
    fn increase(amount: i64) -> impl Fn(&mut Exchange) -> CamelResult<()> + Send + Sync + 'static {
        move |e| {
            let n = e.message().body_as::<i64>()?.unwrap_or_default();
            e.message_mut().set_body(n + amount);
            Ok(())
        }
    }
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .multicast(|m| m.to("direct:increase-by-1").to("direct:increase-by-2"))
                .on_prepare(Arc::new(processor_fn(|e: &mut Exchange| {
                    e.message_mut().set_header("onPrepare", true);
                    Ok(())
                })))
                .aggregation_strategy(Arc::new(aggregation_fn(|old, new| {
                    let Some(mut new) = new else { return Ok(old) };
                    if let Some(old) = old {
                        let sum = old.message().body_as::<i64>()?.unwrap_or_default()
                            + new.message().body_as::<i64>()?.unwrap_or_default();
                        new.message_mut().set_body(sum);
                    }
                    Ok(Some(new))
                })))
                .to("mock:result"),
        );
        r.add(from("direct:increase-by-1").process_fn(increase(1)));
        r.add(from("direct:increase-by-2").process_fn(increase(2)));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_header_received("onPrepare", true);
    mock.expected_bodies_received([5]);
    t.template.send_body("direct:start", 1).await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `MulticastFineGrainedErrorHandlingTest` (each step of the multicast is a branch).
#[tokio::test]
async fn test_fine_grained_error_handling() {
    for error in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.on_exception(
                on_exception(ErrorType::Any)
                    .redelivery_delay(0)
                    .maximum_redeliveries(2),
            );
            r.add(
                from("direct:start")
                    .to("mock:a")
                    .multicast(move |m| {
                        let m = m.to("mock:foo").to("mock:bar");
                        if error {
                            m.throw_exception(CamelError::illegal_argument("Damn"))
                                .to("mock:baz")
                        } else {
                            m.to("mock:baz")
                        }
                    })
                    .stop_on_exception(),
            );
        })
        .await;
        for m in ["mock:a", "mock:foo", "mock:bar"] {
            t.mock(m).expected_message_count(1);
        }
        t.mock("mock:baz")
            .expected_message_count(if error { 0 } else { 1 });
        let r = t.template.send_body("direct:start", "Hello World").await;
        assert_eq!(error, r.is_err());
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// The `MulticastParallelFailureEndpointTest` case without doTry (with doTry Java has no
/// failure endpoint after the catch, which the Rust catch keeps).
#[tokio::test]
async fn test_parallel_failure_endpoint() {
    use camel_core::api::exchange_constants::FAILURE_ENDPOINT;
    for uri in ["direct:run"] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .do_try(|b| {
                        b.to("direct:run")
                            .do_catch(ErrorKind::IllegalArgument, |c| c)
                    })
                    .to("mock:result"),
            );
            r.add(
                from("direct:run")
                    .multicast(|m| m.to("direct:a").to("direct:b"))
                    .parallel_processing()
                    .to("mock:run"),
            );
            r.add(
                from("direct:a")
                    .throw_exception(CamelError::illegal_argument("Oops..."))
                    .to("mock:a"),
            );
            r.add(
                from("direct:b")
                    .set_body(constant("Bye World"))
                    .to("mock:b"),
            );
        })
        .await;
        t.mock("mock:run").expected_message_count(0);
        t.mock("mock:a").expected_message_count(0);
        t.mock("mock:b").expected_message_count(1);
        if uri == "direct:start" {
            t.mock("mock:result")
                .expected_bodies_received(["Bye World"]);
        }
        let mut request = t
            .template
            .create_exchange(uri, ExchangePattern::InOut)
            .unwrap();
        request.message_mut().set_body("Hello World");
        let result = t.template.send(uri, request).await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
        if uri == "direct:run" {
            assert_eq!(
                Some(&Value::from("direct://a")),
                result.property(FAILURE_ENDPOINT)
            );
        } else {
            assert_eq!(None, result.property(FAILURE_ENDPOINT));
        }
    }
}

/// Port of `MulticastUseOriginalNotPropagateExceptionTest`.
#[tokio::test]
async fn test_use_original_not_propagate_exception() {
    fn recipient(r: &mut Routes, n: u8) {
        let name = format!("recipient{n}");
        let throw = format!("throw{n}");
        let message = name.clone();
        r.add(
            from(&format!("direct:{name}"))
                .log(&name)
                .choice(move |c| {
                    c.when(body().contains(throw.as_str()), move |w| {
                        let message = message.clone();
                        w.process_fn(move |_| Err(CamelError::runtime(message.clone())))
                    })
                })
                .set_body(constant(name.clone()))
                .to(&format!("mock:{name}")),
        );
    }
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .error_handler(no_error_handler())
                .multicast(|m| m.to("direct:recipient1").to("direct:recipient2"))
                .aggregation_strategy(Arc::new(UseOriginalAggregationStrategy::new(false)))
                .to("mock:result"),
        );
        recipient(r, 1);
        recipient(r, 2);
    })
    .await;
    for body in ["body", "throw1", "throw2", "throw1;throw2"] {
        let (r1, r2, result) = (
            t.mock("mock:recipient1"),
            t.mock("mock:recipient2"),
            t.mock("mock:result"),
        );
        r1.reset();
        r2.reset();
        result.reset();
        r1.expected_message_count(if body.contains("throw1") { 0 } else { 1 });
        r2.expected_message_count(if body.contains("throw2") { 0 } else { 1 });
        result.expected_bodies_received([body]);
        t.template.send_body("direct:start", body).await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `MulticastGroupedExchangeExceptionTest`.
#[tokio::test]
async fn test_grouped_exchange_exception() {
    use camel_core::processor::aggregate::GroupedExchangeAggregationStrategy;
    for failing in [None, Some("mock:endpointB"), Some("mock:endpointA")] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .multicast(|m| m.to("mock:endpointA").to("mock:endpointB"))
                    .aggregation_strategy(Arc::new(GroupedExchangeAggregationStrategy))
                    .to("mock:result"),
            );
        })
        .await;
        let result = t.mock("mock:result");
        result.expected_message_count(1);
        if let Some(uri) = failing {
            t.mock(uri)
                .when_any_exchange_received(Arc::new(processor_fn(|_| {
                    Err(CamelError::illegal_argument("Fake exception"))
                })));
        }
        t.template.send_body("direct:start", "dummy").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
        assert!(!result.received_exchanges()[0].is_failed(), "no exception");
    }
}
