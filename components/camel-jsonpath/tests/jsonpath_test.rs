//! Ports of the camel-jsonpath tests (the bodies read from files are their bytes here; the
//! POJO and Jackson ObjectMapper cases have no Rust equivalent).

use camel_core::api::context::CamelContext;
use camel_core::api::exchange_constants::SPLIT_SIZE;
use camel_core::model::expression::ExpressionDefinition;
use camel_core::prelude::*;
use camel_jsonpath::jsonpath;
use std::sync::Arc;

fn file(name: &str) -> Value {
    Value::Bytes(std::fs::read(format!("tests/resources/{name}")).unwrap())
}

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_jsonpath::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

fn mock(c: &Arc<DefaultCamelContext>, uri: &str) -> MockEndpoint {
    MockEndpoint::resolve(c.as_ref(), uri).unwrap()
}

fn jp(text: &str) -> ExpressionDefinition {
    ExpressionDefinition::language("jsonpath", text)
}

fn exchange(c: &Arc<DefaultCamelContext>, body: Value) -> Exchange {
    let shared: Arc<dyn CamelContext> = c.clone();
    let mut e = Exchange::new(shared);
    e.message_mut().set_body(body);
    e
}

/// `JsonPathLanguageTest`.
#[tokio::test]
async fn test_jsonpath_language() {
    let c = context(|_: &mut Routes| {}).await;
    let lan = c.resolve_language("jsonpath").unwrap();
    let e = exchange(&c, file("books.json"));
    let authors = lan
        .create_expression("$.store.book[*].author")
        .unwrap()
        .evaluate(&e)
        .unwrap();
    assert_eq!(
        Value::List(vec![
            "Nigel Rees".into(),
            "Evelyn Waugh".into(),
            "John O'Niel".into()
        ]),
        authors
    );
    assert_eq!(
        "19.95",
        lan.create_expression("$.store.bicycle.price")
            .unwrap()
            .evaluate(&e)
            .unwrap()
            .to_string()
    );
    let e2 = exchange(&c, file("type.json"));
    assert_eq!(
        "full",
        lan.create_expression("$.kind")
            .unwrap()
            .evaluate(&e2)
            .unwrap()
            .to_string()
    );
    assert_eq!(
        "customer",
        lan.create_expression("$.type")
            .unwrap()
            .evaluate(&e2)
            .unwrap()
            .to_string()
    );
    // a map
    let mut m = ValueMap::new();
    m.insert("kind".into(), "full".into());
    m.insert("type".into(), "customer".into());
    let e3 = exchange(&c, Value::Map(m));
    assert_eq!(
        "full",
        lan.create_expression("$.kind")
            .unwrap()
            .evaluate(&e3)
            .unwrap()
            .to_string()
    );
    // predicates
    assert!(lan
        .create_predicate("$.store.book[?(@.price < 10)]")
        .unwrap()
        .matches(&e)
        .unwrap());
    assert!(!lan
        .create_predicate("$.store.book[?(@.price > 30)]")
        .unwrap()
        .matches(&e)
        .unwrap());
    // suppress exceptions
    let ex = jsonpath("$.foo").suppress_exceptions(true).expression();
    let ExpressionDefinition::Expression(ex) = ex else {
        unreachable!()
    };
    assert!(ex.evaluate(&e2).unwrap().is_null());
    // unpack array and write as string
    let unpack = |unpack: bool, file_name: &str| {
        let ExpressionDefinition::Expression(x) = jsonpath("$.store.book")
            .write_as_string(true)
            .unpack_array(unpack)
            .expression()
        else {
            unreachable!()
        };
        x.evaluate(&exchange(&c, file(file_name)))
            .unwrap()
            .to_string()
    };
    let json = unpack(true, "expensive.json");
    assert!(json.starts_with('{') && json.ends_with('}'), "{json}");
    let json = unpack(false, "books.json");
    assert!(json.starts_with('[') && json.ends_with(']'), "{json}");
}

