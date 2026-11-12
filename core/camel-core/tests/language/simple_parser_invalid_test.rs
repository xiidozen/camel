//! Ports of `SimpleParserExpressionInvalidTest` and `SimpleParserPredicateInvalidTest`: the
//! position of the syntax errors.

use crate::simple_test::exchange;
use camel_core::languages::simple::expression_parser::SimpleExpressionParser;
use camel_core::languages::simple::predicate_parser::SimplePredicateParser;

fn expression_error_index(text: &str) -> Option<usize> {
    SimpleExpressionParser::new(text, true)
        .parse_expression()
        .err()
        .expect("Should thrown exception")
        .index
}

fn predicate_error_index(text: &str) -> Option<usize> {
    SimplePredicateParser::new(text, true)
        .parse_predicate()
        .err()
        .expect("Should thrown exception")
        .index
}

#[test]
fn test_invalid_expressions() {
    assert_eq!(Some(19), expression_error_index("${body is a nice day"));
    assert_eq!(Some(11), expression_error_index("${body${foo}"));
    assert_eq!(Some(6), expression_error_index("Hello ${foo} how are you?"));
    assert_eq!(Some(11), expression_error_index("Hello ${body"));
}

#[test]
fn test_nested_unknown_function() {
    // the nested function is parsed when evaluating: the error is at its index 0
    let (_t, e) = exchange();
    let err = match SimpleExpressionParser::new("Hello ${bodyAs(${foo})} how are you?", true)
        .parse_expression()
    {
        Ok(exp) => exp.evaluate(&e).expect_err("Should thrown exception"),
        Err(p) => panic!("parsed when evaluating in Java, was: {p:?}"),
    };
    let m = err.to_string();
    assert!(m.contains("at location 0"), "{m}");
}

#[test]
fn test_invalid_predicates() {
    for (text, index) in [
        ("${header.high} == abc", 19),
        ("${header.high} = true", 15),
        ("${body} == 'foo", 14),
        ("${body} == \"foo", 14),
        ("${body} == 'foo' && && ${header} == 123", 20),
        ("${body} == 'foo' || || ${header} == 123", 20),
        ("${body} == == 'foo'", 13),
    ] {
        assert_eq!(Some(index), predicate_error_index(text), "{text}");
    }
}

/// Port of `SimplePredicateParserLogicalTest`.
#[test]
fn test_logical_operators() {
    let matches = |e: &camel_core::prelude::Exchange, text: &str| {
        SimplePredicateParser::new(text, true)
            .parse_predicate()
            .unwrap()
            .matches(e)
            .unwrap()
    };
    use camel_core::prelude::Value;
    let (_t, mut e) = exchange();
    e.message_mut().set_body("hello");
    e.message_mut().set_header("active", true);
    assert!(matches(
        &e,
        "${body} == 'hello' && ${header.active} == true"
    ));
    e.message_mut().set_header("token", Value::Null);
    e.set_property("forceNewSessionToken", true);
    assert!(matches(
        &e,
        "${header.token} == null || ${exchangeProperty.forceNewSessionToken}"
    ));
    e.message_mut().set_body("other");
    assert!(matches(&e, "${header.active} || ${body} == 'hello'"));
    e.message_mut().set_header("enabled", true);
    e.set_property("ready", true);
    assert!(matches(
        &e,
        "${header.enabled} && ${exchangeProperty.ready}"
    ));
    e.set_property("ready", false);
    assert!(!matches(
        &e,
        "${header.enabled} && ${exchangeProperty.ready}"
    ));
    e.message_mut().set_header("token", "abc");
    e.set_property("forceNewSessionToken", false);
    assert!(!matches(
        &e,
        "${header.token} == null || ${exchangeProperty.forceNewSessionToken}"
    ));
    e.message_mut().set_header("foo", "yes");
    assert!(matches(&e, "${header.foo}"));
    e.message_mut().set_body("foo");
    assert!(matches(&e, "${body} == 'foo' && ${body} != 'bar'"));
    e.message_mut().set_body("hello");
    e.message_mut().set_header("score", 5);
    assert!(matches(&e, "${body} == 'world' || ${header.score} > 3"));
    e.message_mut().set_header("score", 1);
    assert!(!matches(&e, "${body} == 'world' || ${header.score} > 3"));
    e.message_mut().set_body(42);
    assert!(matches(&e, "${body} > 10 && ${body} < 100"));
    e.message_mut().set_body("present");
    e.message_mut().set_header("tag", "x");
    assert!(matches(&e, "${body} != null && ${header.tag} != null"));
    e.message_mut().set_body("alpha");
    e.message_mut().set_header("flag", true);
    assert!(matches(
        &e,
        "${body} == 'alpha' && ${header.flag} == true || ${body} == 'beta'"
    ));
    let err = SimplePredicateParser::new("${body} == 'foo' && 42", true)
        .parse_predicate()
        .err()
        .expect("invalid right hand side");
    assert!(
        err.message.contains("right hand side token 42"),
        "{}",
        err.message
    );
    assert!(
        !err.message.contains("right hand side token =="),
        "{}",
        err.message
    );
}
