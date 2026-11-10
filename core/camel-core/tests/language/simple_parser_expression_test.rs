//! Port of `SimpleParserExpressionTest`.

use crate::support::exchange;
use camel_core::languages::simple::expression_parser::SimpleExpressionParser;
use camel_core::prelude::*;
use camel_core::util::value::ValueMap;

fn eval(e: &Exchange, text: &str) -> Value {
    SimpleExpressionParser::new(text, true)
        .parse_expression()
        .unwrap()
        .evaluate(e)
        .unwrap()
}

fn eval_str(e: &Exchange, text: &str) -> String {
    eval(e, text).to_string()
}

#[test]
fn test_simple_parser_eol() {
    assert_eq!("Hello", eval_str(&exchange(), "Hello"));
}

#[test]
fn test_simple_single_quote() {
    assert_eq!("'Hello'", eval_str(&exchange(), "'Hello'"));
}

#[test]
fn test_simple_string_list() {
    assert_eq!(
        "\"Hello\" \"World\"",
        eval_str(&exchange(), "\"Hello\" \"World\"")
    );
}

#[test]
fn test_simple_single_quote_with_function() {
    let mut e = exchange();
    e.message_mut().set_body("World");
    assert_eq!(
        "'Hello World how are you?'",
        eval_str(&e, "'Hello ${body} how are you?'")
    );
}

#[test]
fn test_simple_single_quote_with_function_body_as() {
    let mut e = exchange();
    e.message_mut().set_body("World");
    assert_eq!(
        "'Hello World how are you?'",
        eval_str(&e, "'Hello ${bodyAs(String)} how are you?'")
    );
}

#[test]
fn test_simple_single_quote_eol() {
    assert_eq!("'Hello' World", eval_str(&exchange(), "'Hello' World"));
}

#[test]
fn test_simple_function() {
    let mut e = exchange();
    e.message_mut().set_body("World");
    assert_eq!("World", eval_str(&e, "${body}"));
}

#[test]
fn test_simple_single_quote_dollar() {
    assert_eq!("Pay 200$ today", eval_str(&exchange(), "Pay 200$ today"));
}

#[test]
fn test_simple_single_quote_dollar_end() {
    assert_eq!("Pay 200$", eval_str(&exchange(), "Pay 200$"));
}

#[test]
fn test_simple_unary_inc() {
    let mut e = exchange();
    e.message_mut().set_body("122");
    assert_eq!("123", eval_str(&e, "${body}++"));
}

#[test]
fn test_simple_unary_dec() {
    let mut e = exchange();
    e.message_mut().set_body("122");
    assert_eq!("121", eval_str(&e, "${body}--"));
}

#[test]
fn test_simple_unary_inc_int() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    assert_eq!(Value::Int(123), eval(&e, "${body}++"));
}

#[test]
fn test_simple_unary_dec_int() {
    let mut e = exchange();
    e.message_mut().set_body(122);
    assert_eq!(Value::Int(121), eval(&e, "${body}--"));
}

#[test]
fn test_header_nested_function() {
    let mut e = exchange();
    e.message_mut().set_body("foo");
    e.message_mut().set_header("foo", "abc");
    assert_eq!(Value::from("abc"), eval(&e, "${header.${body}}"));
}

#[test]
fn test_body_as_nested_function() {
    let mut e = exchange();
    e.message_mut().set_body("123");
    e.message_mut().set_header("foo", "Integer");
    assert_eq!(Value::Int(123), eval(&e, "${bodyAs(${header.foo})}"));
}

#[test]
fn test_three_nested_functions() {
    let mut e = exchange();
    e.message_mut().set_body("123");
    e.message_mut().set_header("foo", "Int");
    e.message_mut().set_header("bar", "e");
    e.message_mut().set_header("baz", "ger");
    assert_eq!(
        Value::Int(123),
        eval(&e, "${bodyAs(${header.foo}${header.bar}${header.baz})}")
    );
}

#[test]
fn test_nested_nested_functions() {
    let mut e = exchange();
    e.message_mut().set_body("123");
    e.message_mut().set_header("foo", "Integer");
    e.message_mut().set_header("bar", "foo");
    assert_eq!(
        Value::Int(123),
        eval(&e, "${bodyAs(${header.${header.bar}})}")
    );
}

#[test]
fn test_simple_map() {
    let mut e = exchange();
    let mut map = ValueMap::new();
    map.insert("foo".into(), Value::from("123"));
    map.insert("foo bar".into(), Value::from("456"));
    e.message_mut().set_body(Value::Map(map));
    assert_eq!(Value::from("123"), eval(&e, "${body[foo]}"));
    assert_eq!(Value::from("456"), eval(&e, "${body['foo bar']}"));
}

#[test]
fn test_unary_lenient() {
    let mut e = exchange();
    e.message_mut()
        .set_header("JMSMessageID", "JMSMessageID-123");
    assert_eq!(
        "THE MSG ID JMSMessageID-123 isA --",
        eval_str(&e, "THE MSG ID ${header.JMSMessageID} isA --")
    );
}

#[test]
fn test_unary_lenient2() {
    let mut e = exchange();
    e.message_mut()
        .set_header("JMSMessageID", "JMSMessageID-123");
    assert_eq!(
        "------------THE MSG ID JMSMessageID-123------------",
        eval_str(
            &e,
            "------------THE MSG ID ${header.JMSMessageID}------------"
        )
    );
}

#[test]
fn test_unary_lenient3() {
    let mut e = exchange();
    e.message_mut()
        .set_header("JMSMessageID", "JMSMessageID-123");
    assert_eq!(
        "------------ THE MSG ID JMSMessageID-123 ------------",
        eval_str(
            &e,
            "------------ THE MSG ID ${header.JMSMessageID} ------------"
        )
    );
}
