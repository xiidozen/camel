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
