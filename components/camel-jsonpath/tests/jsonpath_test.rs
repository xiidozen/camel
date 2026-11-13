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

/// `JsonPathSuppressTest`: a missing path is null with suppressExceptions.
#[tokio::test]
async fn test_jsonpath_suppress() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").choice(|ch| {
            ch.when(
                jsonpath("person.middlename")
                    .suppress_exceptions(true)
                    .predicate(),
                |w| w.to("mock:middle"),
            )
            .otherwise(|o| o.to("mock:other"))
        }));
    })
    .await;
    let t = c.create_producer_template();
    for (json, middle) in [
        ("{\"person\" : {\"firstname\" : \"foo\", \"middlename\" : \"foo2\", \"lastname\" : \"bar\"}}", true),
        ("{\"person\" : {\"firstname\" : \"foo\", \"lastname\" : \"bar\"}}", false),
    ] {
        mock(&c, "mock:middle").reset();
        mock(&c, "mock:other").reset();
        mock(&c, "mock:middle").expected_message_count(i32::from(middle));
        mock(&c, "mock:other").expected_message_count(i32::from(!middle));
        t.send_body("direct:start", json).await.unwrap();
        mock(&c, "mock:middle").assert_is_satisfied().await;
        mock(&c, "mock:other").assert_is_satisfied().await;
    }
}

/// `JsonPathHeaderNameTest`: the JSON of a header, an int result.
#[tokio::test]
async fn test_jsonpath_header_name() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header(
                    "number",
                    jsonpath("$..store.book.length()")
                        .result_type("int")
                        .source("header:myHeader"),
                )
                .to("mock:result"),
        );
    })
    .await;
    let m = mock(&c, "mock:result");
    m.expected_bodies_received(["Hello World"]);
    m.expected_header_received("number", 3);
    c.create_producer_template()
        .send_body_and_header(
            "direct:start",
            "Hello World",
            "myHeader",
            file("books.json"),
        )
        .await
        .unwrap();
    m.assert_is_satisfied().await;
}

/// `JsonPathLeafNullTest`: a missing leaf is null with DEFAULT_PATH_LEAF_TO_NULL.
#[tokio::test]
async fn test_jsonpath_leaf_null() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header(
                    "summary",
                    jsonpath("$.summary").path_options(camel_jsonpath::path::PathOptions {
                        default_path_leaf_to_null: true,
                        ..Default::default()
                    }),
                )
                .to("mock:result"),
        );
    })
    .await;
    let m = mock(&c, "mock:result");
    m.expected_message_count(1);
    m.message_matches(0, header("summary").is_null());
    c.create_producer_template()
        .send_body(
            "direct:start",
            "{\n    \"errorcode\": 0,\n    \"errormessage\": \"\"\n}",
        )
        .await
        .unwrap();
    m.assert_is_satisfied().await;
}

/// `JsonPathExchangePropertyTest` and `JsonPathVariableTest`.
#[tokio::test]
async fn test_jsonpath_property_and_variable() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_property(
                    "foo",
                    jsonpath("$.store.bicycle.color").result_type("String"),
                )
                .log("${exchangeProperty.foo}")
                .to("mock:color"),
        );
        r.add(
            from("direct:variable")
                .set_variable("cars", constant("[\"Ford\", \"BMW\", \"Fiat\"]"))
                .set_body(simple("${jsonpath(variable:cars , $.length())}"))
                .to("mock:result"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let color = mock(&c, "mock:color");
    color.expected_message_count(1);
    t.send_body("direct:start", file("books.json"))
        .await
        .unwrap();
    color.assert_is_satisfied().await;
    assert_eq!(
        Some(&Value::from("red")),
        color.received_exchanges()[0].property("foo")
    );
    let result = mock(&c, "mock:result");
    result.expected_bodies_received([3]);
    t.send_body("direct:variable", "{}").await.unwrap();
    result.assert_is_satisfied().await;
}

/// `JsonPathSplitSingleListTest`: a definite path as a list of one element.
#[tokio::test]
async fn test_jsonpath_split_single_list() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").split(
            jsonpath("$.store.book[0]").result_type("java.util.List"),
            |s| s.to("mock:authors").convert_body_to("String"),
        ));
    })
    .await;
    let m = mock(&c, "mock:authors");
    m.expected_message_count(1);
    c.create_producer_template()
        .send_body("direct:start", file("books.json"))
        .await
        .unwrap();
    m.assert_is_satisfied().await;
    let Value::Map(row) = m.received_exchanges()[0].message().body().clone() else {
        panic!("a map")
    };
    assert_eq!(Some(&Value::from("Nigel Rees")), row.get("author"));
    assert_eq!(Some(&Value::Float(8.95)), row.get("price"));
}

