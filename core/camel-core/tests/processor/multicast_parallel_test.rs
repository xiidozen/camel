//! Ports of `MulticastParallelTest`, `MulticastParallelStreamingTest`,
//! `MulticastParallelStopOnExceptionTest`, `MulticastParallelNoStopOnExceptionTest`,
//! `MulticastParallelTimeoutTest`, `MulticastParallelTimeout2Test`,
//! `MulticastParallelTimeout3Test`, `MulticastParallelMiddleTimeoutTest`,
//! `MulticastParallelLastTimeoutTest`, `MulticastParallelTwoTimeoutTest`,
//! `MulticastParallelTwoTimeoutMiddleTest`, `MulticastParallelStreamingTimeoutTest`,
//! `MulticastParallelTimeoutAwareTest` and `MulticastParallelAllTimeoutAwareTest`.
//!
//! The Java tests synchronizing the start of the sub tasks with a `Phaser` do not need it:
//! the tokio tasks are all spawned before the timeout starts. The custom executor services of
//! the Java tests are not applicable (tokio tasks).

use crate::support::{cause_of, concat_strategy, ContextTestSupport};
use camel_core::api::processor::AggregationStrategy;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

/// `direct:<name>` routes: an optional delay, `mock:<NAME>` when `with_mock`, then the body
/// is set to `<NAME>`.
fn targets(r: &mut Routes, delays: &[(&str, u64)], with_mock: bool) {
    for (name, delay) in delays {
        let upper = name.to_uppercase();
        let mut route = from(&format!("direct:{name}"));
        if *delay > 0 {
            route = route.delay(constant(*delay as i64));
        }
        if with_mock {
            route = route.to(&format!("mock:{upper}"));
        }
        r.add(route.set_body(constant(upper.as_str())));
    }
}

/// `direct:start` multicasting in parallel to the `direct:<name>` targets, then
/// `mock:result`.
fn start(
    r: &mut Routes,
    names: &[&str],
    strategy: Arc<dyn AggregationStrategy>,
    timeout: Option<u64>,
    streaming: bool,
) {
    let names: Vec<String> = names.iter().map(|n| format!("direct:{n}")).collect();
    let mut route = from("direct:start")
        .multicast(|mut m| {
            for n in &names {
                m = m.to(n);
            }
            m
        })
        .aggregation_strategy(strategy)
        .parallel_processing();
    if let Some(t) = timeout {
        route = route.timeout(t);
    }
    if streaming {
        route = route.streaming();
    }
    r.add(route.to("mock:result"));
}

#[tokio::test]
async fn test_single_multicast_parallel() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        start(r, &["a", "b"], concat_strategy(), None, false);
        targets(r, &[("a", 100), ("b", 0)], false);
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["AB"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        start(r, &["a", "b"], concat_strategy(), None, false);
        targets(r, &[("a", 100), ("b", 0)], false);
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(20);
    mock.all_messages_match(body().is_equal_to("AB"));
    for _ in 0..20 {
        t.template.send_body("direct:start", "Hello").await.unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_streaming() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        start(r, &["a", "b"], concat_strategy(), None, true);
        targets(r, &[("a", 500), ("b", 0)], false);
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    mock.all_messages_match(body().is_equal_to("BA"));
    for _ in 0..3 {
        t.template.send_body("direct:start", "Hello").await.unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

/// `direct:foo` and `direct:baz` are delayed, `direct:bar` fails on `Kaboom`.
fn foo_bar_baz(r: &mut Routes, stop_on_exception: bool) {
    let mut route = from("direct:start")
        .multicast(|m| m.to("direct:foo").to("direct:bar").to("direct:baz"))
        .parallel_processing();
    if stop_on_exception {
        route = route.stop_on_exception();
    }
    r.add(route.to("mock:result"));
    r.add(from("direct:foo").delay(constant(1000)).to("mock:foo"));
    r.add(
        from("direct:bar")
            .process_fn(|e| {
                if e.in_message().body() == "Kaboom" {
                    return Err(CamelError::illegal_argument("Forced"));
                }
                Ok(())
            })
            .to("mock:bar"),
    );
    r.add(from("direct:baz").delay(constant(1000)).to("mock:baz"));
}

async fn foo_bar_baz_ok(stop_on_exception: bool) {
    let t =
        ContextTestSupport::with_routes(move |r: &mut Routes| foo_bar_baz(r, stop_on_exception))
            .await;
    for uri in ["mock:foo", "mock:bar", "mock:baz", "mock:result"] {
        t.mock(uri).expected_bodies_received(["Hello"]);
    }
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_stop_on_exception_ok() {
    foo_bar_baz_ok(true).await;
}

#[tokio::test]
async fn test_multicast_parallel_stop_on_exception_stop() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| foo_bar_baz(r, true)).await;
    for uri in ["mock:foo", "mock:bar", "mock:baz"] {
        t.mock(uri).expected_minimum_message_count(0);
    }
    t.mock("mock:result").expected_message_count(0);
    let err = t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .unwrap_err();
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::CamelExchange), "{cause:?}");
    assert!(cause
        .message()
        .unwrap()
        .starts_with("Multicast processing failed for number "));
    assert_eq!(Some("Forced"), cause_of(cause).message());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_no_stop_on_exception_ok() {
    foo_bar_baz_ok(false).await;
}

#[tokio::test]
async fn test_multicast_parallel_no_stop_on_exception_stop() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| foo_bar_baz(r, false)).await;
    t.mock("mock:foo").expected_bodies_received(["Kaboom"]);
    t.mock("mock:bar").expected_message_count(0);
    t.mock("mock:baz").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    let err = t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .unwrap_err();
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::IllegalArgument), "{cause:?}");
    assert_eq!(Some("Forced"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}

/// Multicasts to the targets with the delays and the timeout, and expects the result body.
async fn timeout_test(delays: &[(&str, u64)], timeout: u64, streaming: bool) -> ContextTestSupport {
    let delays: Vec<(String, u64)> = delays.iter().map(|(n, d)| (n.to_string(), *d)).collect();
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        let d: Vec<(&str, u64)> = delays.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        let names: Vec<&str> = d.iter().map(|(n, _)| *n).collect();
        start(r, &names, concat_strategy(), Some(timeout), streaming);
        targets(r, &d, true);
    })
    .await
}

