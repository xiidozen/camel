//! Ports of `SplitterMapTest`, `SplitterPropertyContinuedTest`, `SplitterCompletedPropertyTest`,
//! `SplitterCollateTest`, `SplitWithDelimiterTest`, `SplitterSkipTest`,
//! `SplitterStopOnExceptionTest`, `SplitterNoStopOnExceptionTest` and
//! `SplitterThrowExceptionFromExpressionTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::api::exchange_constants::SPLIT_COMPLETE;
use camel_core::api::processor::{expression_fn, SharedExpression};
use camel_core::prelude::*;
use std::sync::Arc;

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|s| Value::from(*s)).collect())
}

#[tokio::test]
async fn test_split_map() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").split(body(), |s| {
            s.set_header("myKey", simple("${body.key}"))
                .set_body(simple("${body.value}"))
                .to("mock:line")
        }));
    })
    .await;
    let mock = t.mock("mock:line");
    mock.message_matches(0, body().is_equal_to("Hello World"));
    mock.message_matches(0, header("myKey").is_equal_to("123"));
    mock.message_matches(1, body().is_equal_to("Bye World"));
    mock.message_matches(1, header("myKey").is_equal_to("789"));
    let mut map = ValueMap::new();
    map.insert("123".into(), Value::from("Hello World"));
    map.insert("789".into(), Value::from("Bye World"));
    t.template
        .send_body("direct:start", Value::Map(map))
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_splitter_property_continued() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(camel_core::api::error::ErrorType::Any)
                .continued(true)
                .set_property("errorCode", constant("ERR-1")),
        );
        r.add(
            from("direct:start")
                .split(body(), |s| {
                    s.log("Step #1 - Body: ${body} with error code: ${exchangeProperty.errorCode}")
                        .choice(|c| {
                            c.when(body().contains("Kaboom"), |w| {
                                w.throw_exception(CamelError::illegal_argument("Damn"))
                            })
                        })
                        .log("Step #2 - Body: ${body} with error code: ${exchangeProperty.errorCode}")
                        .choice(|c| {
                            c.when(simple("${exchangeProperty.errorCode} != null"), |w| {
                                w.to("mock:error")
                            })
                            .otherwise(|o| o.to("mock:split"))
                        })
                })
                .to("mock:end"),
        );
    })
    .await;
    let end = t.mock("mock:end");
    end.expected_bodies_received(["A,Kaboom,B,C"]);
    end.message_matches(0, exchange_property("errorCode").is_null());
    let error = t.mock("mock:error");
    error.expected_bodies_received(["Kaboom"]);
    error.message_matches(0, exchange_property("errorCode").is_equal_to("ERR-1"));
    let split = t.mock("mock:split");
    split.expected_bodies_received(["A", "B", "C"]);
    split.all_messages_match(exchange_property("errorCode").is_null());
    t.template
        .send_body("direct:start", "A,Kaboom,B,C")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_splitter_completed_property() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").split(body().tokenize(","), |s| s.to("mock:split")));
        r.add(
            from("direct:streaming")
                .split(body().tokenize(","), |s| s.to("mock:split"))
                .streaming(),
        );
    })
    .await;
    for uri in ["direct:start", "direct:streaming"] {
        let mock = t.mock("mock:split");
        mock.reset();
        mock.expected_bodies_received(["Hello World", "Bye World", "Bye Moon"]);
        for (i, complete) in [false, false, true].into_iter().enumerate() {
            mock.message_matches(i, exchange_property(SPLIT_COMPLETE).is_equal_to(complete));
        }
        t.template
            .send_body(uri, "Hello World,Bye World,Bye Moon")
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_splitter_collate_and_skip() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:collate").split(simple("${collate(3)}"), |s| s.to("mock:chunk")));
        r.add(from("direct:skip").split(simple("${skip(2)}"), |s| s.to("mock:line")));
    })
    .await;
    let chunk = t.mock("mock:chunk");
    chunk.expected_message_count(2);
    t.template
        .send_body("direct:collate", list(&["A", "B", "C", "D", "E"]))
        .await
        .unwrap();
    chunk.assert_is_satisfied().await;
    let received = chunk.received_exchanges();
    assert_eq!(&list(&["A", "B", "C"]), received[0].message().body());
    assert_eq!(&list(&["D", "E"]), received[1].message().body());

    let line = t.mock("mock:line");
    line.expected_bodies_received(["C", "D", "E"]);
    t.template
        .send_body("direct:skip", list(&["A", "B", "C", "D", "E"]))
        .await
        .unwrap();
    line.assert_is_satisfied().await;
    for data in [list(&["A", "B"]), list(&["A"])] {
        line.reset();
        line.expected_message_count(0);
        t.template.send_body("direct:skip", data).await.unwrap();
        line.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_split_with_delimiter() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start-0")
                .split(body(), |s| s.to("mock:result-0"))
                .delimiter("false"),
        );
        r.add(from("direct:start-1").split(body(), |s| s.to("mock:result-1")));
        r.add(
            from("direct:start-2")
                .split(body(), |s| s.to("mock:result-2"))
                .delimiter("#"),
        );
    })
    .await;
    for (n, body, size, expected) in [
        (0, "some , # false text", 1, vec!["some , # false text"]),
        (1, "some , # false text", 2, vec!["some ", " # false text"]),
        (
            2,
            "some , # false # text",
            3,
            vec!["some , ", " false ", " text"],
        ),
    ] {
        let m = t.mock(&format!("mock:result-{n}"));
        m.expected_property_received("CamelSplitSize", size);
        m.expected_bodies_received(expected);
        t.template
            .send_body(&format!("direct:start-{n}"), body)
            .await
            .unwrap();
        m.assert_is_satisfied().await;
    }
}

