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

/// `JsonPathMockMatchesTest`: jsonpath predicates on the messages of a mock.
#[tokio::test]
async fn test_jsonpath_mock_matches() {
    for (order, satisfied) in [([0, 2, 1], true), ([0, 1, 2], false)] {
        let c = context(|r: &mut Routes| {
            r.add(from("direct:start").to("mock:result"));
        })
        .await;
        let m = mock(&c, "mock:result");
        m.expected_message_count(3);
        let predicates = [
            "$.store.book[?(@.price < 10)]",
            "$.store.book[?(@.price < 30)]",
            "$.store.book[?(@.price > 30)]",
        ];
        for (i, p) in order.iter().zip(predicates) {
            let camel_core::model::expression::ExpressionDefinition::Predicate(p) =
                jsonpath(p).predicate()
            else {
                unreachable!()
            };
            m.message_matches(*i, p);
        }
        let t = c.create_producer_template();
        for f in ["cheap.json", "expensive.json", "average.json"] {
            t.send_body("direct:start", file(f)).await.unwrap();
        }
        assert_eq!(satisfied, m.try_assert_is_satisfied().await.is_ok());
    }
}

/// `JsonPathTransformONielTest`, `JsonPathTransformONielEscapedTest` and
/// `JsonPathTransformONielPlaceholderTest`: quotes in the filter.
#[tokio::test]
async fn test_jsonpath_transform_oniel() {
    for (exp, expected) in [
        (
            "$.store.book[?(@.author == \"John O'Niel\")].title",
            vec!["Camels in Space"],
        ),
        (
            "$.store.book[?(@.author == 'John O\\'Niel')].title",
            vec!["Camels in Space"],
        ),
        (
            "$.store.book[?(@.author == '{{who}}' || @.title == '{{search}}')].title",
            vec!["Sword's of Honour", "Camels in Space"],
        ),
    ] {
        let context = camel_core::default_context();
        camel_jsonpath::register(&context);
        context
            .properties()
            .add_initial_property("who", "John O'Niel");
        context
            .properties()
            .add_initial_property("search", "Sword's of Honour");
        context
            .add_routes(move |r: &mut Routes| {
                r.add(from("direct:start").transform(jp(exp)).to("mock:authors"));
            })
            .await
            .unwrap();
        context.start().await.unwrap();
        let m = mock(&context, "mock:authors");
        m.expected_message_count(1);
        context
            .create_producer_template()
            .send_body("direct:start", file("books.json"))
            .await
            .unwrap();
        m.assert_is_satisfied().await;
        let expected = Value::List(expected.into_iter().map(Value::from).collect());
        assert_eq!(
            expected,
            m.received_exchanges()[0].message().body().clone(),
            "{exp}"
        );
    }
}

