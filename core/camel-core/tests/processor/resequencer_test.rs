//! Ports of `ResequencerTest`, `BatchResequencerAllowDuplicatesTest`, `StreamResequencerTest`,
//! `ResequenceStreamDuplicateTest`, `ResequenceStreamRejectOldExchangesTest` and the invalid
//! exchanges cases.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use std::time::Duration;

async fn send_bodies(t: &ContextTestSupport, uri: &str, bodies: &[&str]) {
    for b in bodies {
        t.template.send_body(uri, *b).await.unwrap();
    }
}

async fn batch() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .route_id("myRoute")
                .resequence(body(), |s| s.to("mock:result"))
                .timeout(50),
        );
    })
    .await
}

#[tokio::test]
async fn test_send_messages_in_wrong_order_but_receive_them_in_correct_order() {
    let t = batch().await;
    t.mock("mock:result")
        .expected_bodies_received(["Guillaume", "Hiram", "James", "Rob"]);
    send_bodies(&t, "direct:start", &["Rob", "Hiram", "Guillaume", "James"]).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_restart_route() {
    let t = batch().await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Guillaume", "Hiram", "James", "Rob"]);
    send_bodies(&t, "direct:start", &["Rob", "Hiram", "Guillaume", "James"]).await;
    mock.assert_is_satisfied().await;
    t.context.stop_route("myRoute").await.unwrap();
    mock.reset();
    t.context.start_route("myRoute").await.unwrap();
    mock.expected_bodies_received(["Donald", "Goofy", "Jack"]);
    send_bodies(&t, "direct:start", &["Jack", "Donald", "Goofy"]).await;
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_batch_resequencer_allow_duplicates() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .resequence(header("id"), |s| s.to("mock:result"))
                .allow_duplicates()
                .timeout(100),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["1A", "1B", "2C", "2D", "2E", "2F", "3G", "4H"]);
    for (b, id) in [
        ("1A", "1"),
        ("2C", "2"),
        ("2D", "2"),
        ("4H", "4"),
        ("1B", "1"),
        ("2E", "2"),
        ("3G", "3"),
        ("2F", "2"),
    ] {
        t.template
            .send_body_and_header("direct:start", b, "id", id)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_batch_reverse_and_ignore_invalid() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .resequence(header("seqno"), |s| s.to("mock:result"))
                .reverse()
                .ignore_invalid_exchanges()
                .timeout(100),
        );
        r.add(
            from("direct:strict")
                .resequence(header("seqno"), |s| s.to("mock:strict"))
                .timeout(100),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["C", "B", "A"]);
    for (b, n) in [
        ("B", Value::Int(2)),
        ("A", 1.into()),
        ("X", Value::Null),
        ("C", 3.into()),
    ] {
        t.template
            .send_body_and_header("direct:start", b, "seqno", n)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
    let e = t
        .template
        .send_body_and_header("direct:strict", "X", "other", 1)
        .await;
    assert!(
        e.is_err(),
        "an exchange without a sequence value is invalid"
    );
}

async fn stream() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .resequence(header("seqnum"), |s| s.to("mock:result"))
                .stream()
                .timeout(1000),
        );
    })
    .await
}

