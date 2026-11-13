//! Ports of the camel-gson tests (maps and lists of values for the POJOs).

use camel_core::api::spi::data_format::{DataFormat, SharedDataFormat};
use camel_core::model::builder::data_format;
use camel_core::prelude::*;
use camel_gson::{GsonDataFormat, UnmarshalType};
use std::sync::Arc;

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let c = camel_core::default_context();
    camel_gson::register(&c);
    c.add_routes(move |r: &mut Routes| routes(r)).await.unwrap();
    c.start().await.unwrap();
    c
}

fn mock(c: &Arc<DefaultCamelContext>, uri: &str) -> MockEndpoint {
    MockEndpoint::resolve(c.as_ref(), uri).unwrap()
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

/// `GsonMarshalTest` and `GsonJsonDataFormatTest` (the map cases).
#[tokio::test]
async fn test_marshal_and_unmarshal_map() {
    for json_library in [false, true] {
        let c = context(move |r: &mut Routes| {
            if json_library {
                r.add(from("direct:in").marshal(data_format("json").option("library", "Gson")));
                r.add(
                    from("direct:back")
                        .unmarshal(data_format("json").option("library", "Gson"))
                        .to("mock:reverse"),
                );
                r.add(
                    from("direct:json").unmarshal(
                        data_format("json")
                            .option("library", "Gson")
                            .option("unmarshalType", "java.util.Map"),
                    ),
                );
            } else {
                let df: SharedDataFormat = Arc::new(GsonDataFormat::new());
                r.add(from("direct:in").marshal(df.clone()));
                r.add(from("direct:back").unmarshal(df).to("mock:reverse"));
            }
        })
        .await;
        let t = c.create_producer_template();
        let input = map(&[("name", Value::from("Camel"))]);
        let m = mock(&c, "mock:reverse");
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

/// `GsonMarshalJsonTextTest`.
#[tokio::test]
async fn test_marshal_json_text() {
    let json = "{\"sku\": \"CAMEL-MUG\", \"qty\": 2}";
    let c = context(|r: &mut Routes| {
        r.add(from("direct:marshal").marshal("gson"));
    })
    .await;
    let t = c.create_producer_template();
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
    // the content type header
    let e = Exchange::new(c.clone() as Arc<dyn camel_core::api::context::CamelContext>);
    let mut e = e;
    e.message_mut().set_body(map(&[("a", Value::Int(1))]));
    let e = t.send("direct:marshal", e).await.unwrap();
    assert_eq!(
        Some(&Value::from("application/json")),
        e.message().header("Content-Type")
    );
}

/// `GsonDataFormatTest`: the lenient reading, the numbers as doubles.
#[test]
fn test_unmarshal_values() {
    let c: Arc<dyn camel_core::api::context::CamelContext> = camel_core::default_context();
    let df = GsonDataFormat::new();
    let unmarshal = |json: &str| {
        let mut e = Exchange::new(c.clone());
        df.unmarshal(&mut e, &Value::from(json)).unwrap()
    };
    assert_eq!(Value::from("A string"), unmarshal("\"A string\""));
    assert_eq!(
        map(&[("value", Value::Float(123.0))]),
        unmarshal("{value=123}")
    );
    assert_eq!(
        Value::List(vec![map(&[("value", Value::Float(123.0))])]),
        unmarshal("[{value=123}]")
    );
    // the unmarshal type of the header
    let mut e = Exchange::new(c.clone());
    e.message_mut()
        .set_header(camel_gson::UNMARSHAL_TYPE, "java.util.List");
    assert!(df.unmarshal(&mut e, &Value::from("{\"a\":1}")).is_err());
    let s = GsonDataFormat::new().with_unmarshal_type(UnmarshalType::String);
    let mut e = Exchange::new(c.clone());
    assert_eq!(
        Value::from("5.0"),
        s.unmarshal(&mut e, &Value::from("5")).unwrap()
    );
}

/// `GsonConcurrentTest` (maps for the POJOs).
#[tokio::test]
async fn test_concurrent() {
    for (files, pool) in [(1usize, 1usize), (10, 5)] {
        let c = context(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .marshal(data_format("json").option("library", "Gson"))
                    .to("log:marshalled")
                    .to("direct:marshalled"),
            );
            r.add(
                from("direct:marshalled")
                    .unmarshal(data_format("json").option("library", "Gson"))
                    .to("mock:result"),
            );
        })
        .await;
        let m = mock(&c, "mock:result");
        m.expected_message_count(files as i32);
        let mut tasks = Vec::new();
        for chunk in (0..files).collect::<Vec<_>>().chunks(files.div_ceil(pool)) {
            let chunk = chunk.to_vec();
            let t = c.create_producer_template();
            tasks.push(tokio::spawn(async move {
                for i in chunk {
                    t.send_body(
                        "direct:start",
                        map(&[("name", Value::from(format!("Hi {i}")))]),
                    )
                    .await
                    .unwrap();
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        m.assert_is_satisfied().await;
        // no duplicates
        let mut bodies: Vec<String> = m
            .received_exchanges()
            .iter()
            .map(|e| e.message().body().to_string())
            .collect();
        bodies.sort();
        bodies.dedup();
        assert_eq!(files, bodies.len());
        c.stop().await.unwrap();
    }
}

/// The options of the Spring XML data formats (`SpringGsonJsonDataFormatTest`, with maps).
#[tokio::test]
async fn test_pretty_print_and_nulls() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:inPretty").marshal(
                data_format("json")
                    .option("library", "Gson")
                    .option("prettyPrint", true),
            ),
        );
        r.add(from("direct:nulls").marshal(data_format("gson").option("serializeNulls", true)));
        r.add(
            from("direct:longs")
                .marshal(data_format("gson").option("longSerializationPolicy", "STRING")),
        );
    })
    .await;
    let t = c.create_producer_template();
    let input = map(&[("name", Value::from("Camel")), ("none", Value::Null)]);
    assert_eq!(
        "{\n  \"name\": \"Camel\"\n}",
        text(
            t.request_body("direct:inPretty", input.clone())
                .await
                .unwrap()
        )
    );
    assert_eq!(
        "{\"name\":\"Camel\",\"none\":null}",
        text(t.request_body("direct:nulls", input).await.unwrap())
    );
    assert_eq!(
        "{\"id\":\"123\"}",
        text(
            t.request_body("direct:longs", map(&[("id", Value::Int(123))]))
                .await
                .unwrap()
        )
    );
}