/// `JsonPathSimpleTransformTest`, `JsonPathSimpleSetVariableTest` and
/// `JsonPathSimpleResultTypeTest`: jsonpath in simple templates.
#[tokio::test]
async fn test_jsonpath_simple() {
    let mapping = "{\n  \"id\": \"${jsonpath($.id)}\",\n  \"type\": \"${header.type}\",\n  \"amount\": ${jsonpath($.amount)},\n  \"status\": \"${iif(${jsonpath($.amount)} > 100,HIGH,LOW)}\"\n}\n";
    let mapping2 = "${setVariable(myVar,${jsonpath($.amount)})}\n{\n  \"id\": \"${jsonpath($.id)}\",\n  \"type\": \"${header.type}\",\n  \"amount\": ${jsonpath($.amount)},\n  \"status\": \"${iif(${variable.myVar} > 100,HIGH,LOW)}\"\n}\n";
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:transform")
                .transform(simple("{\n  \"roll\": ${jsonpath($.id)},\n  \"years\": ${jsonpath($.age)},\n  \"fullname\": \"${jsonpath($.name)}\"\n}"))
                .to("mock:result"),
        );
        r.add(from("direct:start").transform(simple(mapping)).log("${body}").to("mock:order"));
        r.add(from("direct:start2").transform(simple(mapping2)).transform(simple("${trim()}")).log("${body}").to("mock:order"));
        r.add(
            from("direct:types")
                .set_header("age", simple("${int:jsonpath($.age)}"))
                .set_header("id", simple("${long:jsonpath($.id)}"))
                .set_header("name", simple("${string:jsonpath($.name)}"))
                .set_header("vip", simple("${boolean:jsonpath($.vip)}"))
                .to("mock:types"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let result = mock(&c, "mock:result");
    result.expected_bodies_received([
        "{\n  \"roll\": 123,\n  \"years\": 42,\n  \"fullname\": \"scott\"\n}",
    ]);
    t.send_body(
        "direct:transform",
        "{\"id\": 123, \"age\": 42, \"name\": \"scott\"}",
    )
    .await
    .unwrap();
    result.assert_is_satisfied().await;
    let expected = "{\n  \"id\": \"123\",\n  \"type\": \"silver\",\n  \"amount\": 444,\n  \"status\": \"HIGH\"\n}";
    for uri in ["direct:start", "direct:start2"] {
        let order = mock(&c, "mock:order");
        order.reset();
        order.expected_bodies_received([expected]);
        t.send_body_and_header(
            uri,
            "{\n  \"id\": 123,\n  \"amount\": 444\n}\n",
            "type",
            "silver",
        )
        .await
        .unwrap();
        order.assert_is_satisfied().await;
    }
    let types = mock(&c, "mock:types");
    types.expected_header_received("age", 42);
    types.expected_header_received("id", 1234567890i64);
    types.expected_header_received("name", "scott");
    types.expected_header_received("vip", true);
    t.send_body(
        "direct:types",
        "{\"id\": \"1234567890\", \"age\": \"42\", \"name\": \"scott\", \"vip\": \"true\"}",
    )
    .await
    .unwrap();
    types.assert_is_satisfied().await;
    let h = types.received_exchanges()[0].message().headers().clone();
    assert!(matches!(h.get("vip"), Some(Value::Bool(true))));
    assert!(matches!(h.get("age"), Some(Value::Int(42))));
}

/// `JsonPathSplitMultipleListTest` and `JsonPathSplitSingleListOptionTest`.
#[tokio::test]
async fn test_jsonpath_split_lists() {
    let always_list = camel_jsonpath::path::PathOptions {
        always_return_list: true,
        ..Default::default()
    };
    let c = context(move |r: &mut Routes| {
        r.add(from("direct:start2").split(
            jsonpath("$.store.book").result_type("java.util.List"),
            |s| s.to("mock:authors2").convert_body_to("String"),
        ));
        r.add(from("direct:start3").split(
            jsonpath("$.store.book[*]").result_type("java.util.List"),
            |s| s.to("mock:authors3").convert_body_to("String"),
        ));
        r.add(
            from("direct:option")
                .split(jsonpath("$.store.book[0]").path_options(always_list), |s| {
                    s.to("mock:authors").convert_body_to("String")
                }),
        );
    })
    .await;
    let t = c.create_producer_template();
    mock(&c, "mock:authors2").expected_message_count(3);
    mock(&c, "mock:authors3").expected_message_count(3);
    mock(&c, "mock:authors").expected_message_count(1);
    for uri in ["direct:start2", "direct:start3", "direct:option"] {
        t.send_body(uri, file("books.json")).await.unwrap();
    }
    for m in ["mock:authors2", "mock:authors3", "mock:authors"] {
        mock(&c, m).assert_is_satisfied().await;
    }
    let Value::Map(row) = mock(&c, "mock:authors").received_exchanges()[0]
        .message()
        .body()
        .clone()
    else {
        panic!("a map")
    };
    assert_eq!(Some(&Value::from("Nigel Rees")), row.get("author"));
}

/// `JsonPathSplitWriteAsStringTest`, `JsonPathSplitWriteAsStringMapTest`,
/// `JsonPathSplitWriteAsStringResultTypeStringTest` and `JsonPathWriteAsStringObjectTest`.
#[tokio::test]
async fn test_jsonpath_write_as_string() {
    let httpbin = "{\n  \"args\": {\n    \"age\": 30,\n    \"name\": \"Alice\"\n  },\n  \"headers\": {\n    \"Host\": \"httpbin.org\",\n    \"Accept\": \"application/json\"\n  },\n  \"origin\": \"178.227.111.11\",\n  \"url\": \"https://httpbin.org/get?name=Alice&age=30\"\n}\n";
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start").split(jsonpath("$.content").write_as_string(true), |s| {
                s.to("mock:line").to("log:line")
            }),
        );
        r.add(
            from("direct:map").split(jsonpath("$.content.*").write_as_string(true), |s| {
                s.to("mock:lineMap")
            }),
        );
        r.add(
            from("direct:one")
                .set_body(
                    jsonpath("$.content[?(@.id == 456)]")
                        .write_as_string(true)
                        .result_type("String"),
                )
                .to("mock:result"),
        );
        r.add(
            from("direct:header")
                .set_header("NewBody", jsonpath("$.args").write_as_string(true))
                .to("mock:header"),
        );
        r.add(
            from("direct:body")
                .set_body(jsonpath("$.args").write_as_string(true))
                .to("mock:body"),
        );
        r.add(
            from("direct:nested")
                .set_body(jsonpath("$.headers").write_as_string(true))
                .to("mock:nested"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let lines = [
        "{\"action\":\"CU\",\"id\":123,\"modifiedTime\":\"2015-07-28T11:40:09.520+02:00\"}",
        "{\"action\":\"CU\",\"id\":456,\"modifiedTime\":\"2015-07-28T11:42:29.510+02:00\"}",
    ];
    for (uri, m, f) in [
        ("direct:start", "mock:line", "content.json"),
        ("direct:map", "mock:lineMap", "content-map.json"),
    ] {
        let e = mock(&c, m);
        e.expected_bodies_received(lines);
        t.send_body(uri, file(f)).await.unwrap();
        e.assert_is_satisfied().await;
    }
    let result = mock(&c, "mock:result");
    result.expected_bodies_received([lines[1]]);
    t.send_body("direct:one", file("content.json"))
        .await
        .unwrap();
    result.assert_is_satisfied().await;
    let json = |text: &str| camel_support_parse(text);
    t.send_body("direct:header", httpbin).await.unwrap();
    let header = mock(&c, "mock:header").received_exchanges()[0]
        .message()
        .header("NewBody")
        .unwrap()
        .to_string();
    assert_eq!(json("{\"age\":30,\"name\":\"Alice\"}"), json(&header));
    t.send_body("direct:body", httpbin).await.unwrap();
    let body = mock(&c, "mock:body").received_exchanges()[0]
        .message()
        .body()
        .to_string();
    assert_eq!(json("{\"age\":30,\"name\":\"Alice\"}"), json(&body));
    assert!(
        body.starts_with('{') && body.ends_with('}') && !body.contains('='),
        "{body}"
    );
    t.send_body("direct:nested", httpbin).await.unwrap();
    let nested = mock(&c, "mock:nested").received_exchanges()[0]
        .message()
        .body()
        .to_string();
    assert_eq!(
        json("{\"Host\":\"httpbin.org\",\"Accept\":\"application/json\"}"),
        json(&nested)
    );
}

fn camel_support_parse(text: &str) -> Value {
    let lan = camel_jsonpath::JsonPathLanguage::new();
    let e = Exchange::new(camel_core::api::context::StandaloneContext::shared());
    let mut e = e;
    e.message_mut().set_body(text);
    camel_core::api::spi::language::Language::create_expression(&lan, "$")
        .unwrap()
        .evaluate(&e)
        .unwrap()
}

/// `JsonPathTransformResultTypeTest` (Rust has one floating point type).
#[tokio::test]
async fn test_jsonpath_transform_result_type() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(jsonpath("$.store.book[0].price").result_type("Float"))
                .to("mock:price"),
        );
        r.add(
            from("direct:second")
                .transform(jsonpath("$.store.book[0].price").result_type("Double"))
                .to("mock:price"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let m = mock(&c, "mock:price");
    m.expected_message_count(4);
    for uri in [
        "direct:start",
        "direct:second",
        "direct:start",
        "direct:second",
    ] {
        t.send_body(uri, file("books.json")).await.unwrap();
    }
    m.assert_is_satisfied().await;
    for e in m.received_exchanges() {
        assert!(
            matches!(e.message().body(), Value::Float(f) if (*f - 8.95).abs() < 1e-6),
            "{}",
            e.message().body()
        );
    }
}

/// `JsonPathCharsetTest`: UTF-16 detected, ISO-8859-1 from the header.
#[tokio::test]
async fn test_jsonpath_charset() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(jp("$.store.book[*].title"))
                .to("mock:authors"),
        );
    })
    .await;
    let t = c.create_producer_template();
    for (f, encoding, titles) in [
        (
            "booksUTF16BE.json",
            None,
            ["Sayings of the Century", "Sword of Honour"],
        ),
        (
            "booksUTF16LE.json",
            None,
            ["Sayings of the Century", "Sword of Honour"],
        ),
        (
            "germanbooks-iso-8859-1.json",
            Some("ISO-8859-1"),
            ["Joseph und seine Brüder", "Götzendämmerung"],
        ),
    ] {
        let m = mock(&c, "mock:authors");
        m.reset();
        m.expected_message_count(1);
        match encoding {
            Some(enc) => t
                .send_body_and_header(
                    "direct:start",
                    file(f),
                    camel_jsonpath::HEADER_JSON_ENCODING,
                    enc,
                )
                .await
                .unwrap(),
            None => t.send_body("direct:start", file(f)).await.unwrap(),
        };
        m.assert_is_satisfied().await;
        let Value::List(l) = m.received_exchanges()[0].message().body().clone() else {
            panic!("a list")
        };
        assert_eq!(Value::from(titles[0]), l[0], "{f}");
        assert_eq!(Value::from(titles[1]), l[1], "{f}");
    }
}

