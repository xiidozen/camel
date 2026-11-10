//! Port of the sequential `SplitterTest` cases (parallel processing and streaming are not
//! ported yet).

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::{SPLIT_INDEX, SPLIT_SIZE};
use camel_core::prelude::*;
use camel_core::processor::aggregate::UseLatestAggregationStrategy;
use std::collections::HashSet;
use std::sync::Arc;

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(on_exception(ErrorKind::Camel).to("mock:failed"));
        r.add(
            from("direct:seqential")
                .split(body(), |s| s.to("mock:result"))
                .aggregation_strategy(Arc::new(UseLatestAggregationStrategy)),
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
