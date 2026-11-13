//! Port of the camel-toon `ToonDataFormatTest` (the POJO case has no Rust equivalent).

use camel_core::api::exchange_constants::CONTENT_TYPE;
use camel_core::model::builder::data_format;
use camel_core::prelude::*;
use std::sync::Arc;

async fn context() -> Arc<DefaultCamelContext> {
    let c = camel_core::default_context();
    camel_toon::register(&c);
    c.add_routes(|r: &mut Routes| {
        r.add(from("direct:marshal").marshal("toon"));
        r.add(
            from("direct:marshalNull")
                .marshal(data_format("toon"))
                .allow_null_body(),
        );
        r.add(from("direct:unmarshal").unmarshal("toon"));
        r.add(from("direct:roundtrip").marshal("toon").unmarshal("toon"));
        r.add(from("direct:pipe").marshal(data_format("toon").option("delimiter", "PIPE")));
        r.add(
            from("direct:unmarshalPipe").unmarshal(data_format("toon").option("delimiter", "PIPE")),
        );
        r.add(from("direct:indent").marshal(data_format("toon").option("indent", 4)));
        r.add(
            from("direct:lengthMarker").marshal(data_format("toon").option("lengthMarker", true)),
        );
        r.add(
            from("direct:noContentType")
                .marshal(data_format("toon").option("contentTypeHeader", false)),
        );
        r.add(from("direct:strictFalse").unmarshal(data_format("toon").option("strict", false)));
    })
    .await
    .unwrap();
    c.start().await.unwrap();
    c
}

fn exchange(c: &Arc<DefaultCamelContext>, body: Value) -> Exchange {
    let shared: Arc<dyn camel_core::api::context::CamelContext> = c.clone();
    let mut e = Exchange::new(shared);
    e.message_mut().set_body(body);
    e
}

fn map(pairs: &[(&str, Value)]) -> Value {
    let mut m = ValueMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v.clone());
    }
    Value::Map(m)
}

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|s| Value::from(*s)).collect())
}

fn text(v: Value) -> String {
    match v {
        Value::Bytes(b) => String::from_utf8(b).unwrap(),
        other => other.to_string(),
    }
}