/// `JsonPathSourceTest`: files consumed with their charset.
#[tokio::test]
async fn test_jsonpath_source_file_charset() {
    for (f, charset) in [
        ("germanbooks-utf8.json", "UTF-8"),
        ("germanbooks-iso-8859-1.json", "ISO-8859-1"),
    ] {
        let c = context(move |r: &mut Routes| {
            r.add(
                from(&format!(
                    "file:tests/resources?fileName={f}&noop=true&initialDelay=0&charset={charset}"
                ))
                .multicast(|m| {
                    m.pipeline(|p| {
                        p.transform(jsonpath("$.store.book[0].title").result_type("String"))
                            .to("mock:title0")
                    })
                    .pipeline(|p| {
                        p.transform(jsonpath("$.store.book[1].title").result_type("String"))
                            .to("mock:title1")
                    })
                }),
            );
        })
        .await;
        mock(&c, "mock:title0").expected_bodies_received(["Joseph und seine Br\u{00fc}der"]);
        mock(&c, "mock:title1").expected_bodies_received(["G\u{00f6}tzend\u{00e4}mmerung"]);
        mock(&c, "mock:title0").assert_is_satisfied().await;
        mock(&c, "mock:title1").assert_is_satisfied().await;
        c.stop().await.unwrap();
    }
}

