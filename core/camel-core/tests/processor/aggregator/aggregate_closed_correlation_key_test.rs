//! Port of `AggregateClosedCorrelationKeyTest`.

use crate::support::{await_until, body_in_aggregating_strategy, cause_of, ContextTestSupport};
use camel_core::prelude::*;
use std::time::Duration;

async fn routes(capacity: usize) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(header("id"), body_in_aggregating_strategy(), |a| {
                    a.to("mock:result")
                })
                .completion_size(2)
                .close_correlation_key_on_completion(capacity),
        );
    })
    .await
}

fn assert_closed(err: &CamelError, key: &str) {
    assert!(err.is(ErrorKind::CamelExecution), "{err:?}");
    let cause = cause_of(err);
    assert!(cause.is(ErrorKind::ClosedCorrelationKey), "{err:?}");
    assert_eq!(Some(key), cause.detail());
    assert!(
        cause
            .to_string()
            .starts_with(&format!("The correlation key [{key}] has been closed.")),
        "{cause}"
    );
}

#[tokio::test]
async fn test_aggregate_closed_correlation_key() {
    let t = routes(1000).await;
    t.mock("mock:result").expected_bodies_received(["A+B"]);
    t.template
        .send_body_and_header("direct:start", "A", "id", 1)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "B", "id", 1)
        .await
        .unwrap();
    let err = t
        .template
        .send_body_and_header("direct:start", "C", "id", 1)
        .await
        .unwrap_err();
    assert_closed(&err, "1");
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_aggregate_closed_correlation_key_cache() {
    let t = routes(2).await;
    let result = t.mock("mock:result");
    result.expected_bodies_received(["A+B", "C+D", "E+F"]);
    for (b, id) in [("A", 1), ("B", 1), ("C", 2), ("D", 2), ("E", 3), ("F", 3)] {
        t.template
            .send_body_and_header("direct:start", b, "id", id)
            .await
            .unwrap();
    }
    await_until(Duration::from_secs(5), || result.received_counter() >= 3).await;
    let mut closed = 0;
    for (b, id) in [("G", 1), ("H", 2), ("I", 3)] {
        if let Err(err) = t
            .template
            .send_body_and_header("direct:start", b, "id", id)
            .await
        {
            closed += 1;
            assert_closed(&err, &id.to_string());
        }
    }
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(2, closed, "There should be 2 closed");
}
