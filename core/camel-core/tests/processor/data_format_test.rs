//! Ports of `MarshalVariableTest`, `UnmarshalVariableTest`, `MarshalAllowNullBodyTest`,
//! `UnmarshalProcessorTest` (the cases of a returned body, a null body, a null variable and an
//! inconvertible body; Rust data formats return the body only), `DataFormatServiceTest` and
//! `DeadLetterChannelUnmarshalSetHeaderTest`.

use crate::support::ContextTestSupport;
use camel_core::api::context::CamelContext;
use camel_core::api::spi::data_format::{
    data_format_fn, data_format_value, input_bytes, DataFormat, SharedDataFormat,
};
use camel_core::model::builder::custom_data_format;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

fn text(v: &Value) -> String {
    match v {
        Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        other => other.to_string(),
    }
}

/// A started context with the data format bound as `myDF`.
async fn with_df(
    df: SharedDataFormat,
    routes: impl Fn(&mut Routes) + Send + Sync + 'static,
) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("myDF", data_format_value(df))
        .unwrap();
    t.context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t
}

/// `MyByeDataFormat` of both variable tests: `Bye ` and the input.
fn bye() -> SharedDataFormat {
    Arc::new(data_format_fn(
        "myBye",
        |_, graph: &Value, out: &mut Vec<u8>| {
            out.extend(format!("Bye {graph}").into_bytes());
            Ok(())
        },
        |_, body: &Value| {
            Ok(Value::from(format!(
                "Bye {}",
                String::from_utf8_lossy(&input_bytes(body)?)
            )))
        },
    ))
}

fn variable_routes(marshal: bool) -> impl Fn(&mut Routes) + Send + Sync + 'static {
    move |r: &mut Routes| {
        let df = |route: camel_core::model::builder::RouteDsl| {
            if marshal {
                route.marshal(custom_data_format("myDF"))
            } else {
                route.unmarshal(custom_data_format("myDF"))
            }
        };
        r.add(
            df(from("direct:send")
                .set_variable("hello", simple("Camel"))
                .to("mock:before"))
            .variable_send("hello")
            .to("mock:result"),
        );
        r.add(
            df(from("direct:receive"))
                .variable_receive("bye")
                .to("mock:after")
                .set_body(simple("${variable:bye}"))
                .to("mock:result"),
        );
        r.add(
            df(from("direct:sendAndReceive")
                .set_variable("hello", simple("Camel"))
                .to("mock:before"))
            .variable_send("hello")
            .variable_receive("bye")
            .to("mock:result"),
        );
    }
}

