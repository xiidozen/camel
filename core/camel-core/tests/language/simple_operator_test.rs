//! Port of `SimpleOperatorTest`. Java `BigDecimal` values are decimals here.

use super::simple_test::{assert_expression, assert_predicate, evaluate};
use crate::support::ContextTestSupport;
use camel_bean::MethodBean;
use camel_core::api::convert::FromValue;
use camel_core::api::exchange_constants::FILE_NAME;
use camel_core::api::processor::SharedPredicate;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// The exchange of the test, with the Java `MyFileNameGenerator` bound as `generator`.
fn exchange() -> (ContextTestSupport, Exchange) {
    let (t, e) = super::simple_test::exchange();
    let generator = MethodBean::new("MyFileNameGenerator")
        .method("generateFilename", |_| Ok(Value::from("abc")))
        .method("generateId", |_| Ok(Value::Int(123)));
    t.context
        .registry()
        .unwrap()
        .bind("generator", generator.into_value())
        .unwrap();
    (t, e)
}

fn create_predicate(e: &Exchange, text: &str) -> SharedPredicate {
    let simple = e.context().resolve_language("simple").unwrap();
    simple.create_predicate(text).unwrap()
}

/// Java `assertThrows(SimpleIllegalSyntaxException.class, ...)` with `getIndex()`.
fn assert_syntax_error_at(e: &Exchange, text: &str, index: usize) {
    let simple = e.context().resolve_language("simple").unwrap();
    let err = simple
        .create_predicate(text)
        .err()
        .unwrap_or_else(|| panic!("Should have thrown an exception: {text}"));
    assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err:?}");
    let message = err.message().unwrap_or_default();
    assert!(
        message.contains(&format!(" at location {index}\n")),
        "{text}: {message}"
    );
}

#[test]
fn test_value_with_space() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello Big World");
    assert_predicate(&e, "${in.body} == 'Hello Big World'", true);
    assert_predicate(&e, "${in.body} == ${body}", true);
}

#[test]
fn test_null_value() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Value");
    assert_predicate(&e, "${in.body} != null", true);
    assert_predicate(&e, "${body} == null", false);

    e.message_mut().set_body(Value::Null);
    assert_predicate(&e, "${in.body} == null", true);
    assert_predicate(&e, "${body} != null", false);
}

#[test]
fn test_empty_value() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("");
    assert_predicate(&e, "${in.body} == null", false);
    assert_predicate(&e, "${body} == null", false);

    e.message_mut().set_body("");
    assert_predicate(&e, "${in.body} == ''", true);
    assert_predicate(&e, "${body} == \"\"", true);

    e.message_mut().set_body(" ");
    assert_predicate(&e, "${in.body} == ''", false);
    assert_predicate(&e, "${body} == \"\"", false);

    e.message_mut().set_body("Value");
    assert_predicate(&e, "${in.body} == ''", false);
    assert_predicate(&e, "${body} == \"\"", false);
}

#[test]
fn test_and() {
    let (_t, e) = exchange();
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} == 123",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} == 444",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' && ${in.header.bar} == 123",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' && ${in.header.bar} == 444",
        false,
    );

    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} > 100",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} < 200",
        true,
    );
}

#[test]
fn test_two_and() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} == 123 && ${body} == 'Hello World'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} == 123 && ${body} == 'Bye World'",
        false,
    );
}

#[test]
fn test_three_and() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_predicate(&e, "${in.header.foo} == 'abc' && ${in.header.bar} == 123 && ${body} == 'Hello World' && ${in.header.xx} == null", true);
}

#[test]
fn test_two_or() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} == 44 || ${body} == 'Bye World'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'xxx' || ${in.header.bar} == 44 || ${body} == 'Bye World'",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'xxx' || ${in.header.bar} == 44 || ${body} == 'Hello World'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'xxx' || ${in.header.bar} == 123 || ${body} == 'Bye World'",
        true,
    );
}

#[test]
fn test_three_or() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_predicate(&e, "${in.header.foo} == 'xxx' || ${in.header.bar} == 44 || ${body} == 'Bye Moon' || ${body} contains 'World'", true);
    assert_predicate(&e, "${in.header.foo} == 'xxx' || ${in.header.bar} == 44 || ${body} == 'Bye Moon' || ${body} contains 'Moon'", false);
    assert_predicate(&e, "${in.header.foo} == 'abc' || ${in.header.bar} == 44 || ${body} == 'Bye Moon' || ${body} contains 'Moon'", true);
    assert_predicate(&e, "${in.header.foo} == 'xxx' || ${in.header.bar} == 123 || ${body} == 'Bye Moon' || ${body} contains 'Moon'", true);
    assert_predicate(&e, "${in.header.foo} == 'xxx' || ${in.header.bar} == 44 || ${body} == 'Hello World' || ${body} contains 'Moon'", true);
}

#[test]
fn test_and_with_quotation() {
    let (_t, e) = exchange();
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} == '123'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} == '444'",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' && ${in.header.bar} == '123'",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' && ${in.header.bar} == '444'",
        false,
    );

    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} > '100'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' && ${in.header.bar} < '200'",
        true,
    );
}

#[test]
fn test_or() {
    let (_t, e) = exchange();
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} == 123",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} == 444",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} == 123",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} == 444",
        false,
    );

    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} < 100",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} < 200",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} < 200",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} < 100",
        false,
    );
}

#[test]
fn test_or_with_quotation() {
    let (_t, e) = exchange();
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} == '123'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} == '444'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} == '123'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} == '444'",
        false,
    );

    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} < '100'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'abc' || ${in.header.bar} < '200'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} < '200'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.foo} == 'def' || ${in.header.bar} < '100'",
        false,
    );
}

#[test]
fn test_equal_operator() {
    let (_t, mut e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} == 'abc'", true);
    assert_predicate(&e, "${in.header.foo} == 'def'", false);
    assert_predicate(&e, "${in.header.foo} == '1'", false);

    // special with just minus sign
    assert_predicate(&e, "${in.header.foo} == '-'", false);
    assert_predicate(&e, "${in.header.bar} == '-'", false);

    // boolean to boolean comparison
    e.message_mut().set_header("bool", true);
    e.message_mut().set_header("booley", false);
    assert_predicate(&e, "${in.header.bool} == true", true);
    assert_predicate(&e, "${in.header.bool} == 'true'", true);
    assert_predicate(&e, "${in.header.booley} == false", true);
    assert_predicate(&e, "${in.header.booley} == 'false'", true);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} == '123'", true);
    assert_predicate(&e, "${in.header.bar} == 123", true);
    assert_predicate(&e, "${in.header.bar} == '444'", false);
    assert_predicate(&e, "${in.header.bar} == 444", false);
    assert_predicate(&e, "${in.header.bar} == '1'", false);

    // should not need type conversion
    // n/a: no type converter statistics in Rust
}