/// `JsonPathWithSimpleCBRTest`: simple functions in the path.
#[tokio::test]
async fn test_jsonpath_with_simple_cbr() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").choice(|ch| {
            ch.when(
                jsonpath("$.store.book[?(@.price < ${header.cheap})]").predicate(),
                |w| w.to("mock:cheap"),
            )
            .when(
                jsonpath("$.store.book[?(@.price < ${header.average})]").predicate(),
                |w| w.to("mock:average"),
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
        let mut e = t
            .create_exchange("direct:start", ExchangePattern::InOnly)
            .unwrap();
        e.message_mut().set_header("cheap", 10);
        e.message_mut().set_header("average", 30);
        e.message_mut().set_body(file(file_name));
        t.send("direct:start", e).await.unwrap();
        for m in ["mock:cheap", "mock:average", "mock:expensive"] {
            mock(&c, m).assert_is_satisfied().await;
        }
    }
}

/// `JsonPathComparisonHintTest`.
#[test]
fn test_jsonpath_comparison_hint() {
    use camel_jsonpath::comparison_hint;
    let hint = comparison_hint("$.status == 'paid'").unwrap();
    assert!(hint.contains("$[?(@.status == 'paid')]"), "{hint}");
    assert!(hint.contains("${body[status]} == 'paid'"), "{hint}");
    let hint = comparison_hint("$.store.book.price < 10").unwrap();
    assert!(hint.contains("$[?(@.store.book.price < 10)]"), "{hint}");
    assert!(!hint.contains("${body["), "{hint}");
    assert!(comparison_hint("$.status").is_none());
    assert!(comparison_hint("$.store.book[?(@.price < 10)]").is_none());
}

/// The message of the root cause of the error.
fn root_message(e: &CamelError) -> String {
    let mut cause: &dyn std::error::Error = e;
    while let Some(c) = cause.source() {
        cause = c;
    }
    cause.to_string()
}

/// `JsonPathNullBodyHintTest`.
#[tokio::test]
async fn test_jsonpath_null_body_hint() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:body").transform(jp("$.foo")).to("mock:result"));
        r.add(
            from("direct:variable")
                .transform(jsonpath("$.foo").source("variable:input"))
                .to("mock:result"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let e = t.send_body("direct:body", Value::Null).await.unwrap_err();
    let message = format!("{} {}", e, root_message(&e));
    for part in [
        "The jsonpath expression got no message body to evaluate (the body is null)",
        "constant: resource:file:",
        "poll:",
        "has the body of its caller",
    ] {
        assert!(message.contains(part), "{message}");
    }
    let e = t
        .send_body("direct:variable", "{ \"foo\": \"bar\" }")
        .await
        .unwrap_err();
    assert!(
        format!("{e} {}", root_message(&e)).contains("No 'input' variable available"),
        "{e}"
    );
    let e = t.send_body("direct:body", 42).await.unwrap_err();
    let message = format!("{} {}", e, root_message(&e));
    assert!(
        message.contains("Cannot read body as supported JSON value"),
        "{message}"
    );
    assert!(
        message.contains("the body has type: java.lang.Integer"),
        "{message}"
    );
}

/// `JsonPathNullHeaderTest` and `JsonPathChoicePrimitiveTest`.
#[tokio::test]
async fn test_jsonpath_null_header_and_primitive_choice() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header("foo", jp("$.foo"))
                .to("mock:foo"),
        );
        r.add(from("direct:counter").choice(|ch| {
            ch.when(jsonpath("$[?(@.counter>0)]").predicate(), |w| {
                w.to("mock:positive")
            })
            .otherwise(|o| o.to("mock:zero"))
        }));
    })
    .await;
    let t = c.create_producer_template();
    let foo = mock(&c, "mock:foo");
    foo.expected_header_received("foo", "cheese");
    t.send_body("direct:start", "{\n  \"foo\": \"cheese\"\n}\n")
        .await
        .unwrap();
    foo.assert_is_satisfied().await;
    foo.reset();
    foo.expected_header_received("foo", Value::Null);
    t.send_body("direct:start", "{\n  \"foo\": null\n}\n")
        .await
        .unwrap();
    foo.assert_is_satisfied().await;
    mock(&c, "mock:positive").expected_bodies_received(["{\"counter\": 1}", "{\"counter\": 2}"]);
    mock(&c, "mock:zero").expected_bodies_received(["{\"counter\": 0}"]);
    for b in ["{\"counter\": 1}", "{\"counter\": 0}", "{\"counter\": 2}"] {
        t.send_body("direct:counter", b).await.unwrap();
    }
    mock(&c, "mock:positive").assert_is_satisfied().await;
    mock(&c, "mock:zero").assert_is_satisfied().await;
}