/// `JsonPathCBRTest` (the bean predicate with a `@JsonPath` parameter has no Rust equivalent).
#[tokio::test]
async fn test_jsonpath_cbr() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").choice(|ch| {
            ch.when(jsonpath("$.store.book[?(@.price < 10)]").predicate(), |w| {
                w.to("mock:cheap")
            })
            .when(jsonpath("$.store.book[?(@.price < 30)]").predicate(), |w| {
                w.to("mock:average")
            })
            .otherwise(|o| o.to("mock:expensive"))
        }));
        r.add(from("direct:bicycle2").choice(|ch| {
            ch.when(
                ExpressionDefinition::language(
                    "simple",
                    "${jsonpath($.store.bicycle.price)} < 100",
                ),
                |w| w.to("mock:cheap"),
            )
            .otherwise(|o| o.to("mock:expensive"))
        }));
    })
    .await;
    let t = c.create_producer_template();
    for (file_name, expected) in [
        ("cheap.json", "mock:cheap"),
        ("average.json", "mock:average"),
        ("expensive.json", "mock:expensive"),
    ] {
        for m in ["mock:cheap", "mock:average", "mock:expensive"] {
            let e = mock(&c, m);
            e.reset();
            e.expected_message_count(i32::from(m == expected));
        }
        t.send_body("direct:start", file(file_name)).await.unwrap();
        for m in ["mock:cheap", "mock:average", "mock:expensive"] {
            mock(&c, m).assert_is_satisfied().await;
        }
    } // the price of the bicycle compared in simple
    for m in ["mock:cheap", "mock:average", "mock:expensive"] {
        let e = mock(&c, m);
        e.reset();
        e.expected_message_count(i32::from(m == "mock:cheap"));
    }
    t.send_body("direct:bicycle2", file("cheap.json"))
        .await
        .unwrap();
    for m in ["mock:cheap", "mock:average", "mock:expensive"] {
        mock(&c, m).assert_is_satisfied().await;
    }
}

/// `JsonPathSplitTest`.
#[tokio::test]
async fn test_jsonpath_split() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").split(jp("$.store.book[*]"), |s| {
            s.to("mock:authors").convert_body_to("String")
        }));
        r.add(
            from("direct:start-1")
                .split(jsonpath("text.div").result_type("String"), |s| {
                    s.to("mock:result-1")
                })
                .delimiter("false"),
        );
        r.add(
            from("direct:start-2").split(jsonpath("text.div").result_type("String"), |s| {
                s.to("mock:result-2")
            }),
        );
        r.add(
            from("direct:start-3")
                .split(jsonpath("text.div").result_type("String"), |s| {
                    s.to("mock:result-3")
                })
                .delimiter("#"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let authors = mock(&c, "mock:authors");
    authors.expected_message_count(3);
    t.send_body("direct:start", file("books.json"))
        .await
        .unwrap();
    authors.assert_is_satisfied().await;
    let row = authors.received_exchanges()[0].message().body().clone();
    let Value::Map(m) = row else { panic!("a map") };
    assert_eq!(Some(&Value::from("Nigel Rees")), m.get("author"));
    assert_eq!(Some(&Value::Float(8.95)), m.get("price"));
    for (uri, json, size, bodies) in [
        (
            "direct:start-1",
            "{ \"text\": { \"div\": \"some , text\" } }",
            1,
            vec!["some , text"],
        ),
        (
            "direct:start-2",
            "{ \"text\": { \"div\": \"some , text\" } }",
            2,
            vec!["some ", " text"],
        ),
        (
            "direct:start-3",
            "{ \"text\": { \"div\": \"some ,# text\" } }",
            2,
            vec!["some ,", " text"],
        ),
    ] {
        let m = mock(&c, &uri.replace("direct:start", "mock:result"));
        m.expected_property_received(SPLIT_SIZE, size as i64);
        m.expected_bodies_received(bodies);
        t.send_body(uri, json).await.unwrap();
        m.assert_is_satisfied().await;
    }
}

/// `JsonPathTransformTest`.
#[tokio::test]
async fn test_jsonpath_transform() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(jp("$.store.book[*].author"))
                .to("mock:authors"),
        );
    })
    .await;
    let m = mock(&c, "mock:authors");
    m.expected_message_count(1);
    c.create_producer_template()
        .send_body("direct:start", file("books.json"))
        .await
        .unwrap();
    m.assert_is_satisfied().await;
    let Value::List(authors) = m.received_exchanges()[0].message().body().clone() else {
        panic!("a list")
    };
    assert_eq!(Value::from("Nigel Rees"), authors[0]);
    assert_eq!(Value::from("Evelyn Waugh"), authors[1]);
}
