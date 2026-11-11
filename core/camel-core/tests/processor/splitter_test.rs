//! Port of `SplitterTest` (`testSplitterWithStreamingAndFileBody` waits for file bodies;
//! `testSplitterWithIterable` is covered by the list body test: Rust has no lazy iterable
//! bodies).

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::{SPLIT_COMPLETE, SPLIT_INDEX, SPLIT_SIZE};
use camel_core::api::processor::{AggregationStrategy, SharedAggregationStrategy};
use camel_core::prelude::*;
use camel_core::processor::aggregate::UseLatestAggregationStrategy;
use std::collections::HashSet;
use std::sync::Arc;

/// Java `MyAggregationStrategy`: `UseLatestAggregationStrategy` counting the aggregations
/// in the `aggregated` property.
struct MyAggregationStrategy;

impl AggregationStrategy for MyAggregationStrategy {
    fn aggregate(
        &self,
        old: Option<Exchange>,
        new: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        let count = old
            .as_ref()
            .and_then(|o| o.property("aggregated"))
            .and_then(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            })
            .unwrap_or(0);
        let mut result = UseLatestAggregationStrategy.aggregate(old, new)?;
        if let Some(r) = result.as_mut() {
            r.set_property("aggregated", count + 1);
        }
        Ok(result)
    }
}

fn my_aggregation_strategy() -> SharedAggregationStrategy {
    Arc::new(MyAggregationStrategy)
}

/// Sends the body with the `foo` header to the uri.
async fn request_with_foo(t: &ContextTestSupport, uri: &str, body: &str) -> Exchange {
    let mut e = t
        .template
        .create_exchange(uri, ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body(body);
    e.message_mut().set_header("foo", "bar");
    t.template.send(uri, e).await.unwrap()
}

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(on_exception(ErrorKind::Camel).to("mock:failed"));
        r.add(
            from("direct:seqential")
                .split(body(), |s| s.to("mock:result"))
                .aggregation_strategy(Arc::new(UseLatestAggregationStrategy)),
        );
        r.add(
            from("direct:parallel")
                .split(body(), |s| s.to("mock:result"))
                .aggregation_strategy(my_aggregation_strategy())
                .parallel_processing(),
        );
        r.add(
            from("direct:parallelAggregate")
                .split(body(), |s| s.to("mock:result"))
                .aggregation_strategy(my_aggregation_strategy())
                .parallel_processing()
                .parallel_aggregate(),
        );
        r.add(
            from("direct:streaming")
                .split(body(), |s| s.to("mock:result"))
                .streaming(),
        );
        r.add(
            from("direct:parallel-streaming")
                .split(body(), |s| s.to("mock:result"))
                .aggregation_strategy(my_aggregation_strategy())
                .parallel_processing()
                .streaming(),
        );
        r.add(
            from("direct:exception")
                .split(body(), |s| {
                    s.process_fn(|e| {
                        if e.in_message().body() == "Exception" {
                            return Err(CamelError::new(
                                ErrorKind::Camel,
                                "Just want to throw exception here",
                            ));
                        }
                        Ok(())
                    })
                    .to("mock:result")
                })
                .aggregation_strategy(my_aggregation_strategy())
                .parallel_processing(),
        );
        r.add(from("direct:simple").split(body(), |s| s.to("mock:result")));
    })
    .await
}

#[tokio::test]
async fn test_sending_a_message_using_multicast_receives_its_own_exchange() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_bodies_received(["James", "Guillaume", "Hiram", "Rob"]);
    let mut e = t
        .template
        .create_exchange("direct:seqential", ExchangePattern::InOnly)
        .unwrap();
    e.message_mut().set_body("James,Guillaume,Hiram,Rob");
    e.message_mut().set_header("foo", "bar");
    t.template.send("direct:seqential", e).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let mut ids = HashSet::new();
    for (i, e) in result.received_exchanges().iter().enumerate() {
        ids.insert(e.exchange_id().to_string());
        assert_eq!(Some(&Value::Int(i as i64)), e.property(SPLIT_INDEX));
        assert_eq!(Some(&Value::Int(4)), e.property(SPLIT_SIZE));
    }
    assert_eq!(
        4,
        ids.len(),
        "The sub messages should have unique exchange ids"
    );
}

