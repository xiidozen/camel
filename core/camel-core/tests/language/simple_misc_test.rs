//! Ports of `SimpleIncHeaderTest`, `SimpleDecHeaderTest`, `SimpleParserRegexpPredicateTest`
//! and `SimpleWhiteSpaceTest`.

use crate::simple_test::{assert_predicate, exchange};
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_inc_and_dec_header() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:inc")
                .set_header("myCounter", simple("${header.myCounter}++"))
                .to("mock:inc"),
        );
        r.add(
            from("direct:dec")
                .set_header("myCounter", simple("${header.myCounter}--"))
                .to("mock:dec"),
        );
    })
    .await;
    for (uri, mock, expected) in [
        ("direct:inc", "mock:inc", 124),
        ("direct:dec", "mock:dec", 122),
    ] {
        let m = t.mock(mock);
        m.expected_message_count(1);
        m.expected_header_received("myCounter", expected);
        t.template
            .send_body_and_header(uri, "Hello World", "myCounter", 123)
            .await
            .unwrap();
        m.assert_is_satisfied().await;
        let received = m.received_exchanges();
        assert!(matches!(
            received[0].message().header("myCounter"),
            Some(Value::Int(_))
        ));
    }
}

#[test]
fn test_simple_regexp() {
    let (_t, mut e) = exchange();
    let p = r"${body} regex '^\d{2}\.\d{2}\.\d{4}$'";
    e.message_mut().set_body("12.34.5678");
    assert_predicate(&e, p, true);
    e.message_mut().set_body("12.2a.22ab");
    assert_predicate(&e, p, false);
}

#[test]
fn test_white_space() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("some text");
    for sep in [" ", "\t", "\n", "\r\n", "\r"] {
        assert_predicate(
            &e,
            &format!("${{in.body}} contains 'some' &&{sep}${{in.body}} contains 'text'"),
            true,
        );
    }
    e.message_mut().set_body("\tsome\nmulti\rline\r\ntext");
    assert_predicate(&e, "${in.body} == '\tsome\nmulti\rline\r\ntext'", true);
}

/// Port of `SimpleNestedTest`.
#[tokio::test]
async fn test_nested() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("myProp", "${header.myValue}");
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .set_body(simple("{{myProp}}").nested(true))
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received([23]);
    t.template
        .send_body_and_header("direct:start", "13", "myValue", "${sum(${body},10)}")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `SimplePrettyTest`.
#[tokio::test]
async fn test_pretty() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:xml")
                .set_body(simple("<person><name>Jack</name></person>").pretty(true))
                .to("mock:result"),
        );
        r.add(
            from("direct:json")
                .set_body(simple("{ \"name\": \"Jack\", \"age\": 44 }").pretty(true))
                .to("mock:result"),
        );
        r.add(
            from("direct:text")
                .set_body(simple("Hello ${body}").pretty(true))
                .to("mock:result"),
        );
    })
    .await;
    for (uri, body, expected) in [
        (
            "direct:xml",
            "Hello World",
            "<person>\n  <name>\n    Jack\n  </name>\n</person>",
        ),
        (
            "direct:json",
            "Hello World",
            "{\n\t\"name\": \"Jack\",\n\t\"age\": 44\n}\n",
        ),
        ("direct:text", "World", "Hello World"),
    ] {
        let mock = t.mock("mock:result");
        mock.reset();
        mock.expected_bodies_received([expected]);
        t.template.send_body(uri, body).await.unwrap();
        mock.assert_is_satisfied().await;
    }
}

/// Port of `SimpleBackwardsCompatibleTest`.
#[test]
fn test_backwards_compatible() {
    let (_t, mut e) = exchange();
    crate::simple_test::assert_expression(&e, "${body}", "<hello id='m123'>world!</hello>");
    assert_predicate(&e, "${body}", true);
    e.message_mut().set_header("foo", 123);
    crate::simple_test::assert_expression(&e, "${header.foo}", 123);
    assert_predicate(&e, "${header.foo}", true);
    assert_predicate(&e, "${header.unknown}", false);
    e.message_mut().set_body("Hello");
    e.message_mut().set_header("high", true);
    assert_predicate(&e, "${header.high} == true && ${header.foo} == 123", true);
    assert_predicate(&e, "${header.high} == false || ${header.foo} == 123", true);
}

/// Ports of `SimpleSetHeaderPredicateTest` and `SimpleRouteExpressionAsPredicateTest` (a
/// boolean result type makes the expression a predicate).
#[tokio::test]
async fn test_boolean_result_type() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header(
                    "bar",
                    simple("${header.foo} == 'Camel'").result_type("boolean"),
                )
                .to("mock:result"),
        );
        r.add(
            from("direct:other")
                .set_header("param1", constant("hello"))
                .log("param1 = ${header.param1}")
                .set_header(
                    "param2",
                    simple("${header.param1} == 'hello'").result_type("Boolean"),
                )
                .log("param2 = ${header.param2}")
                .to("mock:other"),
        );
        r.add(
            from("direct:foo")
                .set_body(simple("${header.foo} == ${header.foo}").result_type("boolean"))
                .to("mock:foo"),
        );
        r.add(
            from("direct:bar")
                .set_body(simple("${header.bar} == ${header.bar}"))
                .to("mock:bar"),
        );
    })
    .await;
    for (foo, expected) in [("World", false), ("Camel", true)] {
        let m = t.mock("mock:result");
        m.reset();
        m.expected_header_received("bar", expected);
        t.template
            .send_body_and_header("direct:start", "Hello World", "foo", foo)
            .await
            .unwrap();
        m.assert_is_satisfied().await;
    }
    let other = t.mock("mock:other");
    other.expected_header_received("param1", "hello");
    other.expected_header_received("param2", true);
    t.template
        .send_body("direct:other", "Hello World")
        .await
        .unwrap();
    other.assert_is_satisfied().await;
    t.mock("mock:foo").expected_bodies_received([true]);
    t.mock("mock:bar").expected_bodies_received(["ABC == ABC"]);
    t.template
        .send_body_and_header("direct:foo", "Hello World", "foo", "ABC")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:bar", "Hello World", "bar", "ABC")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `BodyOriginalMessageTest`.
#[tokio::test]
async fn test_original_body() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(simple("Bye ${body}"))
                .to("mock:foo")
                .set_body(simple("${originalBody}"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:foo").expected_bodies_received(["Bye World"]);
    t.mock("mock:result").expected_bodies_received(["World"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
