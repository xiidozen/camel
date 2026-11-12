//! Ports of `MessageHistoryCopyExchangeTest`, `MessageHistoryStepOnlyTest`,
//! `MessageHistoryCopyMessageTest`, `MessageHistoryBodyTypeAndSizeTest` and
//! `MessageHistoryDumpRoutingTest`, and of the simple `SimpleMessageHistoryTest` and
//! `SimpleMessageHistoryNotDetailedTest`.

use crate::support::ContextTestSupport;
use camel_core::api::context::context_plugin;
use camel_core::api::exchange_constants::MESSAGE_HISTORY;
use camel_core::api::message_history::{message_history, MessageHistoryFactory};
use camel_core::prelude::*;

/// A context with the message history enabled (with the node pattern), and the routes.
async fn context(
    pattern: Option<&str>,
    copy: bool,
    size: bool,
    routes: impl RouteBuilder,
) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    t.context.set_message_history(true);
    let factory = context_plugin::<MessageHistoryFactory>(t.context.as_ref()).unwrap();
    factory.set_node_pattern(pattern);
    factory.set_copy_message(copy);
    factory.set_size_enabled(size);
    t.context.add_routes(routes).await.unwrap();
    t.context.start().await.unwrap();
    t
}

async fn request(t: &ContextTestSupport, uri: &str, body: &str) -> Exchange {
    let mut e = t
        .template
        .create_exchange(uri, ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body(body);
    t.template.send(uri, e).await.unwrap()
}

#[tokio::test]
async fn test_defensive_copy_of_message_history() {
    let t = context(None, false, false, |r: &mut Routes| {
        r.add(
            from("seda:start")
                .to("log:foo")
                .to("mock:a")
                .to("direct:bar")
                .to("mock:b"),
        );
        r.add(from("direct:bar").to("log:bar").to("mock:bar"));
    })
    .await;
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:b").expected_message_count(1);
    t.template
        .send_body("seda:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let a = message_history(&t.mock("mock:a").received_exchanges()[0]).unwrap();
    let b = message_history(&t.mock("mock:b").received_exchanges()[0]).unwrap();
    // each copy has its own list: mock:a saw the history up to itself
    let ids = |h: &[camel_core::api::message_history::MessageHistory]| {
        h.iter().map(|x| x.label.clone()).collect::<Vec<_>>()
    };
    assert_eq!(vec!["to[log:foo]", "to[mock:a]"], ids(&a));
    assert_eq!(
        vec![
            "to[log:foo]",
            "to[mock:a]",
            "to[direct:bar]",
            "to[log:bar]",
            "to[mock:bar]",
            "to[mock:b]"
        ],
        ids(&b)
    );
}

fn step_routes(r: &mut Routes) {
    r.add(
        from("direct:start")
            .step("a", |s| s.to("log:foo").to("mock:a"))
            .step("b", |s| s.to("direct:bar").to("mock:b")),
    );
    r.add(from("direct:bar").step("bar", |s| s.to("log:bar").to("mock:bar")));
}

#[tokio::test]
async fn test_step_only() {
    let t = context(Some("step"), false, false, step_routes).await;
    for m in ["mock:a", "mock:b", "mock:bar"] {
        t.mock(m).expected_message_count(1);
    }
    let out = request(&t, "direct:start", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
    let history = message_history(&out).unwrap();
    assert_eq!(3, history.len());
    for (h, id) in history.iter().zip(["a", "b", "bar"]) {
        assert_eq!("step", h.short_name);
        assert_eq!(id, h.node_id);
    }
}

#[tokio::test]
async fn test_copy_message() {
    let t = context(Some("step"), true, false, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .step("a", |s| s.transform(constant("Bye World")).to("mock:a"))
                .step("b", |s| {
                    s.transform(constant("Hi World"))
                        .to("direct:bar")
                        .to("mock:b")
                }),
        );
        r.add(from("direct:bar").step("bar", |s| s.to("log:bar").to("mock:bar")));
    })
    .await;
    for m in ["mock:a", "mock:b", "mock:bar"] {
        t.mock(m).expected_message_count(1);
    }
    let out = request(&t, "direct:start", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
    let history = message_history(&out).unwrap();
    assert_eq!(3, history.len());
    for (h, (id, body)) in history.iter().zip([
        ("a", "Hello World"),
        ("b", "Bye World"),
        ("bar", "Hi World"),
    ]) {
        assert_eq!(id, h.node_id);
        assert_eq!(&Value::from(body), h.message.as_ref().unwrap().body());
    }
}

#[tokio::test]
async fn test_body_type_and_size_per_step() {
    let t = context(Some("step"), false, true, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .step("a", |s| s.convert_body_to("byte[]"))
                .step("b", |s| {
                    s.transform(constant(Value::List(vec![1.into(), 2.into(), 3.into()])))
                })
                .step("c", |s| {
                    s.process_fn(|e| {
                        e.message_mut().set_body(Value::Null);
                        Ok(())
                    })
                })
                .step("d", |s| s.to("mock:result")),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    let out = request(&t, "direct:start", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
    let history = message_history(&out).unwrap();
    assert_eq!(4, history.len());
    let expected = [
        ("a", "java.lang.String", 11),
        ("b", "byte[]", 11),
        ("c", "java.util.ArrayList", 3),
        ("d", "null", -1),
    ];
    for (h, (id, ty, size)) in history.iter().zip(expected) {
        assert_eq!(id, h.node_id);
        assert_eq!(Some(ty), h.body_type.as_deref());
        assert_eq!(size, h.body_size, "{id}");
    }
    let table =
        camel_core::support::message_helper::dump_message_history_stacktrace(&out, None, false);
    for text in [
        "Body type",
        "java.lang.String",
        "byte[]",
        "java.util.ArrayList",
        "null",
    ] {
        assert!(table.contains(text), "{table}");
    }
    let route_row = table
        .lines()
        .find(|l| l.contains("from[direct://start]"))
        .unwrap_or_default();
    assert!(
        route_row.contains("null") && !route_row.trim().ends_with('0'),
        "route row: {route_row}"
    );
}

#[tokio::test]
async fn test_dump_routing() {
    let body = "Hello World 1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890";
    let t = context(None, false, false, |r: &mut Routes| {
        r.add(
            from("seda:start")
                .to("log:foo")
                .to("direct:bar")
                .delay(constant(30))
                .to("log:baz")
                .process_fn(|_| {
                    Err(CamelError::illegal_argument(
                        "Forced to dump message history",
                    ))
                })
                .to("mock:result"),
        );
        r.add(
            from("direct:bar")
                .to("log:bar")
                .delay(constant(10))
                .to("mock:bar"),
        );
    })
    .await;
    t.mock("mock:bar").expected_bodies_received([body]);
    t.template.send_body("seda:start", body).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_simple_message_history() {
    for (function, detailed) in [
        ("${messageHistory}", true),
        ("${messageHistory(false)}", false),
    ] {
        let t = context(None, false, false, move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .to("mock:a")
                    .log(function)
                    .to("mock:b")
                    .log(function)
                    .transform(simple(function))
                    .to("mock:result"),
            );
        })
        .await;
        for m in ["mock:a", "mock:b", "mock:result"] {
            t.mock(m).expected_message_count(1);
        }
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
        let result = t.mock("mock:result").received_exchanges()[0]
            .message()
            .body()
            .to_string();
        assert!(result.contains("mock:a"), "{result}");
        assert!(result.contains("mock:b"), "{result}");
        assert!(
            result.contains(&format!("transform[simple{{{function}}}")),
            "{result}"
        );
        assert_eq!(detailed, result.contains("Hello World"), "{result}");
        assert!(t.mock("mock:result").received_exchanges()[0]
            .property(MESSAGE_HISTORY)
            .is_some());
    }
}

/// Ports of `DefaultErrorHandlerLogExhaustedMessageHistoryTest`,
/// `DeadLetterChannelLogExhaustedMessageHistoryTest` and
/// `DeadLetterChannelLogExhaustedMessageHistoryWithBodyTest`: the exhausted failure is logged
/// with the message history table (and the exchange).
#[tokio::test]
async fn test_log_exhausted_message_history() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.error_handler(
            default_error_handler()
                .redelivery_delay(0)
                .maximum_redeliveries(3)
                .log_exhausted_message_history(true),
        );
        r.add(
            from("direct:start")
                .log("Incoming ${body}")
                .throw_exception(CamelError::illegal_argument("Forced")),
        );
    })
    .await;
    assert!(t
        .template
        .send_body("direct:start", "Hello World")
        .await
        .is_err());
    for body in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let mut dlc = dead_letter_channel("mock:dead")
                .redelivery_delay(0)
                .maximum_redeliveries(3)
                .log_exhausted_message_history(true)
                .log_exhausted_message_body(body);
            if !body {
                dlc = dlc.log_handled(true).log_exhausted(true);
            }
            r.error_handler(dlc);
            r.add(
                from("direct:start")
                    .log("Incoming ${body}")
                    .throw_exception(CamelError::illegal_argument("Forced")),
            );
        })
        .await;
        t.mock("mock:dead").expected_message_count(1);
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}