/// `JsonPathSimpleInitBlockTest`, `JsonPathSimpleInitBlockFunctionTest` and
/// `JsonPathSimpleTransformTwoTest`.
#[tokio::test]
async fn test_jsonpath_simple_init_block() {
    let mapping = "$init{\n  $id := ${jsonpath($.id)};\n  $type := ${header.type};\n  $price := ${jsonpath($.amount)};\n  $level := ${iif(${jsonpath($.amount)} > 100,HIGH,LOW)};\n}init$\n{\n  \"id\": \"$id\",\n  \"type\": \"$type\",\n  \"amount\": $price,\n  \"status\": \"$level\"\n}\n";
    let mapping2 = "$init{\n  $id := ${jsonpath($.id)};\n  $type := ${header.type};\n  $price := ${jsonpath($.amount)};\n  $level := ${iif(${jsonpath($.amount)} > 100,HIGH,LOW)};\n}init$\n{\n  \"id\": \"$id\",\n  \"type\": \"$type\",\n  \"amount\": ${jsonpath($.amount)},\n  \"status\": \"${lowercase($level)}\"\n}\n";
    let mapping3 = "$init{\n  $id := ${jsonpath($.id)};\n  $type := ${header.type};\n  $price := ${jsonpath($.amount)};\n  $level ~:= ${body > 100 ? 'HIGH' : 'LOW'};\n  $newStatus ~:= ${sum(${body},50)};\n}init$\n{\n  \"id\": \"$id\",\n  \"type\": \"$type\",\n  \"amount\": $price,\n  \"oldStatus\": $price ~> $level() ~> ${safeQuote()}\n  \"status\": ${newStatus($price)} ~> $level() ~> ${safeQuote()}\n}\n";
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(simple(mapping))
                .log("${body}")
                .to("mock:order"),
        );
        r.add(
            from("direct:start2")
                .transform(simple(mapping2))
                .log("${body}")
                .to("mock:order2"),
        );
        r.add(
            from("direct:start3")
                .transform(simple(mapping3))
                .log("${body}")
                .to("mock:order3"),
        );
        r.add(
            from("direct:two")
                .transform(simple("${jsonpath($.id)}"))
                .to("mock:result"),
        );
    })
    .await;
    let t = c.create_producer_template();
    for (uri, m, body, header, expected) in [
        ("direct:start", "mock:order", "{\n  \"id\": 123,\n  \"amount\": 44\n}\n", "silver", "{\n  \"id\": \"123\",\n  \"type\": \"silver\",\n  \"amount\": 44,\n  \"status\": \"LOW\"\n}"),
        ("direct:start2", "mock:order2", "{\n  \"id\": 456,\n  \"amount\": 888\n}\n", "gold", "{\n  \"id\": \"456\",\n  \"type\": \"gold\",\n  \"amount\": 888,\n  \"status\": \"high\"\n}"),
        ("direct:start3", "mock:order3", "{\n  \"id\": 123,\n  \"amount\": 78\n}\n", "silver", "{\n  \"id\": \"123\",\n  \"type\": \"silver\",\n  \"amount\": 78,\n  \"oldStatus\": \"LOW\"\n  \"status\": \"HIGH\"\n}"),
    ] {
        let e = mock(&c, m);
        e.expected_bodies_received([expected]);
        t.send_body_and_header(uri, body, "type", header).await.unwrap();
        e.assert_is_satisfied().await;
    }
    let r = mock(&c, "mock:result");
    r.expected_bodies_received(["123"]);
    t.send_body("direct:two", "{\"id\": 123}").await.unwrap();
    r.assert_is_satisfied().await;
}

