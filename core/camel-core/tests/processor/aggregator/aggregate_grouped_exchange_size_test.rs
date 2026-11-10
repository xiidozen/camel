//! Port of `AggregateGroupedExchangeSizeTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::processor::aggregate::GroupedExchangeAggregationStrategy;
use std::sync::Arc;

fn bodies(e: &Exchange) -> Vec<String> {
    match e.message().body() {
        Value::List(l) => l
            .iter()
            .map(|v| {
                v.downcast_ref::<Exchange>()
                    .expect("an exchange")
                    .message()
                    .body()
                    .to_string()
            })
            .collect(),
        other => panic!("not a list: {other:?}"),
    }
}

#[tokio::test]
async fn test_grouped_size() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(
                    constant(true),
                    Arc::new(GroupedExchangeAggregationStrategy),
                    |a| a.to("mock:result"),
                )
                .completion_size(3),
        );
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_message_count(2);
    for b in ["100", "150", "130", "200", "190", "120"] {
        t.template.send_body("direct:start", b).await.unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
    let exchanges = result.received_exchanges();
    assert_eq!(vec!["100", "150", "130"], bodies(&exchanges[0]));
    assert_eq!(vec!["200", "190", "120"], bodies(&exchanges[1]));
}