#[test]
fn test_equal_ignore_operator() {
    let (_t, e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} =~ 'abc'", true);
    assert_predicate(&e, "${in.header.foo} =~ 'ABC'", true);
    assert_predicate(&e, "${in.header.foo} =~ 'Abc'", true);
    assert_predicate(&e, "${in.header.foo} =~ 'Def'", false);
    assert_predicate(&e, "${in.header.foo} =~ '1'", false);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} =~ '123'", true);
    assert_predicate(&e, "${in.header.bar} =~ 123", true);
    assert_predicate(&e, "${in.header.bar} =~ '444'", false);
    assert_predicate(&e, "${in.header.bar} =~ 444", false);
    assert_predicate(&e, "${in.header.bar} =~ '1'", false);

    // special with just minus sign
    assert_predicate(&e, "${in.header.foo} =~ '-'", false);
    assert_predicate(&e, "${in.header.bar} =~ '-'", false);
}

#[test]
fn test_not_equal_operator() {
    let (_t, e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} != 'abc'", false);
    assert_predicate(&e, "${in.header.foo} != 'def'", true);
    assert_predicate(&e, "${in.header.foo} != '1'", true);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} != '123'", false);
    assert_predicate(&e, "${in.header.bar} != 123", false);
    assert_predicate(&e, "${in.header.bar} != '444'", true);
    assert_predicate(&e, "${in.header.bar} != 444", true);
    assert_predicate(&e, "${in.header.bar} != '1'", true);

    // special with just minus sign
    assert_predicate(&e, "${in.header.foo} != '-'", true);
    assert_predicate(&e, "${in.header.bar} != '-'", true);
}

#[test]
fn test_not_equal_ignore_operator() {
    let (_t, e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} !=~ 'abc'", false);
    assert_predicate(&e, "${in.header.foo} !=~ 'ABC'", false);
    assert_predicate(&e, "${in.header.foo} !=~ 'Abc'", false);
    assert_predicate(&e, "${in.header.foo} !=~ 'Def'", true);
    assert_predicate(&e, "${in.header.foo} !=~ '1'", true);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} !=~ '123'", false);
    assert_predicate(&e, "${in.header.bar} !=~ 123", false);
    assert_predicate(&e, "${in.header.bar} !=~ '444'", true);
    assert_predicate(&e, "${in.header.bar} !=~ 444", true);
    assert_predicate(&e, "${in.header.bar} !=~ '1'", true);
}

#[test]
fn test_floating_number() {
    let (_t, mut e) = exchange();
    // set a String value
    e.message_mut().set_body("0.02");

    assert_predicate(&e, "${body} > 0", true);
    assert_predicate(&e, "${body} < 0", false);

    assert_predicate(&e, "${body} > 0.00", true);
    assert_predicate(&e, "${body} < 0.00", false);

    assert_predicate(&e, "${body} > 0.01", true);
    assert_predicate(&e, "${body} < 0.01", false);

    assert_predicate(&e, "${body} > 0.02", false);
    assert_predicate(&e, "${body} < 0.02", false);

    assert_predicate(&e, "${body} == 0.02", true);
}

#[test]
fn test_greater_than_operator() {
    let (_t, e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} > 'aaa'", true);
    assert_predicate(&e, "${in.header.foo} > 'def'", false);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} > '100'", true);
    assert_predicate(&e, "${in.header.bar} > 100", true);
    assert_predicate(&e, "${in.header.bar} > '123'", false);
    assert_predicate(&e, "${in.header.bar} > 123", false);
    assert_predicate(&e, "${in.header.bar} > '200'", false);
}

#[test]
fn test_greater_than_string_to_int() {
    let (_t, mut e) = exchange();
    // set a String value
    e.message_mut().set_header("num", "70");

    // string to int comparison
    assert_predicate(&e, "${in.header.num} > 100", false);
    assert_predicate(&e, "${in.header.num} > 100", false);
    assert_predicate(&e, "${in.header.num} > 80", false);
    assert_predicate(&e, "${in.header.num} > 800", false);
    assert_predicate(&e, "${in.header.num} > 1", true);
    assert_predicate(&e, "${in.header.num} > 8", true);
    assert_predicate(&e, "${in.header.num} > 48", true);
    assert_predicate(&e, "${in.header.num} > 69", true);
    assert_predicate(&e, "${in.header.num} > 71", false);
    assert_predicate(&e, "${in.header.num} > 88", false);
    assert_predicate(&e, "${in.header.num} > 777", false);
}

#[test]
fn test_less_than_string_to_int() {
    let (_t, mut e) = exchange();
    // set a String value
    e.message_mut().set_header("num", "70");

    // string to int comparison
    assert_predicate(&e, "${in.header.num} < 100", true);
    assert_predicate(&e, "${in.header.num} < 100", true);
    assert_predicate(&e, "${in.header.num} < 80", true);
    assert_predicate(&e, "${in.header.num} < 800", true);
    assert_predicate(&e, "${in.header.num} < 1", false);
    assert_predicate(&e, "${in.header.num} < 8", false);
    assert_predicate(&e, "${in.header.num} < 48", false);
    assert_predicate(&e, "${in.header.num} < 69", false);
    assert_predicate(&e, "${in.header.num} < 71", true);
    assert_predicate(&e, "${in.header.num} < 88", true);
    assert_predicate(&e, "${in.header.num} < 777", true);
}

#[test]
fn test_greater_than_or_equal_operator() {
    let (_t, e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} >= 'aaa'", true);
    assert_predicate(&e, "${in.header.foo} >= 'abc'", true);
    assert_predicate(&e, "${in.header.foo} >= 'def'", false);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} >= '100'", true);
    assert_predicate(&e, "${in.header.bar} >= 100", true);
    assert_predicate(&e, "${in.header.bar} >= '123'", true);
    assert_predicate(&e, "${in.header.bar} >= 123", true);
    assert_predicate(&e, "${in.header.bar} >= '200'", false);

    // special with just minus sign
    assert_predicate(&e, "${in.header.foo} >= '-'", true);
    assert_predicate(&e, "${in.header.bar} >= '-'", true);
}

#[test]
fn test_less_than_operator() {
    let (_t, e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} < 'aaa'", false);
    assert_predicate(&e, "${in.header.foo} < 'def'", true);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} < '100'", false);
    assert_predicate(&e, "${in.header.bar} < 100", false);
    assert_predicate(&e, "${in.header.bar} < '123'", false);
    assert_predicate(&e, "${in.header.bar} < 123", false);
    assert_predicate(&e, "${in.header.bar} < '200'", true);
}