/// `JsonPathTransformHeaderNameTest`.
#[tokio::test]
async fn test_jsonpath_transform_header_name() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(jsonpath("$.store.book[*].author").source("header:myHeader"))
                .to("mock:authors"),
        );
    })
    .await;
    let m = mock(&c, "mock:authors");
    m.expected_message_count(1);
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
    let Value::List(l) = m.received_exchanges()[0].message().body().clone() else {
        panic!("a list")
    };
    assert_eq!(Value::from("Nigel Rees"), l[0]);
    assert_eq!(Value::from("Evelyn Waugh"), l[1]);
}

/// The Spring XML tests in the XML DSL: `SpringJsonPathCBRTest`, `SpringJsonPathSuppressTest`,
/// `SpringJsonPathSplitWriteAsStringTest`, `SpringJsonPathTransformHeaderNameTest`,
/// `SpringJsonPathTransformResultTypeTest` and `SpringJsonPathTransformTest`.
#[tokio::test]
async fn test_spring_jsonpath() {
    let xml = r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route><from uri="direct:cbr"/><choice>
    <when><jsonpath>$.store.book[?(@.price &lt; 10)]</jsonpath><to uri="mock:cheap"/></when>
    <when><jsonpath>$.store.book[?(@.price &lt; 30)]</jsonpath><to uri="mock:average"/></when>
    <otherwise><to uri="mock:expensive"/></otherwise></choice></route>
  <route><from uri="direct:suppress"/><choice>
    <when><jsonpath suppressExceptions="true">person.middlename</jsonpath><to uri="mock:middle"/></when>
    <otherwise><to uri="mock:other"/></otherwise></choice></route>
  <route><from uri="direct:split"/><split><jsonpath writeAsString="true">$.content</jsonpath><to uri="mock:line"/><to uri="log:line"/></split></route>
  <route><from uri="direct:header"/><transform><jsonpath source="header:myHeader">$.store.book[*].author</jsonpath></transform><to uri="mock:authors"/></route>
  <route><from uri="direct:price"/><transform><jsonpath resultType="Double">$.store.book[0].price</jsonpath></transform><to uri="mock:price"/></route>
  <route><from uri="direct:transform"/><transform><jsonpath>$.store.book[*].author</jsonpath></transform><to uri="mock:transform"/></route>
</routes>"#;
    let c = camel_core::default_context();
    camel_jsonpath::register(&c);
    c.add_routes(camel_xml_io_dsl::XmlRoutes::parse(xml).unwrap())
        .await
        .unwrap();
    c.start().await.unwrap();
    let t = c.create_producer_template();
    mock(&c, "mock:cheap").expected_message_count(1);
    mock(&c, "mock:average").expected_message_count(1);
    mock(&c, "mock:expensive").expected_message_count(1);
    for f in ["cheap.json", "average.json", "expensive.json"] {
        t.send_body("direct:cbr", file(f)).await.unwrap();
    }
    mock(&c, "mock:middle").expected_message_count(1);
    mock(&c, "mock:other").expected_message_count(1);
    t.send_body("direct:suppress", "{\"person\" : {\"firstname\" : \"foo\", \"middlename\" : \"foo2\", \"lastname\" : \"bar\"}}").await.unwrap();
    t.send_body(
        "direct:suppress",
        "{\"person\" : {\"firstname\" : \"foo\", \"lastname\" : \"bar\"}}",
    )
    .await
    .unwrap();
    mock(&c, "mock:line").expected_bodies_received([
        "{\"action\":\"CU\",\"id\":123,\"modifiedTime\":\"2015-07-28T11:40:09.520+02:00\"}",
        "{\"action\":\"CU\",\"id\":456,\"modifiedTime\":\"2015-07-28T11:42:29.510+02:00\"}",
    ]);
    t.send_body("direct:split", file("content.json"))
        .await
        .unwrap();
    t.send_body_and_header(
        "direct:header",
        "Hello World",
        "myHeader",
        file("books.json"),
    )
    .await
    .unwrap();
    t.send_body("direct:price", file("books.json"))
        .await
        .unwrap();
    t.send_body("direct:transform", file("books.json"))
        .await
        .unwrap();
    for m in [
        "mock:cheap",
        "mock:average",
        "mock:expensive",
        "mock:middle",
        "mock:other",
        "mock:line",
    ] {
        mock(&c, m).assert_is_satisfied().await;
    }
    for m in ["mock:authors", "mock:transform"] {
        let Value::List(l) = mock(&c, m).received_exchanges()[0].message().body().clone() else {
            panic!("a list")
        };
        assert_eq!(Value::from("Nigel Rees"), l[0], "{m}");
    }
    assert!(matches!(
        mock(&c, "mock:price").received_exchanges()[0]
            .message()
            .body(),
        Value::Float(_)
    ));
}