#[tokio::test]
async fn test_stream_wrong_order() {
    let t = stream().await;
    t.mock("mock:result")
        .expected_bodies_received(["msg1", "msg2", "msg3", "msg4"]);
    for (b, n) in [("msg4", 4), ("msg1", 1), ("msg3", 3), ("msg2", 2)] {
        t.template
            .send_body_and_header("direct:start", b, "seqnum", n)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_stream_multithreaded() {
    let t = stream().await;
    let bodies: Vec<String> = (0..100).map(|i| format!("msg{i}")).collect();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(bodies.clone());
    mock.set_result_wait_time(20_000);
    let mut senders = Vec::new();
    for start in 0..2 {
        let template = t.context.create_producer_template();
        senders.push(tokio::spawn(async move {
            let mut i = start;
            while i < 100 {
                template
                    .send_body_and_header("direct:start", format!("msg{i}"), "seqnum", i as i64)
                    .await
                    .unwrap();
                i += 2;
            }
        }));
    }
    for s in senders {
        s.await.unwrap();
    }
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_stream_duplicate_keeps_first() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .resequence(header("seqno"), |s| s.to("mock:result"))
                .stream()
                .timeout(2000),
        );
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["A1", "B"]);
    for (b, n) in [("B", 2), ("A1", 1), ("A2", 1)] {
        t.template
            .send_body_and_header("direct:start", b, "seqno", n)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

async fn reject_old() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(camel_core::api::error::ErrorType::Class(
                "org.apache.camel.processor.resequencer.MessageRejectedException".into(),
            ))
            .maximum_redeliveries(0)
            .handled(true)
            .to("mock:error"),
        );
        r.add(
            from("direct:start")
                .resequence(header("seqno"), |s| s.to("mock:result"))
                .stream()
                .capacity(3)
                .reject_old()
                .timeout(1000),
        );
    })
    .await
}

async fn send_seq(t: &ContextTestSupport, items: &[(&str, i64)]) {
    for (b, n) in items {
        t.template
            .send_body_and_header("direct:start", *b, "seqno", *n)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_in_sequence_after_capacity_reached() {
    let t = reject_old().await;
    t.mock("mock:result")
        .expected_bodies_received(["A", "B", "C", "E"]);
    t.mock("mock:error").expected_message_count(0);
    send_seq(&t, &[("B", 2), ("C", 3), ("A", 1), ("E", 5)]).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_out_of_sequence_after_capacity_reached() {
    let t = reject_old().await;
    t.mock("mock:result")
        .expected_bodies_received(["B", "C", "D"]);
    t.mock("mock:error").expected_bodies_received(["A"]);
    send_seq(&t, &[("D", 4), ("C", 3), ("B", 2), ("A", 1)]).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_out_of_sequence_after_capacity_reached_complex() {
    let t = reject_old().await;
    t.mock("mock:result")
        .expected_bodies_received(["A", "D", "E", "F"]);
    t.mock("mock:error").expected_bodies_received(["B", "C"]);
    send_seq(&t, &[("E", 5), ("D", 4), ("A", 1)]).await;
    crate::support::await_until(Duration::from_secs(5), || {
        t.mock("mock:result").received_counter() >= 1
    })
    .await;
    send_seq(&t, &[("B", 2), ("C", 3), ("F", 6)]).await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Ports of `ResequenceStreamIgnoreInvalidExchangesTest`,
/// `ResequenceStreamNotIgnoreInvalidExchangesTest`,
/// `ResequenceBatchIgnoreInvalidExchangesTest` and
/// `ResequenceBatchNotIgnoreInvalidExchangesTest`: the invalid message `A` (without `seqno`)
/// sent first to fourth is ignored, or fails without disturbing the others.
#[tokio::test]
async fn test_invalid_exchanges() {
    for stream in [true, false] {
        for ignore in [true, false] {
            for bad in 0..4 {
                let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
                    let route = from("direct:start")
                        .resequence(header("seqno"), |s| s.to("mock:result"))
                        .timeout(100);
                    let route = if stream { route.stream() } else { route };
                    r.add(if ignore {
                        route.ignore_invalid_exchanges()
                    } else {
                        route
                    });
                })
                .await;
                t.mock("mock:result")
                    .expected_bodies_received(["B", "C", "D"]);
                let mut valid = vec![("D", 4), ("C", 3), ("B", 2)].into_iter();
                for i in 0..4 {
                    if i == bad {
                        let r = t.template.send_body("direct:start", "A").await;
                        assert_eq!(
                            ignore,
                            r.is_ok(),
                            "stream={stream} ignore={ignore} bad={bad}"
                        );
                    } else {
                        let (b, n) = valid.next().unwrap();
                        t.template
                            .send_body_and_header("direct:start", b, "seqno", n)
                            .await
                            .unwrap();
                    }
                }
                t.assert_mock_endpoints_satisfied().await;
            }
        }
    }
}
