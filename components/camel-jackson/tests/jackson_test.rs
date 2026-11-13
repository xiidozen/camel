//! Ports of the camel-jackson tests on maps, lists and texts: `JacksonDataFormatTest`,
//! `JacksonMarshalJsonTextTest` (but the Java `GenericFile` body), `JacksonMarshalTest` (but
//! the POJO case), `JacksonMarshalContentTypeHeaderTest`; the POJO tests have no Rust
//! equivalent.

use camel_core::api::exchange_constants::CONTENT_TYPE;
use camel_core::api::spi::data_format::{DataFormat, SharedDataFormat};
use camel_core::prelude::*;
use camel_jackson::JacksonDataFormat;
use std::sync::Arc;

fn text(v: &Value) -> String {
    match v {
        Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        other => other.to_string(),
    }
}

fn map(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    )
}

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_jackson::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

#[test]
fn test_jackson_data_format() {
    let mut e = Exchange::new(camel_core::api::context::StandaloneContext::shared());
    let df = JacksonDataFormat::new();
    for (json, expected) in [
        ("\"A string\"", Value::from("A string")),
        ("{\"value\":123}", map(&[("value", Value::Int(123))])),
        (
            "[{\"value\":123}]",
            Value::List(vec![map(&[("value", Value::Int(123))])]),
        ),
    ] {
        assert_eq!(expected, df.unmarshal(&mut e, &Value::from(json)).unwrap());
    }
}

#[tokio::test]
async fn test_marshal_json_text() {
    let json = "{\"sku\": \"CAMEL-MUG\", \"qty\": 2}";
    let c = context(|r: &mut Routes| {
        r.add(from("direct:marshal").marshal("jackson"));
    })
    .await;
    let t = c.create_producer_template();
    let marshal = |body: Value| {
        let t = &t;
        async move { text(&t.request_body("direct:marshal", body).await.unwrap()) }
    };
    // a text with a JSON object or array is written as is
    assert_eq!(json, marshal(Value::from(json)).await);
    assert_eq!(
        format!("[{json}]"),
        marshal(Value::from(format!("[{json}]"))).await
    );
    assert_eq!(
        format!("  {json}\n"),
        marshal(Value::from(format!("  {json}\n"))).await
    );
    // any other text is a JSON string
    assert_eq!("\"hello\"", marshal(Value::from("hello")).await);
    assert_eq!("\"{not closed\"", marshal(Value::from("{not closed")).await);
    // bytes are written as is
    assert_eq!(json, marshal(Value::Bytes(json.as_bytes().to_vec())).await);
    // objects
    assert_eq!(
        "{\"sku\":\"CAMEL-MUG\"}",
        marshal(map(&[("sku", Value::from("CAMEL-MUG"))])).await
    );
}

#[tokio::test]
async fn test_marshal_and_unmarshal_map() {
    let pretty: SharedDataFormat = Arc::new(JacksonDataFormat::new().with_pretty_print(true));
    let c = context(move |r: &mut Routes| {
        r.add(from("direct:in").marshal("jackson"));
        r.add(from("direct:back").unmarshal("jackson").to("mock:reverse"));
        r.add(from("direct:inPretty").marshal(pretty.clone()));
        r.add(
            from("direct:backPretty")
                .unmarshal(pretty.clone())
                .to("mock:reverse"),
        );
        r.add(
            from("direct:nullBody")
                .unmarshal("jackson")
                .allow_null_body()
                .to("mock:nullBody"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let input = map(&[("name", Value::from("Camel"))]);
    for (uri, back, expected) in [
        (
            "direct:in",
            "direct:back",
            "{\"name\":\"Camel\"}".to_string(),
        ),
        (
            "direct:inPretty",
            "direct:backPretty",
            "{\n  \"name\" : \"Camel\"\n}".to_string(),
        ),
    ] {
        let mock = MockEndpoint::resolve(c.as_ref(), "mock:reverse").unwrap();
        mock.reset();
        mock.expected_message_count(1);
        mock.expected_bodies_received([input.clone()]);
        let marshalled = t.request_body(uri, input.clone()).await.unwrap();
        assert_eq!(expected, text(&marshalled));
        t.send_body(back, marshalled).await.unwrap();
        mock.assert_is_satisfied().await;
    }
    let null = MockEndpoint::resolve(c.as_ref(), "mock:nullBody").unwrap();
    null.expected_message_count(1);
    null.message_matches(0, body().is_null());
    t.send_body("direct:nullBody", Value::Null).await.unwrap();
    null.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_marshal_content_type_header() {
    let no_header: SharedDataFormat =
        Arc::new(JacksonDataFormat::new().with_content_type_header(false));
    let c = context(move |r: &mut Routes| {
        r.add(from("direct:yes").marshal("jackson"));
        r.add(from("direct:yes2").marshal("json"));
        r.add(from("direct:no").marshal(no_header.clone()));
    })
    .await;
    let t = c.create_producer_template();
    for (uri, expected) in [
        ("direct:yes", Some("application/json")),
        ("direct:yes2", Some("application/json")),
        ("direct:no", None),
    ] {
        let mut e = t.create_exchange(uri, ExchangePattern::InOut).unwrap();
        e.message_mut()
            .set_body(map(&[("name", Value::from("Camel"))]));
        let out = t.send(uri, e).await.unwrap();
        assert_eq!(
            expected.map(str::to_string),
            out.message().header(CONTENT_TYPE).map(|v| v.to_string()),
            "{uri}"
        );
    }
}

#[test]
fn test_jackson_pretty() {
    let v = camel_support_json(r#"{"a":[1,{"b":null}],"c":{},"d":[]}"#);
    assert_eq!(
        "{\n  \"a\" : [ 1, {\n    \"b\" : null\n  } ],\n  \"c\" : { },\n  \"d\" : [ ]\n}",
        camel_jackson::jackson_pretty(&v)
    );
}

fn camel_support_json(text: &str) -> Value {
    let df = JacksonDataFormat::new();
    let mut e = Exchange::new(camel_core::api::context::StandaloneContext::shared());
    df.unmarshal(&mut e, &Value::from(text)).unwrap()
}
