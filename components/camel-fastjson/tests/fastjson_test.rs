//! Ports of the camel-fastjson tests (maps for the POJOs).

use camel_core::api::spi::data_format::{DataFormat, SharedDataFormat};
use camel_core::model::builder::data_format;
use camel_core::prelude::*;
use camel_fastjson::FastjsonDataFormat;
use std::sync::Arc;

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let c = camel_core::default_context();
    camel_fastjson::register(&c);
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

/// `FastjsonMarshalTest` and `JSONFastjsonDataFormatTEST` (the map cases).
#[tokio::test]
async fn test_marshal_and_unmarshal_map() {
    for json_library in [false, true] {
        let c = context(move |r: &mut Routes| {
            if json_library {
                r.add(from("direct:in").marshal(data_format("json").option("library", "Fastjson")));
                r.add(
                    from("direct:back")
                        .unmarshal(data_format("json").option("library", "Fastjson"))
                        .to("mock:reverse"),
                );
                r.add(
                    from("direct:json").unmarshal(
                        data_format("json")
                            .option("library", "Fastjson")
                            .option("unmarshalType", "java.util.Map"),
                    ),
                );
            } else {
                let df: SharedDataFormat = Arc::new(FastjsonDataFormat::new());
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

/// `FastjsonMarshalJsonTextTest`.
#[tokio::test]
async fn test_marshal_json_text() {
    let json = "{\"sku\": \"CAMEL-MUG\", \"qty\": 2}";
    let c = context(|r: &mut Routes| {
        r.add(from("direct:marshal").marshal("fastjson"));
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

/// The options: serializeNulls, prettyPrint and the content headers.
#[tokio::test]
async fn test_options() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:nulls").marshal(data_format("fastjson").option("serializeNulls", true)));
        r.add(from("direct:pretty").marshal(data_format("fastjson").option("prettyPrint", true)));
        r.add(from("direct:plain").marshal(data_format("fastjson")));
    })
    .await;
    let t = c.create_producer_template();
    let input = map(&[("name", Value::from("Camel")), ("none", Value::Null)]);
    assert_eq!(
        "{\"name\":\"Camel\",\"none\":null}",
        text(t.request_body("direct:nulls", input.clone()).await.unwrap())
    );
    assert_eq!(
        "{\n\t\"name\":\"Camel\"\n}",
        text(
            t.request_body("direct:pretty", input.clone())
                .await
                .unwrap()
        )
    );
    let mut e = Exchange::new(c.clone() as Arc<dyn camel_core::api::context::CamelContext>);
    e.message_mut().set_body(input);
    let e = t.send("direct:plain", e).await.unwrap();
    assert_eq!(
        Some(&Value::from("application/json")),
        e.message().header("Content-Type")
    );
    assert_eq!(Some(&Value::Int(16)), e.message().header("Content-Length"));
    let _ = FastjsonDataFormat::new().unmarshal(
        &mut Exchange::new(c.clone() as Arc<dyn camel_core::api::context::CamelContext>),
        &Value::from("{}"),
    );
}