#[tokio::test]
async fn test_toon() {
    let c = context().await;
    let t = c.create_producer_template();
    let rt = |v: Value| {
        let t = c.create_producer_template();
        async move { t.request_body("direct:roundtrip", v).await.unwrap() }
    };
    // marshalAndUnmarshalMap
    let input = map(&[("id", Value::Int(1)), ("name", Value::from("Ada"))]);
    let toon = text(
        t.request_body("direct:marshal", input.clone())
            .await
            .unwrap(),
    );
    assert!(!toon.trim().is_empty());
    assert_eq!(
        input,
        t.request_body("direct:unmarshal", toon).await.unwrap()
    );
    // marshalAndUnmarshalList
    let colors = list(&["red", "green", "blue"]);
    let toon = text(
        t.request_body("direct:marshal", colors.clone())
            .await
            .unwrap(),
    );
    assert_eq!(
        colors,
        t.request_body("direct:unmarshal", toon).await.unwrap()
    );
    // marshalAndUnmarshalNestedObject
    let nested = map(&[
        ("name", Value::from("Ada")),
        (
            "address",
            map(&[("city", Value::from("London")), ("zip", Value::Int(12345))]),
        ),
    ]);
    assert_eq!(nested, rt(nested.clone()).await);
    // marshalAndUnmarshalPrimitives
    assert_eq!(Value::Bool(true), rt(Value::Bool(true)).await);
    assert_eq!(Value::Bool(false), rt(Value::Bool(false)).await);
    assert_eq!(Value::from("hello"), rt(Value::from("\"hello\"")).await);
    assert_eq!(Value::Int(42), rt(Value::Int(42)).await);
    assert_eq!(Value::Float(1.5), rt(Value::Float(1.5)).await);
    // marshalAndUnmarshalNull
    let toon = text(
        t.request_body("direct:marshalNull", Value::Null)
            .await
            .unwrap(),
    );
    assert_eq!("null", toon);
    assert_eq!(
        Value::Null,
        t.request_body("direct:unmarshal", toon).await.unwrap()
    );
    // marshalJsonStringAsDocument
    let toon = text(
        t.request_body("direct:marshal", "{\"id\":1,\"name\":\"Ada\"}")
            .await
            .unwrap(),
    );
    assert!(!toon.starts_with('"') && toon.contains("Ada"), "{toon}");
    assert_eq!(
        input,
        t.request_body("direct:unmarshal", toon).await.unwrap()
    );
    // roundTripPreservesSemanticValues
    let semantic = map(&[
        ("count", Value::Int(3)),
        ("active", Value::Bool(true)),
        ("tags", list(&["a", "b"])),
    ]);
    assert_eq!(semantic, rt(semantic.clone()).await);
    // marshalPrimitiveArray
    let ints = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(ints, rt(ints.clone()).await);
    // marshalUniformObjectArray
    let users = Value::List(vec![
        map(&[("id", Value::Int(1)), ("name", Value::from("Ada"))]),
        map(&[("id", Value::Int(2)), ("name", Value::from("Grace"))]),
    ]);
    assert_eq!(users, rt(users.clone()).await);
    // marshalEmptyArray
    assert_eq!(Value::List(vec![]), rt(Value::List(vec![])).await);
    // roundTripEscapedStrings
    let escaped = map(&[
        ("comma", Value::from("red,green")),
        ("tab", Value::from("left\tright")),
        ("pipe", Value::from("a|b")),
        ("quotes", Value::from("say \"hello\"")),
        ("newline", Value::from("line1\nline2")),
        ("unicode", Value::from("café 日本語")),
    ]);
    assert_eq!(escaped, rt(escaped.clone()).await);
    // delimiterOptionUsesPipe
    let toon = text(
        t.request_body("direct:pipe", list(&["a", "b", "c"]))
            .await
            .unwrap(),
    );
    assert!(toon.contains('|'), "{toon}");
    assert_eq!(
        list(&["a", "b", "c"]),
        t.request_body("direct:unmarshalPipe", toon).await.unwrap()
    );
    // indentOptionUsesFourSpaces
    let outer = map(&[("outer", map(&[("inner", Value::from("value"))]))]);
    assert!(text(t.request_body("direct:indent", outer).await.unwrap()).contains("    inner"));
    // lengthMarkerOptionPrefixesArrayLength
    assert!(text(
        t.request_body("direct:lengthMarker", list(&["a", "b"]))
            .await
            .unwrap()
    )
    .contains("[#"));
    // contentTypeHeaderIsSetByDefault and contentTypeHeaderCanBeDisabled
    let ok = map(&[("ok", Value::Bool(true))]);
    let e = t
        .send("direct:marshal", exchange(&c, ok.clone()))
        .await
        .unwrap();
    assert_eq!(
        Some(&Value::from("text/toon")),
        e.message().header(CONTENT_TYPE)
    );
    let e = t
        .send("direct:noContentType", exchange(&c, ok.clone()))
        .await
        .unwrap();
    assert_eq!(None, e.message().header(CONTENT_TYPE));
    // invalidJsonStringMarshalFails and emptyJsonStringMarshalFails
    for bad in ["not-json", ""] {
        let err = t.request_body("direct:marshal", bad).await.unwrap_err();
        assert!(
            err.find_cause(camel_core::api::error::ErrorKind::IllegalArgument)
                .is_some(),
            "{err}"
        );
    }
    let err = t
        .request_body("direct:marshal", "not-json")
        .await
        .unwrap_err();
    let cause = err
        .find_cause(camel_core::api::error::ErrorKind::IllegalArgument)
        .unwrap();
    assert!(
        cause.message().unwrap_or_default().contains("Invalid JSON"),
        "{cause}"
    );
    // malformedToonFailsInStrictMode
    let err = t
        .request_body("direct:unmarshal", "tags[2]: a,b,c")
        .await
        .unwrap_err();
    assert!(
        err.find_cause(camel_core::api::error::ErrorKind::IllegalArgument)
            .is_some(),
        "{err}"
    );
    // strictFalseUsesBestEffortParsing
    assert_eq!(
        map(&[("tags", list(&["a", "b", "c"]))]),
        t.request_body("direct:strictFalse", "tags[2]: a,b,c")
            .await
            .unwrap()
    );
    // emptyToonUnmarshalsToEmptyObject
    assert_eq!(
        Value::Map(ValueMap::new()),
        t.request_body("direct:unmarshal", "").await.unwrap()
    );
}