#[tokio::test]
async fn test_marshal_unmarshal_variables() {
    for marshal in [true, false] {
        let t = with_df(bye(), variable_routes(marshal)).await;
        t.mock("mock:before").expected_bodies_received(["World"]);
        t.mock("mock:before")
            .expected_variable_received("hello", "Camel");
        t.mock("mock:result")
            .expected_bodies_received(["Bye Camel"]);
        t.mock("mock:result")
            .expected_variable_received("hello", "Camel");
        t.template.send_body("direct:send", "World").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;

        let t = with_df(bye(), variable_routes(marshal)).await;
        t.mock("mock:after").expected_bodies_received(["World"]);
        t.mock("mock:after")
            .expected_variable_received("bye", "Bye World");
        t.mock("mock:result")
            .expected_bodies_received(["Bye World"]);
        t.mock("mock:result")
            .expected_variable_received("bye", "Bye World");
        t.template
            .send_body("direct:receive", "World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;

        let t = with_df(bye(), variable_routes(marshal)).await;
        t.mock("mock:before").expected_bodies_received(["World"]);
        t.mock("mock:before")
            .expected_variable_received("hello", "Camel");
        t.mock("mock:result").expected_bodies_received(["World"]);
        t.mock("mock:result")
            .expected_variable_received("bye", "Bye Camel");
        t.template
            .send_body("direct:sendAndReceive", "World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// `MyNullDataFormat`: marshals like JSON does, a null body becomes the text null.
fn null_df() -> SharedDataFormat {
    Arc::new(data_format_fn(
        "myNull",
        |_, graph: &Value, out: &mut Vec<u8>| {
            let s = if graph.is_null() {
                "null".to_string()
            } else {
                graph.to_string()
            };
            out.extend(s.into_bytes());
            Ok(())
        },
        |_, _: &Value| Ok(Value::Null),
    ))
}

#[tokio::test]
async fn test_marshal_allow_null_body() {
    let routes = |r: &mut Routes| {
        r.add(from("direct:default").marshal(custom_data_format("myDF")));
        r.add(
            from("direct:allow")
                .marshal(custom_data_format("myDF"))
                .allow_null_body()
                .to("mock:allow"),
        );
        r.add(
            from("direct:receive")
                .set_variable("out", constant("stale"))
                .marshal(custom_data_format("myDF"))
                .allow_null_body()
                .variable_receive("out")
                .to("mock:receive"),
        );
    };
    let t = with_df(null_df(), routes).await;
    // a null body is marshalled by default
    assert_eq!(
        "null",
        text(
            &t.template
                .request_body("direct:default", Value::Null)
                .await
                .unwrap()
        )
    );
    // allowNullBody skips the marshalling, keeping the headers
    let mut e = t
        .template
        .create_exchange("direct:allow", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_header("foo", "bar");
    let out = t.template.send("direct:allow", e).await.unwrap();
    assert!(out.message().body().is_null());
    assert_eq!(
        Some("bar".to_string()),
        out.message().header("foo").map(|v| v.to_string())
    );
    // and marshals a body
    assert_eq!(
        "Camel",
        text(
            &t.template
                .request_body("direct:allow", "Camel")
                .await
                .unwrap()
        )
    );
    // the receive variable becomes null
    t.mock("mock:receive")
        .expected_variable_received("out", Value::Null);
    t.template
        .send_body("direct:receive", Value::Null)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// `UnmarshalProcessorTest`, through routes.
#[tokio::test]
async fn test_unmarshal_processor() {
    let object = || -> SharedDataFormat {
        Arc::new(data_format_fn(
            "MyDataFormat",
            |_, _: &Value, _: &mut Vec<u8>| {
                Err(CamelError::illegal_state(
                    "This method is not expected to be used by UnmarshalProcessor",
                ))
            },
            |_, body: &Value| {
                input_bytes(body)?;
                Ok(Value::from("unmarshalled"))
            },
        ))
    };
    let routes = |r: &mut Routes| {
        r.add(from("direct:start").unmarshal(custom_data_format("myDF")));
        r.add(
            from("direct:allow")
                .unmarshal(custom_data_format("myDF"))
                .allow_null_body(),
        );
        r.add(
            from("direct:variable")
                .unmarshal(custom_data_format("myDF"))
                .variable_send("order"),
        );
    };
    let t = with_df(object(), routes).await;
    // the returned body
    assert_eq!(
        "unmarshalled",
        t.template
            .request_body("direct:start", "body")
            .await
            .unwrap()
            .to_string()
    );
    // allowNullBody
    let out = t
        .template
        .request_body("direct:allow", Value::Null)
        .await
        .unwrap();
    assert!(out.is_null());
    // a null body says the body is null
    let e = t
        .template
        .request_body("direct:start", Value::Null)
        .await
        .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert_eq!(ErrorKind::NoTypeConversionAvailable, cause.kind());
    assert_eq!(
        Some("Cannot unmarshal using MyDataFormat: the message body is null, so there is nothing to unmarshal. Set the message body first, or set allowNullBody=true to skip a null body."),
        cause.message()
    );
    // a null variable says the variable is null
    let e = t
        .template
        .request_body("direct:variable", "body")
        .await
        .unwrap_err();
    assert_eq!(
        Some("Cannot unmarshal using MyDataFormat: the variable order is null, so there is nothing to unmarshal. Set the variable order first, or set allowNullBody=true to skip a null body."),
        crate::support::cause_of(&e).message()
    );
    // a body that cannot be converted keeps the converter message
    let e = t
        .template
        .request_body("direct:start", Value::List(vec![]))
        .await
        .unwrap_err();
    assert_eq!(
        Some("No type converter available to convert from type: List to the required type: java.io.InputStream"),
        crate::support::cause_of(&e).message()
    );
}

/// `DataFormatServiceTest`: the data format gets the context and is started and stopped with
/// the route.
#[tokio::test]
async fn test_data_format_service() {
    #[derive(Default)]
    struct My {
        started: AtomicBool,
        stopped: AtomicBool,
        context: Mutex<Option<Weak<dyn CamelContext>>>,
    }
    impl DataFormat for My {
        fn name(&self) -> &str {
            "my"
        }
        fn marshal(&self, _: &mut Exchange, _: &Value, out: &mut Vec<u8>) -> CamelResult<()> {
            out.extend(b"Hi Camel");
            Ok(())
        }
        fn unmarshal(&self, _: &mut Exchange, _: &Value) -> CamelResult<Value> {
            Ok(Value::from("Bye World"))
        }
        fn set_camel_context(&self, context: Weak<dyn CamelContext>) {
            *self.context.lock().unwrap() = Some(context);
        }
        fn start(&self) -> CamelResult<()> {
            self.started.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn stop(&self) -> CamelResult<()> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    let my = Arc::new(My::default());
    let df: SharedDataFormat = my.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:a").marshal(df.clone()).to("mock:a"));
        r.add(from("direct:b").unmarshal(df.clone()).to("mock:b"));
    })
    .await;
    assert!(my.started.load(Ordering::SeqCst));
    assert!(my
        .context
        .lock()
        .unwrap()
        .as_ref()
        .and_then(Weak::upgrade)
        .is_some());
    t.mock("mock:a").expected_bodies_received(["Hi Camel"]);
    t.mock("mock:b").expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("direct:b", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    t.context.stop().await.unwrap();
    assert!(my.stopped.load(Ordering::SeqCst));
}

/// `DeadLetterChannelUnmarshalSetHeaderTest`.
#[tokio::test]
async fn test_dead_letter_channel_unmarshal_set_header() {
    let df: SharedDataFormat = Arc::new(data_format_fn(
        "my",
        |_, _: &Value, _: &mut Vec<u8>| Ok(()),
        |_, _: &Value| Err(CamelError::illegal_argument("Damn")),
    ));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .error_handler(dead_letter_channel("direct:error"))
                .unmarshal(df.clone()),
        );
        r.add(
            from("direct:error")
                .set_header("foo", constant("123"))
                .set_header("bar", constant("456"))
                .to("mock:error"),
        );
    })
    .await;
    let mock = t.mock("mock:error");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_header_received("foo", "123");
    mock.expected_header_received("bar", "456");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