#[tokio::test]
async fn test_splitter_with_aggregation_strategy() {
    let t = setup().await;
    t.mock("mock:result")
        .expected_bodies_received(["James", "Guillaume", "Hiram", "Rob", "Roman"]);
    let mut e = t
        .template
        .create_exchange("direct:seqential", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body("James,Guillaume,Hiram,Rob,Roman");
    e.message_mut().set_header("foo", "bar");
    let result = t.template.send("direct:seqential", e).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let out = result.message();
    assert_eq!("Roman", out.body().to_string());
    assert_eq!(
        Some("bar".to_string()),
        out.header_as::<String>("foo").unwrap()
    );
    assert_eq!(Some(&Value::Int(4)), result.property(SPLIT_INDEX));
}

#[tokio::test]
async fn test_empty_body() {
    let t = setup().await;
    t.mock("mock:result").expected_message_count(0);
    let mut e = t
        .template
        .create_exchange("direct:seqential", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_header("foo", "bar");
    let result = t.template.send("direct:seqential", e).await.unwrap();
    assert!(result.exception().is_none(), "Should not fail");
    t.assert_mock_endpoints_satisfied().await;
}

// the route direct:simple of the Java test (not used by its tests): a list body
#[tokio::test]
async fn test_split_list_body() {
    let t = setup().await;
    t.mock("mock:result").expected_bodies_received(["A", "B"]);
    t.template
        .send_body(
            "direct:simple",
            Value::List(vec![Value::from("A"), Value::from("B")]),
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_sending_a_message_using_multicast_receives_its_own_exchange_parallel() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expects_no_duplicates(body().expression());
    result.expected_message_count(4);
    let mut e = t
        .template
        .create_exchange("direct:parallel", ExchangePattern::InOnly)
        .unwrap();
    e.message_mut().set_body("James,Guillaume,Hiram,Rob");
    e.message_mut().set_header("foo", "bar");
    t.template.send("direct:parallel", e).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let names = ["James", "Guillaume", "Hiram", "Rob"];
    let mut found = HashSet::new();
    for e in result.received_exchanges() {
        let Some(Value::Int(i)) = e.property(SPLIT_INDEX) else {
            panic!("no split index");
        };
        found.insert(*i);
        assert_eq!(names[*i as usize], e.message().body().to_string());
        assert_eq!(Some(&Value::Int(4)), e.property(SPLIT_SIZE));
    }
    assert_eq!(4, found.len());
}

fn assert_foo_and_aggregated(result: &Exchange, aggregated: bool) {
    assert_eq!(
        Some("bar".to_string()),
        result.message().header_as::<String>("foo").unwrap()
    );
    if aggregated {
        assert_eq!(Some(&Value::Int(5)), result.property("aggregated"));
    }
}

#[tokio::test]
async fn test_splitter_with_aggregation_strategy_parallel() {
    let t = setup().await;
    t.mock("mock:result").expected_message_count(5);
    let result = request_with_foo(&t, "direct:parallel", "James,Guillaume,Hiram,Rob,Roman").await;
    t.assert_mock_endpoints_satisfied().await;
    assert_foo_and_aggregated(&result, true);
}

#[tokio::test]
async fn test_splitter_with_aggregation_strategy_parallel_streaming() {
    let t = setup().await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(5);
    mock.expected_bodies_received_in_any_order(["James", "Guillaume", "Hiram", "Rob", "Roman"]);
    let result = request_with_foo(
        &t,
        "direct:parallel-streaming",
        "James,Guillaume,Hiram,Rob,Roman",
    )
    .await;
    t.assert_mock_endpoints_satisfied().await;
    assert_foo_and_aggregated(&result, true);
}

#[tokio::test]
async fn test_splitter_parallel_aggregate() {
    let t = setup().await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(5);
    mock.expected_bodies_received_in_any_order(["James", "Guillaume", "Hiram", "Rob", "Roman"]);
    let result = request_with_foo(
        &t,
        "direct:parallelAggregate",
        "James,Guillaume,Hiram,Rob,Roman",
    )
    .await;
    t.assert_mock_endpoints_satisfied().await;
    assert_foo_and_aggregated(&result, false);
}

#[tokio::test]
async fn test_splitter_with_streaming_and_string_body() {
    let t = setup().await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(5);
    mock.expected_header_received("foo", "bar");
    request_with_foo(&t, "direct:streaming", "James,Guillaume,Hiram,Rob,Roman").await;
    t.assert_mock_endpoints_satisfied().await;
    // check properties with split details is correct
    let received = mock.received_exchanges();
    let size = received.len();
    for (i, e) in received.iter().enumerate() {
        assert_eq!(Some(&Value::Int(i as i64)), e.property(SPLIT_INDEX));
        if i < size - 1 {
            assert_eq!(Some(&Value::Bool(false)), e.property(SPLIT_COMPLETE));
            // this property cannot be set when streaming is used, except for the last
            // exchange
            assert_eq!(None, e.property(SPLIT_SIZE));
        } else {
            assert_eq!(Some(&Value::Bool(true)), e.property(SPLIT_COMPLETE));
            // when we are complete the size is set
            assert_eq!(Some(&Value::Int(size as i64)), e.property(SPLIT_SIZE));
        }
    }
}

#[tokio::test]
async fn test_splitter_with_exception() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_message_count(4);
    result.expected_header_received("foo", "bar");
    let failed = t.mock("mock:failed");
    failed.expected_message_count(1);
    failed.expected_header_received("foo", "bar");
    let e = request_with_foo(
        &t,
        "direct:exception",
        "James,Guillaume,Hiram,Rob,Exception",
    )
    .await;
    assert!(
        e.exception().is_some_and(|e| e.is(ErrorKind::Camel)),
        "The result exchange should have a camel exception"
    );
    t.assert_mock_endpoints_satisfied().await;
}