/// `JsonPathMapTransformTest`: the JSON already parsed into a map.
#[tokio::test]
async fn test_jsonpath_map_transform() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(jp("$.store.book[*].author"))
                .to("mock:authors"),
        );
    })
    .await;
    let document = camel_support_parse(
        &String::from_utf8(std::fs::read("tests/resources/books.json").unwrap()).unwrap(),
    );
    assert!(matches!(document, Value::Map(_)));
    let m = mock(&c, "mock:authors");
    m.expected_message_count(1);
    c.create_producer_template()
        .send_body("direct:start", document)
        .await
        .unwrap();
    m.assert_is_satisfied().await;
    let Value::List(l) = m.received_exchanges()[0].message().body().clone() else {
        panic!("a list")
    };
    assert_eq!(Value::from("Nigel Rees"), l[0]);
    assert_eq!(Value::from("Evelyn Waugh"), l[1]);
}

/// `JsonPathPredicateJsonSmartTest`: a negated existence filter.
#[tokio::test]
async fn test_jsonpath_predicate_negated_filter() {
    let c = context(|_: &mut Routes| {}).await;
    let p = c
        .resolve_language("jsonpath")
        .unwrap()
        .create_predicate("$.messages[?(!@.bot_id)]")
        .unwrap();
    assert!(
        p.matches(&exchange(&c, file("messages.json"))).unwrap(),
        "Should have message from bot"
    );
}