#[test]
fn test_against_negative_value() {
    let (_t, mut e) = exchange();
    assert_predicate(&e, "${in.header.bar} == 123", true);
    assert_predicate(&e, "${in.header.bar} == -123", false);
    assert_predicate(&e, "${in.header.bar} =~ 123", true);
    assert_predicate(&e, "${in.header.bar} =~ -123", false);
    assert_predicate(&e, "${in.header.bar} > -123", true);
    assert_predicate(&e, "${in.header.bar} >= -123", true);
    assert_predicate(&e, "${in.header.bar} > 123", false);
    assert_predicate(&e, "${in.header.bar} >= 123", true);
    assert_predicate(&e, "${in.header.bar} < -123", false);
    assert_predicate(&e, "${in.header.bar} <= -123", false);
    assert_predicate(&e, "${in.header.bar} < 123", false);
    assert_predicate(&e, "${in.header.bar} <= 123", true);

    e.message_mut().set_header("strNum", "123");
    assert_predicate(&e, "${in.header.strNum} contains '123'", true);
    assert_predicate(&e, "${in.header.strNum} !contains '123'", false);
    assert_predicate(&e, "${in.header.strNum} contains '-123'", false);
    assert_predicate(&e, "${in.header.strNum} !contains '-123'", true);
    assert_predicate(&e, "${in.header.strNum} ~~ '123'", true);
    assert_predicate(&e, "${in.header.strNum} ~~ '-123'", false);

    e.message_mut().set_header("num", -123);
    assert_predicate(&e, "${in.header.num} == -123", true);
    assert_predicate(&e, "${in.header.num} == 123", false);
    assert_predicate(&e, "${in.header.num} =~ -123", true);
    assert_predicate(&e, "${in.header.num} =~ 123", false);
    assert_predicate(&e, "${in.header.num} > -123", false);
    assert_predicate(&e, "${in.header.num} >= -123", true);
    assert_predicate(&e, "${in.header.num} > 123", false);
    assert_predicate(&e, "${in.header.num} >= 123", false);
    assert_predicate(&e, "${in.header.num} < -123", false);
    assert_predicate(&e, "${in.header.num} <= -123", true);
    assert_predicate(&e, "${in.header.num} < 123", true);
    assert_predicate(&e, "${in.header.num} <= 123", true);

    e.message_mut().set_header("strNumNegative", "-123");
    assert_predicate(&e, "${in.header.strNumNegative} contains '123'", true);
    assert_predicate(&e, "${in.header.strNumNegative} !contains '123'", false);
    assert_predicate(&e, "${in.header.strNumNegative} contains '-123'", true);
    assert_predicate(&e, "${in.header.strNumNegative} !contains '-123'", false);
    assert_predicate(&e, "${in.header.strNumNegative} ~~ '123'", true);
    assert_predicate(&e, "${in.header.strNumNegative} ~~ '-123'", true);
}

#[test]
fn test_less_than_or_equal_operator() {
    let (_t, mut e) = exchange();
    // string to string comparison
    assert_predicate(&e, "${in.header.foo} <= 'aaa'", false);
    assert_predicate(&e, "${in.header.foo} <= 'abc'", true);
    assert_predicate(&e, "${in.header.foo} <= 'def'", true);

    // string to string
    e.message_mut().set_header("dude", "555");
    e.message_mut().set_header("dude2", "0099");
    assert_predicate(&e, "${in.header.dude} <= ${in.header.dude}", true);
    assert_predicate(&e, "${in.header.dude2} <= ${in.header.dude}", true);

    // integer to string comparison
    assert_predicate(&e, "${in.header.bar} <= '100'", false);
    assert_predicate(&e, "${in.header.bar} <= 100", false);
    assert_predicate(&e, "${in.header.bar} <= '123'", true);
    assert_predicate(&e, "${in.header.bar} <= 123", true);
    assert_predicate(&e, "${in.header.bar} <= '200'", true);

    // should not need type conversion
    // n/a: no type converter statistics in Rust
}

#[test]
fn test_type_coerce_no_conversion_needed() {
    let (_t, mut e) = exchange();
    // int to int comparison
    e.message_mut().set_header("num", 70);
    assert_predicate(&e, "${in.header.num} > 100", false);
    assert_predicate(&e, "${in.header.num} < 100", true);
    assert_predicate(&e, "${in.header.num} == 70", true);
    assert_predicate(&e, "${in.header.num} != 70", false);
    assert_predicate(&e, "${in.header.num} > 100", false);
    assert_predicate(&e, "${in.header.num} > 80", false);
    assert_predicate(&e, "${in.header.num} > 800", false);
    assert_predicate(&e, "${in.header.num} < 800", true);
    assert_predicate(&e, "${in.header.num} > 1", true);
    assert_predicate(&e, "${in.header.num} > 8", true);
    assert_predicate(&e, "${in.header.num} > 48", true);
    assert_predicate(&e, "${in.header.num} > 69", true);
    assert_predicate(&e, "${in.header.num} > 71", false);
    assert_predicate(&e, "${in.header.num} < 71", true);
    assert_predicate(&e, "${in.header.num} > 88", false);
    assert_predicate(&e, "${in.header.num} > 777", false);

    // String to int comparison
    e.message_mut().set_header("num", "70");
    assert_predicate(&e, "${in.header.num} > 100", false);
    assert_predicate(&e, "${in.header.num} < 100", true);
    assert_predicate(&e, "${in.header.num} == 70", true);
    assert_predicate(&e, "${in.header.num} != 70", false);
    assert_predicate(&e, "${in.header.num} > 100", false);
    assert_predicate(&e, "${in.header.num} > 80", false);
    assert_predicate(&e, "${in.header.num} > 800", false);
    assert_predicate(&e, "${in.header.num} < 800", true);
    assert_predicate(&e, "${in.header.num} > 1", true);
    assert_predicate(&e, "${in.header.num} > 8", true);
    assert_predicate(&e, "${in.header.num} > 48", true);
    assert_predicate(&e, "${in.header.num} > 69", true);
    assert_predicate(&e, "${in.header.num} > 71", false);
    assert_predicate(&e, "${in.header.num} < 71", true);
    assert_predicate(&e, "${in.header.num} > 88", false);
    assert_predicate(&e, "${in.header.num} > 777", false);

    // should not need type conversion
    // n/a: no type converter statistics in Rust
}

#[test]
fn test_is_null() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} == null", false);
    assert_predicate(&e, "${in.header.none} == null", true);
}

#[test]
fn test_is_not_null() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} != null", true);
    assert_predicate(&e, "${in.header.none} != null", false);
}

#[test]
fn test_right_operator_is_simple_language() {
    let (_t, e) = exchange();
    // operator on right side is also using ${ } placeholders
    assert_predicate(&e, "${in.header.foo} == ${in.header.foo}", true);
    assert_predicate(&e, "${in.header.foo} == ${in.header.bar}", false);
}

#[test]
fn test_right_operator_is_bean_language() {
    let (_t, e) = exchange();
    // operator on right side is also using ${ } placeholders
    assert_predicate(
        &e,
        "${in.header.foo} == ${bean:generator.generateFilename}",
        true,
    );
    assert_predicate(&e, "${in.header.bar} == ${bean:generator.generateId}", true);
    assert_predicate(&e, "${in.header.bar} >= ${bean:generator.generateId}", true);
}

#[test]
fn test_contains() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} contains 'a'", true);
    assert_predicate(&e, "${in.header.foo} contains 'ab'", true);
    assert_predicate(&e, "${in.header.foo} contains 'abc'", true);
    assert_predicate(&e, "${in.header.foo} contains 'def'", false);
}