fn kaboom(e: &mut Exchange) -> CamelResult<()> {
    if matches!(e.message().body(), Value::Text(t) if t == "Kaboom") {
        return Err(CamelError::illegal_argument("Forced"));
    }
    Ok(())
}

#[tokio::test]
async fn test_split_stop_on_exception() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:stop")
                .split(body().tokenize(","), |s| {
                    s.process_fn(kaboom).to("mock:split")
                })
                .stop_on_exception(),
        );
        r.add(from("direct:nostop").split(body().tokenize(","), |s| {
            s.process_fn(kaboom).to("mock:split")
        }));
    })
    .await;
    let mock = t.mock("mock:split");
    mock.expected_bodies_received(["Hello World", "Bye World"]);
    t.template
        .send_body("direct:stop", "Hello World,Bye World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;

    mock.reset();
    mock.expected_bodies_received(["Hello World"]);
    let e = t
        .template
        .send_body("direct:stop", "Hello World,Kaboom,Bye World")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::CamelExchange), "{e}");
    assert!(
        cause
            .message()
            .unwrap_or_default()
            .starts_with("Multicast processing failed for number 1."),
        "{cause}"
    );
    assert_eq!(Some("Forced"), cause_of(cause).message());
    mock.assert_is_satisfied().await;

    for (body, expected) in [
        ("Hello World,Bye World,Hi World", true),
        ("Hello World,Kaboom,Bye World,Hi World", false),
    ] {
        mock.reset();
        mock.expected_bodies_received(["Hello World", "Bye World", "Hi World"]);
        let r = t.template.send_body("direct:nostop", body).await;
        assert_eq!(expected, r.is_ok());
        if let Err(e) = r {
            let cause = cause_of(&e);
            assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
            assert_eq!(Some("Forced"), cause.message());
        }
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_splitter_throw_exception_from_expression() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::ExpressionEvaluation)
                .handled(true)
                .to("mock://error"),
        );
        let split_me: SharedExpression = Arc::new(expression_fn(|e: &Exchange| {
            let _ = e;
            Err(CamelError::expression_evaluation(
                CamelError::illegal_state("splitMe"),
            ))
        }));
        r.add(from("direct://start").split(split_me, |s| s.to("mock://result")));
    })
    .await;
    t.mock("mock:error").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