/// `JsonPathChoiceReaderTest` and `JsonPathStreamCachingCBRTest` (readers and streams are
/// texts and bytes in Rust; the bean parameter case has no Rust equivalent).
#[tokio::test]
async fn test_jsonpath_choice_text_and_bytes() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").choice(|ch| {
            ch.when(jsonpath("$[?(@.counter>0)]").predicate(), |w| {
                w.to("mock:positive")
            })
            .otherwise(|o| o.to("mock:zero"))
        }));
        r.add(from("direct:cbr").choice(|ch| {
            ch.when(jsonpath("$.store.book[?(@.price < 10)]").predicate(), |w| {
                w.to("mock:cheap")
            })
            .when(jsonpath("$.store.book[?(@.price < 30)]").predicate(), |w| {
                w.to("mock:average")
            })
            .otherwise(|o| o.to("mock:expensive"))
        }));
    })
    .await;
    let t = c.create_producer_template();
    mock(&c, "mock:positive").expected_bodies_received(["{\"counter\": 1}", "{\"counter\": 2}"]);
    mock(&c, "mock:zero").expected_bodies_received(["{\"counter\": 0}"]);
    for b in ["{\"counter\": 1}", "{\"counter\": 0}", "{\"counter\": 2}"] {
        t.send_body("direct:start", Value::Bytes(b.as_bytes().to_vec()))
            .await
            .unwrap();
    }
    mock(&c, "mock:positive").assert_is_satisfied().await;
    mock(&c, "mock:zero").assert_is_satisfied().await;
    for m in ["mock:cheap", "mock:average", "mock:expensive"] {
        mock(&c, m).expected_message_count(1);
    }
    for f in ["cheap.json", "average.json", "expensive.json"] {
        t.send_body("direct:cbr", file(f)).await.unwrap();
    }
    for m in ["mock:cheap", "mock:average", "mock:expensive"] {
        mock(&c, m).assert_is_satisfied().await;
    }
}

/// `JsonPathEngineWriteAsStringTest`.
#[tokio::test]
async fn test_jsonpath_engine_write_as_string() {
    let json = "{\n  \"args\": {\n    \"age\": 30,\n    \"name\": \"Alice\"\n  },\n  \"content\": [\n    {\"id\": 1, \"value\": \"first\"},\n    {\"id\": 2, \"value\": \"second\"}\n  ]\n}\n";
    let c = context(|_: &mut Routes| {}).await;
    let e = exchange(&c, Value::from(json));
    let eval = |b: camel_jsonpath::JsonPathBuilder| {
        let ExpressionDefinition::Expression(x) = b.expression() else {
            unreachable!()
        };
        x.evaluate(&e).unwrap()
    };
    let as_string = eval(jsonpath("$.args").write_as_string(true));
    let Value::Text(t) = &as_string else {
        panic!("a text: {as_string}")
    };
    assert_eq!(
        camel_support_parse("{\"age\":30,\"name\":\"Alice\"}"),
        camel_support_parse(t)
    );
    let Value::Map(m) = eval(jsonpath("$.args")) else {
        panic!("a map")
    };
    assert_eq!(Some(&Value::Int(30)), m.get("age"));
    assert_eq!(Some(&Value::from("Alice")), m.get("name"));
    let Value::List(rows) = eval(jsonpath("$.content[*]").write_as_string(true)) else {
        panic!("a list")
    };
    assert_eq!(2, rows.len());
    assert_eq!(
        camel_support_parse("{\"id\":1,\"value\":\"first\"}"),
        camel_support_parse(&rows[0].to_string())
    );
    assert_eq!(
        camel_support_parse("{\"id\":2,\"value\":\"second\"}"),
        camel_support_parse(&rows[1].to_string())
    );
}