#[test]
fn test_contains_braces_in_quoted_literal() {
    let (_t, mut e) = exchange();
    // a } inside a quoted literal is text, not the end of a function
    e.message_mut().set_body("{name:XOrder,items:[]}");
    assert_predicate(&e, "${body} contains '{name:XOrder}'", false);
    assert_predicate(&e, "${body} contains '{name:XOrder,'", true);
    assert_predicate(&e, "${body} contains '}'", true);
    assert_predicate(&e, "${body} contains \"[]}\"", true);
    assert_predicate(&e, "${body} contains '{'", true);
    assert_predicate(&e, "${body} == '{name:XOrder,items:[]}'", true);
    assert_predicate(
        &e,
        "${body} == '{name:XOrder,items:[]}' && ${body} contains '}'",
        true,
    );
}

#[test]
fn test_contains_number_in_string() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body("The answer is 42 and is the answer to life the universe and everything");
    assert_predicate(&e, "${body} contains '42'", true);
    assert_predicate(&e, "${body} contains 42", true);
    assert_predicate(&e, "${body} contains '77'", false);
    assert_predicate(&e, "${body} contains 77", false);
}

#[test]
fn test_not_contains() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} not contains 'a'", false);
    assert_predicate(&e, "${in.header.foo} not contains 'ab'", false);
    assert_predicate(&e, "${in.header.foo} not contains 'abc'", false);
    assert_predicate(&e, "${in.header.foo} not contains 'def'", true);
    assert_predicate(&e, "${in.header.foo} !contains 'a'", false);
    assert_predicate(&e, "${in.header.foo} !contains 'ab'", false);
    assert_predicate(&e, "${in.header.foo} !contains 'abc'", false);
    assert_predicate(&e, "${in.header.foo} !contains 'def'", true);
}

#[test]
fn test_contains_ignore_case() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} ~~ 'A'", true);
    assert_predicate(&e, "${in.header.foo} ~~ 'Ab'", true);
    assert_predicate(&e, "${in.header.foo} ~~ 'Abc'", true);
    assert_predicate(&e, "${in.header.foo} ~~ 'defG'", false);
}

#[test]
fn test_not_contains_ignore_case() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} !~~ 'A'", false);
    assert_predicate(&e, "${in.header.foo} !~~ 'Ab'", false);
    assert_predicate(&e, "${in.header.foo} !~~ 'Abc'", false);
    assert_predicate(&e, "${in.header.foo} !~~ 'defG'", true);
}

#[test]
fn test_regex() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} regex '^a..$'", true);
    assert_predicate(&e, "${in.header.foo} regex '^ab.$'", true);
    assert_predicate(&e, "${in.header.foo} regex '^ab.$'", true);
    assert_predicate(&e, "${in.header.foo} regex '^d.*$'", false);

    assert_predicate(&e, "${in.header.bar} regex '^\\d{3}'", true);
    assert_predicate(&e, "${in.header.bar} regex '^\\d{2}'", false);
}

#[test]
fn test_not_regex() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} not regex '^a..$'", false);
    assert_predicate(&e, "${in.header.foo} not regex '^ab.$'", false);
    assert_predicate(&e, "${in.header.foo} not regex '^ab.$'", false);
    assert_predicate(&e, "${in.header.foo} not regex '^d.*$'", true);

    assert_predicate(&e, "${in.header.bar} not regex '^\\d{3}'", false);
    assert_predicate(&e, "${in.header.bar} not regex '^\\d{2}'", true);
}

#[test]
fn test_in() {
    let (_t, e) = exchange();
    // string to string
    assert_predicate(&e, "${in.header.foo} in 'foo,abc,def'", true);
    assert_predicate(
        &e,
        "${in.header.foo} in ${bean:generator.generateFilename}",
        true,
    );
    assert_predicate(&e, "${in.header.foo} in 'foo,abc,def'", true);
    assert_predicate(&e, "${in.header.foo} in 'foo,def'", false);

    // integer to string
    assert_predicate(&e, "${in.header.bar} in '100,123,200'", true);
    assert_predicate(&e, "${in.header.bar} in ${bean:generator.generateId}", true);
    assert_predicate(&e, "${in.header.bar} in '100,200'", false);
}

#[test]
fn test_not_in() {
    let (_t, e) = exchange();
    // string to string
    assert_predicate(&e, "${in.header.foo} not in 'foo,abc,def'", false);
    assert_predicate(
        &e,
        "${in.header.foo} not in ${bean:generator.generateFilename}",
        false,
    );
    assert_predicate(&e, "${in.header.foo} not in 'foo,abc,def'", false);
    assert_predicate(&e, "${in.header.foo} not in 'foo,def'", true);
    assert_predicate(&e, "${in.header.foo} !in 'foo,abc,def'", false);
    assert_predicate(
        &e,
        "${in.header.foo} !in ${bean:generator.generateFilename}",
        false,
    );
    assert_predicate(&e, "${in.header.foo} !in 'foo,abc,def'", false);
    assert_predicate(&e, "${in.header.foo} !in 'foo,def'", true);

    // integer to string
    assert_predicate(&e, "${in.header.bar} not in '100,123,200'", false);
    assert_predicate(
        &e,
        "${in.header.bar} not in ${bean:generator.generateId}",
        false,
    );
    assert_predicate(&e, "${in.header.bar} not in '100,200'", true);
    assert_predicate(&e, "${in.header.bar} !in '100,123,200'", false);
    assert_predicate(
        &e,
        "${in.header.bar} !in ${bean:generator.generateId}",
        false,
    );
    assert_predicate(&e, "${in.header.bar} !in '100,200'", true);
}

#[test]
fn test_is() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} is 'java.lang.String'", true);
    assert_predicate(&e, "${in.header.foo} is 'java.lang.Integer'", false);

    assert_predicate(&e, "${in.header.foo} is 'String'", true);
    assert_predicate(&e, "${in.header.foo} is 'Integer'", false);

    assert_syntax_error_at(&e, "${in.header.foo} is com.mycompany.DoesNotExist", 20);
}

#[test]
fn test_is_not() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.foo} not is 'java.lang.String'", false);
    assert_predicate(&e, "${in.header.foo} not is 'java.lang.Integer'", true);
    assert_predicate(&e, "${in.header.foo} !is 'java.lang.String'", false);
    assert_predicate(&e, "${in.header.foo} !is 'java.lang.Integer'", true);

    assert_predicate(&e, "${in.header.foo} not is 'String'", false);
    assert_predicate(&e, "${in.header.foo} not is 'Integer'", true);
    assert_predicate(&e, "${in.header.foo} !is 'String'", false);
    assert_predicate(&e, "${in.header.foo} !is 'Integer'", true);

    assert_syntax_error_at(&e, "${in.header.foo} not is com.mycompany.DoesNotExist", 24);

    assert_syntax_error_at(&e, "${in.header.foo} !is com.mycompany.DoesNotExist", 21);
}

#[test]
fn test_compare_decimal_with_integer() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("amount", 100.50);
    e.message_mut().set_header("price", 2.5);
    assert_predicate(&e, "${header.amount} > 100", true);
    assert_predicate(&e, "${header.amount} == 100", false);
    assert_predicate(&e, "${header.amount} != 100", true);
    assert_predicate(&e, "${header.amount} <= 100", false);
    assert_predicate(&e, "${header.price} > 2", true);
    assert_predicate(&e, "${header.price} == 2", false);
    assert_predicate(&e, "${header.price} range '1..2'", false);
    assert_predicate(&e, "${header.price} range '2..3'", true);
}

