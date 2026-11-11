//! Ports of `Split123ParallelTest`, `SplitterParallelStopOnExceptionTest`,
//! `SplitterParallelNoStopOnExceptionTest`, `SplitParallelTimeoutTest`,
//! `SplitParallelTimeoutNoAggregationStrategyTest`, `SplitterNullBodyTest` and
//! `SplitterNullBodyParallelTest`.
//!
//! The Java `Phaser`s and custom executor services are not needed (tokio tasks, all spawned
//! before the timeout starts). `StringAggregationStrategy` without delimiter is
//! [`concat_strategy`].

use crate::support::{cause_of, concat_strategy, ContextTestSupport};
use camel_core::api::processor::AggregationStrategy;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn test_split_123_parallel() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .split(body(), |s| s.to("mock:foo"))
                .aggregation_strategy(concat_strategy())
                .parallel_processing()
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:foo")
        .expected_bodies_received_in_any_order(["1", "2", "3"]);
    t.mock("mock:result").expected_bodies_received(["123"]);
    t.template.send_body("direct:start", "1,2,3").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn kaboom(stop_on_exception: bool) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        let mut route = from("direct:start")
            .split(body(), |s| {
                s.process_fn(|e| {
                    if e.in_message().body() == "Kaboom" {
                        return Err(CamelError::illegal_argument("Forced"));
                    }
                    Ok(())
                })
                .to("mock:split")
            })
            .parallel_processing();
        if stop_on_exception {
            route = route.stop_on_exception();
        }
        r.add(route);
    })
    .await
}

#[tokio::test]
async fn test_split_parallel_stop_on_exception_ok() {
    let t = kaboom(true).await;
    t.mock("mock:split")
        .expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
    t.template
        .send_body("direct:start", "Hello World,Bye World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_split_parallel_stop_on_exception_stop() {
    let t = kaboom(true).await;
    let mock = t.mock("mock:split");
    mock.expected_minimum_message_count(0);
    mock.all_messages_match(not(body().is_equal_to("Kaboom")));
    let err = t
        .template
        .send_body("direct:start", "Hello World,Goodday World,Kaboom,Bye World")
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
async fn test_split_parallel_no_stop_on_exception_ok() {
    let t = kaboom(false).await;
    t.mock("mock:split").expected_bodies_received_in_any_order([
        "Hello World",
        "Bye World",
        "Hi World",
    ]);
    t.template
        .send_body("direct:start", "Hello World,Bye World,Hi World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_split_parallel_no_stop_on_exception_stop() {
    let t = kaboom(false).await;
    let mock = t.mock("mock:split");
    mock.all_messages_match(not(body().is_equal_to("Kaboom")));
    mock.expected_bodies_received_in_any_order([
        "Hello World",
        "Goodday World",
        "Bye World",
        "Hi World",
    ]);
    let err = t
        .template
        .send_body(
            "direct:start",
            "Hello World,Goodday World,Kaboom,Bye World,Hi World",
        )
        .await
        .unwrap_err();
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::IllegalArgument), "{cause:?}");
    assert_eq!(Some("Forced"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}

/// Java `MyAggregationStrategy` of `SplitParallelTimeoutTest`.
#[derive(Default)]
struct TimeoutAware {
    received: Mutex<Option<(i32, i32, i64)>>,
}

impl AggregationStrategy for TimeoutAware {
    fn aggregate(
        &self,
        old: Option<Exchange>,
        new: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        concat_strategy().aggregate(old, new)
    }

    fn timeout(&self, _exchange: &mut Exchange, index: i32, total: i32, timeout_millis: i64) {
        *self.received.lock().unwrap() = Some((index, total, timeout_millis));
    }
}

async fn split_timeout(strategy: Option<Arc<dyn AggregationStrategy>>) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        let mut route = from("direct:start").split(body(), |s| {
            s.choice(|c| {
                c.when(body().is_equal_to("A"), |s| s.to("direct:a"))
                    .when(body().is_equal_to("B"), |s| s.to("direct:b"))
                    .when(body().is_equal_to("C"), |s| s.to("direct:c"))
            })
        });
        if let Some(s) = &strategy {
            route = route.aggregation_strategy(s.clone());
        }
        r.add(route.parallel_processing().timeout(100).to("mock:result"));
        r.add(
            from("direct:a")
                .delay(constant(200))
                .set_body(constant("A")),
        );
        r.add(from("direct:b").set_body(constant("B")));
        r.add(from("direct:c").delay(constant(10)).set_body(constant("C")));
    })
    .await
}

#[tokio::test]
async fn test_split_parallel_timeout() {
    let strategy = Arc::new(TimeoutAware::default());
    let t = split_timeout(Some(strategy.clone())).await;
    t.mock("mock:result")
        .message_matches(0, not(body().contains("A")));
    t.template.send_body("direct:start", "A,B,C").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(Some((0, 3, 100)), *strategy.received.lock().unwrap());
}

#[tokio::test]
async fn test_split_parallel_timeout_no_aggregation_strategy() {
    let t = split_timeout(None).await;
    // Java asserts `message(0).body().not(body().contains("A"))`, which compares the body to
    // the predicate object and so always holds; without a strategy the splitter keeps the
    // original message, which is asserted here
    t.mock("mock:result").expected_bodies_received(["A,B,C"]);
    t.template.send_body("direct:start", "A,B,C").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// SplitterNullBodyTest and SplitterNullBodyParallelTest
async fn null_body(parallel: bool) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        for (uri, streaming) in [("direct:start", false), ("direct:streaming", true)] {
            let mut route = from(uri).split(body(), |s| s.to("mock:split"));
            if streaming {
                route = route.streaming();
            }
            if parallel {
                route = route.parallel_processing();
            }
            r.add(route.to("mock:result"));
        }
    })
    .await
}

async fn null_body_case(parallel: bool, uri: &str, body: Value, parts: i32) {
    let t = null_body(parallel).await;
    t.mock("mock:split").expected_message_count(parts);
    t.mock("mock:result").expected_message_count(1);
    t.template.send_body(uri, body).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_split_null_body_cases() {
    for parallel in [false, true] {
        for uri in ["direct:start", "direct:streaming"] {
            null_body_case(parallel, uri, Value::from("A,B,C"), 3).await;
            null_body_case(parallel, uri, Value::List(vec![]), 0).await;
            null_body_case(parallel, uri, Value::Null, 0).await;
        }
    }
}
