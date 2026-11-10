//! Port of `AggregateExpressionSizeTest`.

use crate::support::{body_in_aggregating_strategy, ContextTestSupport};
use camel_core::prelude::*;

#[tokio::test]
async fn test_aggregate_expression_size() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:aggregated")
                })
                .completion_size_expression(header("mySize")),
        );
    })
    .await;
    t.mock("mock:aggregated")
        .expected_bodies_received(["A+B+C"]);
    for b in ["A", "B", "C"] {
        let headers = vec![
            ("id".to_string(), Value::from(123)),
            ("mySize".to_string(), Value::from(3)),
        ];
        t.template
            .send_body_and_headers("direct:start", b, headers)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}
