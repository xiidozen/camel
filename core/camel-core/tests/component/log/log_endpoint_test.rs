//! Port of `LogEndpointTest` (`testLogEndpoint` used a custom Java `CamelLogProcessor` subclass;
//! the group size endpoint is configured by uri here).

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
use camel_core::api::spi::log_listener::LogListener;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

/// Java `TestLogListener`: keeps the last logged message.
#[derive(Default)]
struct TestLogListener(Mutex<Option<String>>);

impl LogListener for TestLogListener {
    fn on_log(&self, _exchange: &Exchange, _logger_name: &str, message: &str) -> String {
        *self.0.lock().unwrap() = Some(message.to_string());
        message.to_string()
    }
}

fn with_listener() -> (ContextTestSupport, Arc<TestLogListener>) {
    let t = ContextTestSupport::new();
    let listener = Arc::new(TestLogListener::default());
    t.context.add_log_listener(listener.clone());
    (t, listener)
}

async fn send(t: &ContextTestSupport, uri: &str, f: impl FnOnce(&mut Exchange)) {
    t.context.start().await.unwrap();
    let mut e = t
        .template
        .create_exchange(uri, ExchangePattern::InOnly)
        .unwrap();
    f(&mut e);
    t.template.send(uri, e).await.unwrap();
}

#[tokio::test]
async fn test_log_endpoint_group_size() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start2")
                .to("log:loggerSetter?groupSize=10")
                .to("mock:result"),
        );
    })
    .await;
    let out = t.mock("mock:result");
    out.expected_message_count(50);
    for _ in 0..50 {
        t.template.send_body("direct:start2", "blub").await.unwrap();
    }
    out.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_show_caught_exception() {
    let (t, logged) = with_listener();
    send(
        &t,
        "log:testShowCaughtException?showCaughtException=true",
        |e| {
            e.set_property(
                EXCEPTION_CAUGHT,
                camel_core::processor::try_catch::caught_value(&CamelError::new(
                    ErrorKind::Runtime,
                    "test",
                )),
            );
        },
    )
    .await;
    assert_eq!(
        Some("Exchange[BodyType: null, Body: [Body is null], CaughtExceptionType: java.lang.RuntimeException, CaughtExceptionMessage: test]".to_string()),
        logged.0.lock().unwrap().clone()
    );
}

#[tokio::test]
async fn test_show_exception() {
    let (t, logged) = with_listener();
    send(&t, "log:testShowException?showException=true", |e| {
        e.set_exception(Some(CamelError::new(ErrorKind::Runtime, "test")));
    })
    .await;
    assert_eq!(
        Some("Exchange[BodyType: null, Body: [Body is null], ExceptionType: java.lang.RuntimeException, ExceptionMessage: test]".to_string()),
        logged.0.lock().unwrap().clone()
    );
}

#[tokio::test]
async fn test_plain() {
    let (t, logged) = with_listener();
    send(&t, "log:info?plain=true", |e| {
        e.message_mut().set_body("Body as string")
    })
    .await;
    assert_eq!(
        Some("Body as string".to_string()),
        logged.0.lock().unwrap().clone()
    );
}