#[test]
fn test_in_with_decimals() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("n", 2);
    e.message_mut().set_header("big", 99);
    e.message_mut().set_header("amount", 2.50);
    // the elements are compared with an integer header by their numeric values, without dropping the decimals
    assert_predicate(&e, "${header.n} in '2.5,3.5'", false);
    assert_predicate(&e, "${header.n} in '2.9'", false);
    assert_predicate(&e, "${header.n} !in '2.5'", true);
    assert_predicate(&e, "${header.n} in '2,3'", true);
    assert_predicate(&e, "${header.n} in '1, 2'", true);
    assert_predicate(&e, "${header.n} in '4294967298'", false);
    // 2.0 is the same number as 2, as with ${header.n} == 2.0
    assert_predicate(&e, "${header.n} in '2.0'", true);
    assert_predicate(&e, "${header.n} == 2.0", true);
    assert_predicate(&e, "${header.big} in '99.99,100.01'", false);
    assert_predicate(&e, "${header.big} in '98,99'", true);
    assert_predicate(&e, "${header.amount} in '2.5'", true);
    assert_predicate(&e, "${header.amount} in '2'", false);
}

#[test]
fn test_range() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.bar} range '100..200'", true);
    assert_predicate(&e, "${in.header.bar} range '200..300'", false);

    assert_predicate(&e, "${in.header.foo} range '200..300'", false);
    assert_predicate(&e, "${bean:generator.generateId} range '123..130'", true);
    assert_predicate(&e, "${bean:generator.generateId} range '120..123'", true);
    assert_predicate(&e, "${bean:generator.generateId} range '120..122'", false);
    assert_predicate(&e, "${bean:generator.generateId} range '124..130'", false);

    assert_syntax_error_at(&e, "${in.header.foo} range abc..200", 23);

    assert_syntax_error_at(&e, "${in.header.foo} range abc..", 23);

    assert_syntax_error_at(&e, "${in.header.foo} range 100.200", 30);

    assert_predicate(
        &e,
        "${in.header.bar} range '100..200' && ${in.header.foo} == 'abc'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.bar} range '200..300' && ${in.header.foo} == 'abc'",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.bar} range '200..300' || ${in.header.foo} == 'abc'",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.bar} range '200..300' || ${in.header.foo} == 'def'",
        false,
    );
}

#[test]
fn test_not_range() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${in.header.bar} not range '100..200'", false);
    assert_predicate(&e, "${in.header.bar} not range '200..300'", true);
    assert_predicate(&e, "${in.header.bar} !range '100..200'", false);
    assert_predicate(&e, "${in.header.bar} !range '200..300'", true);

    assert_predicate(&e, "${in.header.foo} not range '200..300'", true);
    assert_predicate(
        &e,
        "${bean:generator.generateId} not range '123..130'",
        false,
    );
    assert_predicate(
        &e,
        "${bean:generator.generateId} not range '120..123'",
        false,
    );
    assert_predicate(
        &e,
        "${bean:generator.generateId} not range '120..122'",
        true,
    );
    assert_predicate(
        &e,
        "${bean:generator.generateId} not range '124..130'",
        true,
    );
    assert_predicate(&e, "${in.header.foo} !range '200..300'", true);
    assert_predicate(&e, "${bean:generator.generateId} !range '123..130'", false);
    assert_predicate(&e, "${bean:generator.generateId} !range '120..123'", false);
    assert_predicate(&e, "${bean:generator.generateId} !range '120..122'", true);
    assert_predicate(&e, "${bean:generator.generateId} !range '124..130'", true);

    assert_syntax_error_at(&e, "${in.header.foo} not range abc..200", 27);

    assert_syntax_error_at(&e, "${in.header.foo} !range abc..200", 24);

    assert_syntax_error_at(&e, "${in.header.foo} not range abc..", 27);

    assert_syntax_error_at(&e, "${in.header.foo} !range abc..", 24);

    assert_syntax_error_at(&e, "${in.header.foo} not range 100.200", 34);

    assert_syntax_error_at(&e, "${in.header.foo} !range 100.200", 31);
}

#[test]
fn test_unary_inc() {
    let (_t, e) = exchange();
    assert_expression(&e, "${in.header.bar}++", 124);
    assert_expression(&e, "+++++++++++++", "+++++++++++++");
    assert_expression(&e, "Logging ++ start ++", "Logging ++ start ++");
    assert_expression(&e, "Logging +++ start +++", "Logging +++ start +++");
    assert_expression(&e, "++ start ++", "++ start ++");
    assert_expression(&e, "+++ start +++", "+++ start +++");

    assert_predicate(&e, "${in.header.bar}++ == 122", false);
    assert_predicate(&e, "${in.header.bar}++ == 123", false);
    assert_predicate(&e, "${in.header.bar}++ == 124", true);
}

#[test]
fn test_unary_dec() {
    let (_t, e) = exchange();
    assert_expression(&e, "${in.header.bar}--", 122);
    assert_expression(&e, "-------------", "-------------");
    assert_expression(&e, "Logging -- start --", "Logging -- start --");
    assert_expression(&e, "Logging --- start ---", "Logging --- start ---");
    assert_expression(&e, "-- start --", "-- start --");
    assert_expression(&e, "--- start ---", "--- start ---");

    assert_predicate(&e, "${in.header.bar}-- == 122", true);
    assert_predicate(&e, "${in.header.bar}-- == 123", false);
    assert_predicate(&e, "${in.header.bar}-- == 124", false);
}

#[test]
fn test_starts_with() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello there");
    assert_predicate(&e, "${in.body} starts with 'Hello'", true);
    assert_predicate(&e, "${in.body} starts with 'H'", true);
    assert_predicate(&e, "${in.body} starts with 'Hello there'", true);
    assert_predicate(&e, "${in.body} starts with 'Hello ther'", true);
    assert_predicate(&e, "${in.body} starts with 'ello there'", false);
    assert_predicate(&e, "${in.body} starts with 'Hi'", false);
    assert_predicate(&e, "${in.body} startsWith 'Hello'", true);
    assert_predicate(&e, "${in.body} startsWith 'H'", true);
    assert_predicate(&e, "${in.body} startsWith 'Hello there'", true);
    assert_predicate(&e, "${in.body} startsWith 'Hello ther'", true);
    assert_predicate(&e, "${in.body} startsWith 'ello there'", false);
    assert_predicate(&e, "${in.body} startsWith 'Hi'", false);
}

#[test]
fn test_starts_with_text_as_numeric() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("01234");
    assert_predicate(&e, "${in.body} starts with '1234'", false);
    assert_predicate(&e, "${in.body} starts with 1234", false);
    assert_predicate(&e, "${in.body} starts with '01234'", true);
    assert_predicate(&e, "${in.body} starts with \"01234\"", true);
    assert_predicate(&e, "${in.body} starts with 01234", false);
}