#[tokio::test]
async fn test_multicast_parallel_timeout() {
    let t = timeout_test(&[("a", 1000), ("b", 0), ("c", 0)], 250, false).await;
    t.mock("mock:result")
        .message_matches(0, not(body().contains("A")));
    t.mock("mock:A").expected_message_count(0);
    t.mock("mock:B").expected_message_count(1);
    t.mock("mock:C").expected_message_count(1);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_timeout_2() {
    let t = timeout_test(&[("a", 0), ("b", 1000), ("c", 0)], 250, false).await;
    t.mock("mock:result")
        .message_matches(0, not(body().contains("B")));
    t.mock("mock:A").expected_message_count(1);
    t.mock("mock:B").expected_message_count(0);
    t.mock("mock:C").expected_message_count(1);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_timeout_3() {
    let t = timeout_test(&[("a", 0), ("b", 0), ("c", 1000)], 250, false).await;
    t.mock("mock:result")
        .message_matches(0, not(body().contains("C")));
    t.mock("mock:A").expected_message_count(1);
    t.mock("mock:B").expected_message_count(1);
    t.mock("mock:C").expected_message_count(0);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_middle_timeout() {
    let t = timeout_test(&[("a", 0), ("b", 1000), ("c", 100)], 500, false).await;
    t.mock("mock:result").expected_bodies_received(["AC"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_last_timeout() {
    let t = timeout_test(&[("a", 500), ("b", 0), ("c", 3000)], 2000, false).await;
    t.mock("mock:result").expected_bodies_received(["AB"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_two_timeout() {
    let t = timeout_test(&[("a", 500), ("b", 0), ("c", 500)], 200, false).await;
    t.mock("mock:result").expected_bodies_received(["B"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_two_timeout_middle() {
    let t = timeout_test(
        &[("a", 0), ("b", 1000), ("c", 0), ("d", 1000), ("e", 0)],
        250,
        false,
    )
    .await;
    t.mock("mock:result").expected_bodies_received(["ACE"]);
    for (uri, count) in [
        ("mock:A", 1),
        ("mock:B", 0),
        ("mock:C", 1),
        ("mock:D", 0),
        ("mock:E", 1),
    ] {
        t.mock(uri).expected_message_count(count);
    }
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multicast_parallel_streaming_timeout() {
    let t = timeout_test(&[("a", 20000), ("b", 500), ("c", 0)], 1000, true).await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_bodies_received(["CB"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Java `MyAggregationStrategy` of the timeout aware tests: records the timeout call, and
/// sets the body to `AllTimeout` when `set_body`.
#[derive(Default)]
struct TimeoutAware {
    received: Mutex<Option<(String, i32, i32, i64)>>,
    concat: bool,
    set_body: bool,
}

impl AggregationStrategy for TimeoutAware {
    fn aggregate(
        &self,
        old: Option<Exchange>,
        new: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        if !self.concat {
            return Ok(old);
        }
        concat_strategy().aggregate(old, new)
    }

    fn timeout(&self, exchange: &mut Exchange, index: i32, total: i32, timeout_millis: i64) {
        *self.received.lock().unwrap() = Some((
            exchange.exchange_id().to_string(),
            index,
            total,
            timeout_millis,
        ));
        if self.set_body {
            exchange.in_message_mut().set_body("AllTimeout");
        }
    }
}

#[tokio::test]
async fn test_multicast_parallel_timeout_aware() {
    let strategy = Arc::new(TimeoutAware {
        concat: true,
        ..Default::default()
    });
    let s = strategy.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        start(r, &["a", "b", "c"], s.clone(), Some(1000), false);
        targets(r, &[("a", 2000), ("b", 0), ("c", 500)], false);
    })
    .await;
    t.mock("mock:result")
        .message_matches(0, not(body().contains("A")));
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let (_, index, total, timeout) = strategy.received.lock().unwrap().clone().unwrap();
    assert_eq!((0, 3, 1000), (index, total, timeout));
}

#[tokio::test]
async fn test_multicast_parallel_all_timeout_aware() {
    let strategy = Arc::new(TimeoutAware {
        set_body: true,
        ..Default::default()
    });
    let s = strategy.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        start(r, &["a", "b", "c"], s.clone(), Some(500), false);
        targets(r, &[("a", 1000), ("b", 2000), ("c", 1500)], false);
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["AllTimeout"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let (_, index, total, timeout) = strategy.received.lock().unwrap().clone().unwrap();
    assert_eq!((2, 3, 500), (index, total, timeout));
}
