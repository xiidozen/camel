//! Ports of `WireTapLogEndpointTest`, `WireTapOnPrepareTest`, `WireTapOnPrepareRefTest`,
//! `WireTapNoErrorHandlerTest`, `WireTapVariableTest` and `WireTapUsingMulticastTest`.

use crate::support::ContextTestSupport;
use camel_core::api::processor::{processor_fn, SharedProcessor};
use camel_core::prelude::*;
use std::sync::Arc;

#[tokio::test]
async fn test_wire_tap_log() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .wire_tap("log:com.foo.MyApp?level=WARN")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

fn animal(name: &str) -> Value {
    let mut m = ValueMap::new();
    m.insert("id".into(), Value::from(1));
    m.insert("name".into(), Value::from(name));
    Value::Map(m)
}

fn deep_clone() -> SharedProcessor {
    Arc::new(processor_fn(|e: &mut Exchange| {
        let copy = e.message().body().clone();
        e.message_mut().set_body(copy);
        Ok(())
    }))
}

/// Ports of `WireTapOnPrepareTest` and `WireTapOnPrepareRefTest` (each copy has its own body
/// in Rust; the prepare processor runs on the copy).
#[tokio::test]
async fn test_wire_tap_on_prepare() {
    for by_ref in [false, true] {
        let t = ContextTestSupport::new();
        if by_ref {
            t.context
                .simple_registry()
                .bind(
                    "deepClone",
                    Value::Object(camel_core::util::value::ObjectRef::new(deep_clone())),
                )
                .unwrap();
        }
        t.context
            .add_routes(move |r: &mut Routes| {
                let tap = from("direct:start").wire_tap("direct:a");
                let tap = if by_ref {
                    tap.on_prepare_ref("deepClone")
                } else {
                    tap.on_prepare(deep_clone())
                };
                r.add(tap.to("direct:b"));
                r.add(
                    from("direct:a")
                        .process_fn(|e| {
                            assert_eq!(&animal("Tiger"), e.message().body());
                            e.message_mut().set_body(animal("Tony the Tiger"));
                            Ok(())
                        })
                        .to("mock:a"),
                );
                r.add(
                    from("direct:b")
                        .delay(constant(100))
                        .process_fn(|e| {
                            assert_eq!(&animal("Tiger"), e.message().body());
                            Ok(())
                        })
                        .to("mock:b"),
                );
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        t.mock("mock:a").expected_message_count(1);
        t.mock("mock:a")
            .message_matches(0, body().is_equal_to(animal("Tony the Tiger")));
        t.mock("mock:b").expected_message_count(1);
        t.mock("mock:b")
            .message_matches(0, body().is_equal_to(animal("Tiger")));
        t.template
            .send_body("direct:start", animal("Tiger"))
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_wire_tap_no_error_handler() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .error_handler(no_error_handler())
                .to("log:foo")
                .wire_tap("direct:tap")
                .to("mock:result"),
        );
        r.add(
            from("direct:tap")
                .delay(constant(100))
                .set_body(constant("Tapped"))
                .to("mock:result")
                .to("mock:tap"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World", "Tapped"]);
    t.mock("mock:tap").expected_bodies_received(["Tapped"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_wire_tap_variable() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:send")
                .set_variable("hello", simple("Camel"))
                .to("mock:before")
                .wire_tap_variable("direct:foo", "hello")
                .to("mock:result"),
        );
        r.add(
            from("direct:foo")
                .transform(simple("Bye ${body}"))
                .to("mock:tap"),
        );
    })
    .await;
    t.mock("mock:before").expected_bodies_received(["World"]);
    t.mock("mock:before")
        .expected_variable_received("hello", "Camel");
    t.mock("mock:tap").expected_bodies_received(["Bye Camel"]);
    t.mock("mock:tap")
        .expected_variable_received("hello", "Camel");
    t.mock("mock:result").expected_bodies_received(["World"]);
    t.mock("mock:result")
        .expected_variable_received("hello", "Camel");
    t.template.send_body("direct:send", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_wire_tap_using_multicast() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").multicast(|m| m.to("mock:tap").to("mock:result")));
    })
    .await;
    for m in ["mock:tap", "mock:result"] {
        t.mock(m).expected_bodies_received(["<body/>"]);
        t.mock(m).expected_message_count(1);
    }
    t.template
        .send_body("direct:start", "<body/>")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