#[test]
fn test_not_starts_with() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello there");
    assert_predicate(&e, "${in.body} !startsWith 'Bye'", true);
    assert_predicate(&e, "${in.body} !startsWith 'Hello'", false);
    assert_predicate(&e, "${in.body} !startsWith 'B'", true);
    assert_predicate(&e, "${in.body} !startsWith 'H'", false);
    assert_predicate(&e, "${in.body} !startsWith 'Bye there'", true);
    assert_predicate(&e, "${in.body} !startsWith 'Hello there'", false);
    assert_predicate(&e, "${in.body} !startsWith 'Hello ther'", false);
    assert_predicate(&e, "${in.body} !startsWith 'ello there'", true);
    assert_predicate(&e, "${in.body} !startsWith 'Hi'", true);
}

#[test]
fn test_ends_with() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello there");
    assert_predicate(&e, "${in.body} ends with 'there'", true);
    assert_predicate(&e, "${in.body} ends with 're'", true);
    assert_predicate(&e, "${in.body} ends with ' there'", true);
    assert_predicate(&e, "${in.body} ends with 'Hello there'", true);
    assert_predicate(&e, "${in.body} ends with 'Hello ther'", false);
    assert_predicate(&e, "${in.body} ends with 'Hi'", false);
    assert_predicate(&e, "${in.body} endsWith 'there'", true);
    assert_predicate(&e, "${in.body} endsWith 're'", true);
    assert_predicate(&e, "${in.body} endsWith ' there'", true);
    assert_predicate(&e, "${in.body} endsWith 'Hello there'", true);
    assert_predicate(&e, "${in.body} endsWith 'Hello ther'", false);
    assert_predicate(&e, "${in.body} endsWith 'Hi'", false);
}

#[test]
fn test_not_ends_with() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello there");
    assert_predicate(&e, "${in.body} !endsWith 'B'", true);
    assert_predicate(&e, "${in.body} !endsWith 'world'", true);
    assert_predicate(&e, "${in.body} !endsWith 'there'", false);
    assert_predicate(&e, "${in.body} !endsWith 're'", false);
    assert_predicate(&e, "${in.body} !endsWith ' there'", false);
    assert_predicate(&e, "${in.body} !endsWith 'Hello there'", false);
    assert_predicate(&e, "${in.body} !endsWith 'Hello ther'", true);
    assert_predicate(&e, "${in.body} !endsWith 'Hi'", true);
}

#[test]
fn test_equals() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("Account1", "0001");
    e.message_mut().set_header("Account2", "001");
    // == favours numeric comparison when both sides are all digits
    assert_predicate(&e, "${in.header.Account1} == ${in.header.Account2}", true);
    // equals compares as text, so the leading zeros are significant
    assert_predicate(
        &e,
        "${in.header.Account1} equals ${in.header.Account2}",
        false,
    );
    assert_predicate(
        &e,
        "${in.header.Account1} equals ${in.header.Account1}",
        true,
    );
    assert_predicate(&e, "${in.header.Account1} equals '0001'", true);
    assert_predicate(&e, "${in.header.Account1} equals '001'", false);
    assert_predicate(&e, "${in.header.Account1} equals 1", false);

    e.message_mut().set_body("Hello there");
    assert_predicate(&e, "${in.body} equals 'Hello there'", true);
    assert_predicate(&e, "${in.body} equals 'hello there'", false);
    assert_predicate(&e, "${in.body} equals 'Hello'", false);
}

#[test]
fn test_not_equals() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("Account1", "0001");
    e.message_mut().set_header("Account2", "001");
    assert_predicate(
        &e,
        "${in.header.Account1} !equals ${in.header.Account2}",
        true,
    );
    assert_predicate(
        &e,
        "${in.header.Account1} !equals ${in.header.Account1}",
        false,
    );
    assert_predicate(&e, "${in.header.Account1} !equals '0001'", false);

    e.message_mut().set_body("Hello there");
    assert_predicate(&e, "${in.body} !equals 'Hello there'", false);
    assert_predicate(&e, "${in.body} !equals 'hello there'", true);
}

#[test]
fn test_equals_as_literal_text() {
    let (_t, e) = exchange();
    // only parsed as an operator in a predicate, elsewhere it is ordinary text
    assert_expression(
        &e,
        "the value equals something",
        "the value equals something",
    );
    assert_expression(&e, "Logging equals start", "Logging equals start");
}

#[test]
fn test_elvis() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(false);
    assert_predicate(&e, "${body} ?: 'true'", true);
    assert_predicate(&e, "${body} ?: 'false'", false);
    e.message_mut().set_body("Hello");
    assert_predicate(&e, "${body} ?: 'false'", true);
    e.message_mut().set_body(0);
    assert_predicate(&e, "${body} ?: 'true'", true);
    assert_predicate(&e, "${body} ?: 'false'", false);
    e.message_mut().set_body(1);
    assert_predicate(&e, "${body} ?: 'true'", true);
    assert_predicate(&e, "${body} ?: 'false'", true);

    e.message_mut().set_body(Value::Null);
    assert_expression(&e, "${body} ?: 'World'", "World");
    e.message_mut().set_body("");
    assert_expression(&e, "${body} ?: 'World'", "World");
    e.message_mut().set_body("Hello");
    assert_expression(&e, "${body} ?: 'World'", "Hello");
    e.message_mut().set_body(false);
    assert_expression(&e, "${body} ?: 'World'", "World");
    e.message_mut().set_body(true);
    assert_expression(&e, "${body} ?: 'World'", true);
    e.message_mut().set_header("myHeader", "Camel");
    assert_expression(&e, "${header.myHeader} ?: 'World'", "Camel");
    e.message_mut().set_body(0);
    assert_expression(&e, "${body} ?: 'World'", "World");
    e.message_mut().set_body(1);
    assert_expression(&e, "${body} ?: 'World'", 1);
}

#[test]
fn test_ternary() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(false);
    assert_predicate(&e, "${body == true ? 'true' : 'false'}", false);
    assert_predicate(&e, "${body == true ? 'false' : 'true'}", true);
    e.message_mut().set_body("Hello");
    assert_predicate(&e, "${body != null ? 'true' : 'false'}", true);
    assert_predicate(&e, "${body != null ? 'false' : 'true'}", false);
    e.message_mut().set_body(0);
    assert_predicate(&e, "${body == 0 ? 'true' : 'false'}", true);
    assert_predicate(&e, "${body == 0 ? 'false' : 'true'}", false);
    e.message_mut().set_body(1);
    assert_predicate(&e, "${body > 0 ? 'true' : 'false'}", true);
    assert_predicate(&e, "${body > 0 ? 'false' : 'true'}", false);

    e.message_mut().set_body(Value::Null);
    assert_expression(&e, "${body != null ? 'A' : 'B'}", "B");
    e.message_mut().set_body("");
    assert_expression(&e, "${body == '' ? 'A' : 'B'}", "A");
    e.message_mut().set_body("Hello");
    assert_expression(&e, "${body != 'Hello' ? 'A' : 'B'}", "B");
    e.message_mut().set_body(false);
    assert_expression(&e, "${body == true ? 'A' : 'B'}", "B");
    e.message_mut().set_body(false);
    assert_expression(&e, "${body != true ? 'A' : 'B'}", "A");
}

