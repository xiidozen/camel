//! Ports of the camel-jsonb tests (maps for the POJOs).

use camel_core::api::spi::data_format::{DataFormat, SharedDataFormat};
use camel_core::model::builder::data_format;
use camel_core::prelude::*;
use camel_jsonb::JsonbDataFormat;
use std::sync::Arc;

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let c = camel_core::default_context();
    camel_jsonb::register(&c);
    c.add_routes(move |r: &mut Routes| routes(r)).await.unwrap();
    c.start().await.unwrap();
    c
}

fn map(pairs: &[(&str, Value)]) -> Value {
    let mut m = ValueMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v.clone());
    }
    Value::Map(m)
}

fn text(v: Value) -> String {
    match v {
        Value::Bytes(b) => String::from_utf8(b).unwrap(),
        other => other.to_string(),
    }
}

/// `JsonbMarshalTest` and `JSONJsonbDataFormatTEST` (the map cases).
#[tokio::test]
async fn test_marshal_and_unmarshal_map() {
    for json_library in [false, true] {
        let c = context(move |r: &mut Routes| {
            if json_library {
                r.add(from("direct:in").marshal(data_format("json").option("library", "Jsonb")));
                r.add(
                    from("direct:back")
                        .unmarshal(data_format("json").option("library", "Jsonb"))
                        .to("mock:reverse"),
                );
                r.add(
                    from("direct:json").unmarshal(
                        data_format("json")
                            .option("library", "Jsonb")
                            .option("unmarshalType", "java.util.Map"),
                    ),
                );
            } else {
                let df: SharedDataFormat = Arc::new(JsonbDataFormat::new());
                r.add(from("direct:in").marshal(df.clone()));
                r.add(from("direct:back").unmarshal(df).to("mock:reverse"));
            }
        })
        .await;
        let t = c.create_producer_template();
        let input = map(&[("name", Value::from("Camel"))]);
        let m = MockEndpoint::resolve(c.as_ref(), "mock:reverse").unwrap();
        m.expected_bodies_received([input.clone()]);
        let marshalled = t.request_body("direct:in", input).await.unwrap();
        assert_eq!("{\"name\":\"Camel\"}", text(marshalled.clone()));
        t.send_body("direct:back", marshalled).await.unwrap();
        m.assert_is_satisfied().await;
        if json_library {
            let v = t
                .request_body(
                    "direct:json",
                    "{\"pointsOfSale\":{\"pointOfSale\":{\"prodcut\":\"newpad\"}}}",
                )
                .await
                .unwrap();
            let pos = &v.as_map().unwrap()["pointsOfSale"].as_map().unwrap()["pointOfSale"];
            assert_eq!(
                Some(&Value::from("newpad")),
                pos.as_map().unwrap().get("prodcut"),
                "Don't get the right value"
            );
            assert!(t.request_body("direct:json", "[1]").await.is_err());
        }
    }
}

/// `JsonbMarshalJsonTextTest`.
#[tokio::test]
async fn test_marshal_json_text() {
    let json = "{\"sku\": \"CAMEL-MUG\", \"qty\": 2}";
    let c = context(|r: &mut Routes| {
        r.add(from("direct:marshal").marshal("jsonb"));
    })
    .await;
    let marshal = |body: Value| {
        let t = c.create_producer_template();
        async move { text(t.request_body("direct:marshal", body).await.unwrap()) }
    };
    assert_eq!(json, marshal(Value::from(json)).await);
    assert_eq!(
        format!("[{json}]"),
        marshal(Value::from(format!("[{json}]"))).await
    );
    assert_eq!(
        format!("  {json}\n"),
        marshal(Value::from(format!("  {json}\n"))).await
    );
    assert_eq!("\"hello\"", marshal(Value::from("hello")).await);
    assert_eq!("\"{not closed\"", marshal(Value::from("{not closed")).await);
    assert_eq!(json, marshal(Value::Bytes(json.as_bytes().to_vec())).await);
    assert_eq!(
        "{\"sku\":\"CAMEL-MUG\"}",
        marshal(map(&[("sku", Value::from("CAMEL-MUG"))])).await
    );
}

/// `JsonbDataFormatTest`, `JsonbSkipNullTest` and `JsonbAttributeOrderTest` (maps).
#[test]
fn test_unmarshal_and_options() {
    let c: Arc<dyn camel_core::api::context::CamelContext> = camel_core::default_context();
    let unmarshal = |df: &JsonbDataFormat, json: &str| {
        let mut e = Exchange::new(c.clone());
        df.unmarshal(&mut e, &Value::Bytes(json.as_bytes().to_vec()))
            .unwrap()
    };
    let string = JsonbDataFormat::new().with_unmarshal_type(camel_jsonb::UnmarshalType::String);
    assert_eq!(Value::from("A string"), unmarshal(&string, "\"A string\""));
    let maps = JsonbDataFormat::new().with_unmarshal_type(camel_jsonb::UnmarshalType::Map);
    assert_eq!(
        map(&[("value", Value::Int(123))]),
        unmarshal(&maps, "{\"value\":123}")
    );
    let lists = JsonbDataFormat::new().with_unmarshal_type(camel_jsonb::UnmarshalType::List);
    assert_eq!(
        Value::List(vec![map(&[("value", Value::Int(123))])]),
        unmarshal(&lists, "[{\"value\":123}]")
    );
    let mut e = Exchange::new(c.clone());
    e.message_mut()
        .set_header(camel_jsonb::UNMARSHAL_TYPE, "java.util.List");
    assert!(JsonbDataFormat::new()
        .unmarshal(&mut e, &Value::from("{}"))
        .is_err());
    let pojo = map(&[("name", Value::from("Camel")), ("country", Value::Null)]);
    assert_eq!(
        "{\"name\":\"Camel\"}",
        JsonbDataFormat::new().to_json(&pojo)
    );
    assert_eq!(
        "{\"name\":\"Camel\",\"country\":null}",
        JsonbDataFormat::new().with_skip_null(false).to_json(&pojo)
    );
    let numbers = map(&[
        ("longNumber", Value::Int(123)),
        ("intNumber", Value::Int(123)),
        ("floatNumber", Value::Float(123.0)),
        ("doubleNumber", Value::Float(123.123)),
        ("bool", Value::Bool(true)),
        ("bg", Value::Float(123.123)),
    ]);
    let lexicographical =
        JsonbDataFormat::new().with_property_order(camel_jsonb::PropertyOrder::Lexicographical);
    assert_eq!(
        "{\"bg\":123.123,\"bool\":true,\"doubleNumber\":123.123,\"floatNumber\":123.0,\"intNumber\":123,\"longNumber\":123}",
        lexicographical.to_json(&numbers)
    );
}
