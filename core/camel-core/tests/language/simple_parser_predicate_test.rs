//! Port of `SimpleParserPredicateTest` (`testSimpleIn` needs the `ref:` function, not ported yet).

use crate::support::exchange;
use camel_core::languages::simple::predicate_parser::SimplePredicateParser;
use camel_core::prelude::*;
use camel_core::util::value::ValueMap;

fn matches(e: &Exchange, text: &str) -> bool {
    SimplePredicateParser::new(text, true)
        .parse_predicate()
        .unwrap()
        .matches(e)
        .unwrap()
}

#[test]
fn test_simple_boolean_value() {
    let mut e = exchange();
    e.message_mut().set_body("foo");
    assert!(matches(&e, "true"));
    assert!(!matches(&e, "false"));
}

#[test]
fn test_simple_eq() {
    let mut e = exchange();
    e.message_mut().set_body("foo");
    assert!(matches(&e, "${body} == 'foo'"));
}

#[test]
fn test_simple_eq_numeric() {
    let mut e = exchange();
    e.message_mut().set_body(123);
    assert!(matches(&e, "${body} == 123"), "Should match");
}

#[test]
fn test_simple_eq_function_function() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    e.message_mut().set_header("val", 122);
    assert!(matches(&e, "${body} == ${header.val}"), "Should match");
}

#[test]
fn test_simple_eq_function_numeric() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    assert!(matches(&e, "${body} == 122"), "Should match");
}

#[test]
fn test_simple_gt_function_numeric() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    assert!(matches(&e, "${body} > 120"), "Should match");
}

#[test]
fn test_simple_unary_inc() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    assert!(matches(&e, "${body}++ == 123"), "Should match");
}

#[test]
fn test_simple_unary_dec() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    assert!(matches(&e, "${body}-- == 121"), "Should match");
}

#[test]
fn test_simple_eq_function_boolean() {
    let mut e = exchange();
    e.message_mut().set_body("Hello");
    e.message_mut().set_header("high", true);
    assert!(matches(&e, "${header.high} == true"), "Should match");
}

#[test]
fn test_simple_eq_function_boolean_spaces() {
    let mut e = exchange();
    e.message_mut().set_body("Hello");
    e.message_mut().set_header("high", true);
    assert!(matches(&e, "${header.high}   ==     true"), "Should match");
}

fn logical_exchange() -> Exchange {
    let mut e = exchange();
    e.message_mut().set_body("Hello");
    e.message_mut().set_header("high", true);
    e.message_mut().set_header("foo", 123);
    e.message_mut().set_header("bar", "beer");
    e
}

#[test]
fn test_simple_logical_and() {
    assert!(
        matches(
            &logical_exchange(),
            "${header.high} == true && ${header.foo} == 123"
        ),
        "Should match"
    );
}

#[test]
fn test_simple_logical_or() {
    assert!(
        matches(
            &logical_exchange(),
            "${header.high} == false || ${header.foo} == 123"
        ),
        "Should match"
    );
}

#[test]
fn test_simple_logical_and_and() {
    assert!(
        matches(
            &logical_exchange(),
            "${header.high} == true && ${header.foo} == 123 && ${header.bar} == 'beer'"
        ),
        "Should match"
    );
}

#[test]
fn test_simple_many_and_logical() {
    let mut e = exchange();
    e.message_mut().set_body("Hello");
    let mut parts = vec![];
    for i in 0..10 {
        e.message_mut().set_header(format!("foo{i}"), i);
        parts.push(format!("${{header.foo{i}}} == {i}"));
    }
    assert!(matches(&e, &parts.join(" && ")), "Should match");
}

#[test]
fn test_simple_many_or_logical() {
    let mut e = exchange();
    e.message_mut().set_body("Hello");
    let parts: Vec<String> = (0..10)
        .map(|i| format!("${{header.foo{i}}} == {i}"))
        .collect();
    let text = parts.join(" || ") + " || ${body} == 'Hello'";
    assert!(matches(&e, &text), "Should match");
}

#[test]
fn test_simple_expression_predicate() {
    let mut e = exchange();
    e.message_mut().set_body("Hello");
    e.message_mut().set_header("number", "1234");
    assert!(
        matches(&e, "${in.header.number} regex '\\d{4}'"),
        "Should match"
    );
}

#[test]
fn test_simple_map() {
    let mut e = exchange();
    let mut map = ValueMap::new();
    map.insert("foo".into(), Value::from("123"));
    map.insert("foo bar".into(), Value::from("456"));
    e.message_mut().set_body(Value::Map(map));
    assert!(matches(&e, "${body[foo]} == 123"), "Should match");
    assert!(matches(&e, "${body['foo bar']} == 456"), "Should match");
    assert!(matches(&e, "${body[foo bar]} == 456"), "Should match");
    assert!(!matches(&e, "${body[unknown]} == 456"), "Should not match");
}

#[test]
fn test_simple_in_empty() {
    let p = SimplePredicateParser::new("${body} in ',,gold,silver'", true)
        .parse_predicate()
        .unwrap();
    let mut e = exchange();
    for (body, expected) in [
        ("gold", true),
        ("silver", true),
        ("", true),
        ("bronze", false),
    ] {
        e.message_mut().set_body(body);
        assert_eq!(expected, p.matches(&e).unwrap(), "{body}");
    }
}

#[test]
fn test_simple_with_ambiguous_binary_operator() {
    let expression = r#"${body[value][conditions].getJSONObject(${exchangeProperty[CamelLoopIndex]})[levelType]} == "1"
&& ${body[value][conditions].getJSONObject(${exchangeProperty[CamelLoopIndex]})[minLevel]} != null
&& ${body[value][conditions].getJSONObject(${exchangeProperty[CamelLoopIndex]})[minLevel]} == "50"
"#;
    assert!(SimplePredicateParser::new(expression, true)
        .parse_predicate()
        .is_ok());
}