#[test]
fn test_ternary_with_compound_condition() {
    let (_t, mut e) = exchange();
    // CAMEL-24920: the condition of a ternary may be more than one comparison
    e.message_mut().set_body(5);
    assert_expression(&e, "${body > 0 && body < 10 ? 'in' : 'out'}", "in");
    assert_expression(&e, "${body > 0 || body > 100 ? 'in' : 'out'}", "in");
    assert_expression(
        &e,
        "${body > 0 && body < 10 && body != 7 ? 'in' : 'out'}",
        "in",
    );

    e.message_mut().set_body(50);
    assert_expression(&e, "${body > 0 && body < 10 ? 'in' : 'out'}", "out");
    assert_expression(&e, "${body > 0 || body > 100 ? 'in' : 'out'}", "in");

    e.message_mut().set_body("Hello");
    assert_expression(
        &e,
        "${body != null && body contains 'ell' ? 'yes' : 'no'}",
        "yes",
    );
    assert_expression(
        &e,
        "${body != null && body contains 'xxx' ? 'yes' : 'no'}",
        "no",
    );
}

#[test]
fn test_ternary_with_compound_condition_corner_cases() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(5);
    // a quoted value that holds the operator text is not a logical operator
    assert_expression(
        &e,
        "${body > 0 && body < 10 ? 'in && out' : 'no'}",
        "in && out",
    );
    e.message_mut().set_body("a && b");
    assert_expression(&e, "${body contains 'a && b' ? 'yes' : 'no'}", "yes");
    // simple has no word forms, and says so, as it does outside a ternary
    e.message_mut().set_body(5);
    let simple = e.context().resolve_language("simple").unwrap();
    let err = simple
        .create_expression("${body > 0 and body < 10 ? 'in' : 'out'}")
        .err()
        .expect("should fail");
    assert!(err.to_string().contains("use && for and"), "{err}");
}

#[test]
fn test_ternary_value_forms() {
    let (_t, mut e) = exchange();
    // a value form the ternary accepts must be used as that value and not looked up as a function,
    // which is what an unquoted number or boolean used to be (CAMEL-24826)
    e.message_mut().set_body(5);
    assert_expression(&e, "${body > 0 ? 1 : 0}", 1);
    assert_expression(&e, "${body > 0 ? true : false}", true);
    assert_expression(&e, "${body > 0 ? 1.5 : 2.5}", 1.5);
    assert_expression(&e, "${body > 0 ? -7 : 7}", -7);
    assert_expression(&e, "${body > 0 ? ${body} : 0}", 5);

    // and the other branch is reached just as well
    e.message_mut().set_body(-1);
    assert_expression(&e, "${body > 0 ? 1 : 0}", 0);
    assert_expression(&e, "${body > 0 ? true : false}", false);
    assert_expression(&e, "${body > 0 ? 1.5 : 2.5}", 2.5);
    assert_expression(&e, "${body > 0 ? ${body} : 0}", 0);
}

#[test]
fn test_ternary_log() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(
        &e,
        ">>> Message received from WebSocket Client : ${body}",
        ">>> Message received from WebSocket Client : Hello World",
    );

    e.message_mut().set_header(FILE_NAME, "foo.txt");
    assert_expression(
        &e,
        "This is a test bug ${header.CamelFileName}",
        "This is a test bug foo.txt",
    );

    assert_expression(
        &e,
        "This is a test bug : ${header.CamelFileName}",
        "This is a test bug : foo.txt",
    );

    assert_expression(
        &e,
        "This is a test bug ? ${header.CamelFileName}",
        "This is a test bug ? foo.txt",
    );
}

#[test]
fn test_chain() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::Null);
    assert_expression(
        &e,
        "${substringAfter('Hello')} ~> ${trim()} ~> ${uppercase()}",
        Value::Null,
    );
    e.message_mut().set_body("   Hello    World   ");
    assert_expression(
        &e,
        "${substringAfter('Hello')} ~> ${trim()} ~> ${uppercase()}",
        "WORLD",
    );
    // run 2nd time give same result
    assert_expression(
        &e,
        "${substringAfter('Hello')} ~> ${trim()} ~> ${uppercase()}",
        "WORLD",
    );

    e.message_mut().set_body("  Hello    World   ");
    let predicate = create_predicate(
        &e,
        "${substringAfter('Hello')} ~> ${trim()} ~> ${uppercase()} == 'WORLD'",
    );
    assert!(predicate.matches(&e).unwrap());
    // run 2nd time give same result
    assert!(predicate.matches(&e).unwrap());

    e.message_mut().set_body("  Hello    Camel   ");
    let predicate = create_predicate(
        &e,
        "${substringAfter('Hello')} ~> ${trim()} ~> ${uppercase()} == 'WORLD'",
    );
    assert!(!predicate.matches(&e).unwrap());
}

#[test]
fn test_chain_null_safe() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::Null);
    assert_expression(
        &e,
        "${substringAfter('Hello')} ?~> ${collate(2)} ~> ${uppercase()}",
        Value::Null,
    );
    // run 2nd time give same result
    assert_expression(
        &e,
        "${substringAfter('Hello')} ?~> ${collate(2)} ~> ${uppercase()}",
        Value::Null,
    );

    let predicate = create_predicate(
        &e,
        "${substringAfter('Hello')} ?~> ${collate(2)} ~> ${uppercase()}",
    );
    assert!(!predicate.matches(&e).unwrap());
    // run 2nd time give same result
    assert!(!predicate.matches(&e).unwrap());

    e.message_mut().set_body("Hello World,Hello Camel");
    // Java collate returns an iterator (kindOfType "object"); the Rust collate returns the
    // groups as a list, an "array"
    assert_expression(
        &e,
        "${substringAfter('Hello')} ?~> ${collate(2)} ~> ${kindOfType()}",
        "array",
    );
    // run 2nd time give same result
    assert_expression(
        &e,
        "${substringAfter('Hello')} ?~> ${collate(2)} ~> ${kindOfType()}",
        "array",
    );

    let predicate = create_predicate(
        &e,
        "${substringAfter('Hello')} ?~> ${collate(2)} ~> ${kindOfType()} == 'array'",
    );
    assert!(predicate.matches(&e).unwrap());
    // run 2nd time give same result
    assert!(predicate.matches(&e).unwrap());
}

#[test]
fn test_chain_param() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("   Hello World from the Camel   ");
    // no param
    assert_expression(
        &e,
        "${trim()} ~> ${replace('Hello','Hi')}",
        "Hi World from the Camel",
    );
    assert_expression(
        &e,
        "${trim()} ~> ${replace('Hello','Hi')} ~> ${split(' ')} ~> ${size()}",
        5,
    );
    // with $param
    assert_expression(
        &e,
        "${trim()} ~> ${replace('Hello','Hi',$param)}",
        "Hi World from the Camel",
    );
    assert_expression(
        &e,
        "${trim()} ~> ${replace('Hello','Hi',$param)} ~> ${split($param,' ')} ~> ${size($param)}",
        5,
    );
}

#[test]
fn test_digital_string_too_big_for_long() {
    let (_t, mut e) = exchange();
    // CAMEL-24407: numbers such as bank account numbers have more digits than a long can hold
    e.message_mut()
        .set_header("Account1", "12345678901234567890");
    e.message_mut()
        .set_header("Account2", "12345678901234567890");
    e.message_mut()
        .set_header("Account3", "12345678901234567891");

    assert_predicate(&e, "${header.Account1} == ${header.Account2}", true);
    assert_predicate(&e, "${header.Account1} == ${header.Account3}", false);
    assert_predicate(&e, "${header.Account1} != ${header.Account3}", true);
    assert_predicate(&e, "${header.Account1} < ${header.Account3}", true);
    assert_predicate(&e, "${header.Account3} > ${header.Account1}", true);
    assert_predicate(&e, "${header.Account1} == 12345678901234567890", true);
    assert_predicate(&e, "${header.Account1} == '12345678901234567890'", true);
    assert_predicate(&e, "${header.Account1} > 7", true);
}

#[test]
fn test_numeric_literal_as_other_number_type() {
    let (_t, mut e) = exchange();
    // CAMEL-24966: a numeric literal evaluated as another number type is converted, not cast
    e.message_mut().set_body(5);
    let v = evaluate(&e, "${body > 0 ? 1 : 0}");
    assert_eq!(Some(1i64), i64::from_value(&v).unwrap());
    assert_eq!(Some(1i32), i32::from_value(&v).unwrap());
    assert_eq!(Some(1.0f64), f64::from_value(&v).unwrap());
    assert_eq!(Some("1".to_string()), String::from_value(&v).unwrap());
}

#[test]
fn test_negative_long_literal() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("x", 5);
    assert_predicate(&e, "${header.x} > -3000000000", true);
    assert_predicate(&e, "${header.x} < -3000000000", false);
    assert_predicate(&e, "${header.x} > -2147483648", true);
}

#[test]
fn test_elvis_any_zero() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("count", 0);
    assert_expression(&e, "${header.count} ?: 'none'", "none");
    e.message_mut().set_header("count", 0.0);
    assert_expression(&e, "${header.count} ?: 'none'", "none");
    e.message_mut()
        .set_header("count", 0.0 /* Java BigDecimal.ZERO */);
    assert_expression(&e, "${header.count} ?: 'none'", "none");
    e.message_mut().set_header("count", -0.0);
    assert_expression(&e, "${header.count} ?: 'none'", "none");
    // NaN is not zero
    e.message_mut().set_header("count", f64::NAN);
    let v = evaluate(&e, "${header.count} ?: 'none'");
    assert!(matches!(v, Value::Float(f) if f.is_nan()), "{v:?}");
    e.message_mut().set_header("count", 3);
    assert_expression(&e, "${header.count} ?: 'none'", 3);
}

#[test]
fn test_logical_expression_to_string() {
    let (_t, e) = exchange();
    let simple = e.context().resolve_language("simple").unwrap();
    let exp = simple
        .create_expression("${body != null && body.size() > 0}")
        .unwrap();
    assert_eq!("${body} != null && ${body.size()} > 0", exp.describe());
}

#[test]
fn test_elvis_evaluates_left_once() {
    let (t, e) = exchange();
    // Java MyCounter
    let count = Arc::new(AtomicI64::new(0));
    let c = count.clone();
    let counter = MethodBean::new("MyCounter").method("next", move |_| {
        Ok(Value::Int(c.fetch_add(1, Ordering::SeqCst) + 1))
    });
    t.context
        .registry()
        .unwrap()
        .bind("counter", counter.into_value())
        .unwrap();
    assert_expression(&e, "${bean:counter?method=next} ?: 'none'", 1);
    assert_eq!(1, count.load(Ordering::SeqCst));
}

#[test]
fn test_unary_inc_dec_keeps_decimals() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("price", 1.5);
    assert_expression(&e, "${header.price}++", 2.5);
    assert_expression(&e, "${header.price}--", 0.5);
    e.message_mut().set_header("price", 1.25);
    assert_expression(&e, "${header.price}++", 2.25);
    e.message_mut().set_header("price", "1.5");
    assert_expression(&e, "${header.price}++", "2.5");
    e.message_mut().set_header("price", 7);
    assert_expression(&e, "${header.price}--", 6);
}

#[test]
fn test_regex_and_range_with_null_right_hand_side() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(5);
    assert_predicate(&e, "${body} range ${header.none}", false);
    assert_predicate(&e, "${body} !range ${header.none}", true);
    assert_predicate(&e, "${body} regex ${header.none}", false);
    assert_predicate(&e, "${body} !regex ${header.none}", true);
}

// Rust tests of the chain, elvis and ternary operators (behaviour the Java tests do not pin)

#[test]
fn test_ternary_in_predicate() {
    let (_t, e) = exchange();
    assert_predicate(&e, "${header.bar} > 100 ? 'true' : 'false'", true);
    assert_predicate(&e, "${header.bar} < 100 ? 'true' : 'false'", false);
    assert_predicate(
        &e,
        "${header.foo} == 'abc' ? ${header.bar} == 123 : false",
        true,
    );
}

#[test]
fn test_elvis_nested() {
    let (_t, e) = exchange();
    assert_expression(&e, "${header.none} ?: ${header.none2} ?: 'x'", "x");
    assert_expression(&e, "${header.none} ?: ${header.foo} ?: 'x'", "abc");
    assert_expression(&e, "${header.none} ?: 123", 123);
}

#[test]
fn test_chain_restores_the_body() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("  Hello World  ");
    let simple = e.context().resolve_language("simple").unwrap();
    let exp = simple
        .create_expression("${trim()} ~> ${uppercase()}")
        .unwrap();
    assert_eq!(Value::from("HELLO WORLD"), exp.evaluate(&e).unwrap());
    assert_eq!(
        Value::from("HELLO WORLD"),
        exp.evaluate_mut(&mut e).unwrap()
    );
    assert_eq!(&Value::from("  Hello World  "), e.message().body());
    // the chain variable of $param is removed after use
    let exp = simple
        .create_expression("${trim()} ~> ${replace('Hello','Hi',$param)}")
        .unwrap();
    assert_eq!(Value::from("Hi World"), exp.evaluate_mut(&mut e).unwrap());
    assert!(e.variable("CamelSimpleChainParam").is_none());
    assert_eq!(
        "${trim()} ~> ${replace('Hello','Hi',$param)}",
        exp.describe()
    );
}

#[test]
fn test_other_operators_need_an_operand() {
    let (_t, e) = exchange();
    let simple = e.context().resolve_language("simple").unwrap();
    for text in ["${body} ?: World", "${body} ~> trim()"] {
        let err = simple.create_expression(text).err().expect(text);
        assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err:?}");
    }
}
