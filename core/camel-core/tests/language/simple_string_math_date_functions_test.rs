//! Port of `SimpleStringMathDateFunctionsTest`.

use super::simple_test::{assert_expression, evaluate, exchange};
use camel_core::prelude::*;
use camel_core::util::date::Date;
use camel_core::util::value::ValueMap;

#[test]
fn test_capitalize_empty() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("");
    assert_expression(&e, "${capitalize()}", "");
}

#[test]
fn test_math_without_arguments_uses_the_body() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(Value::List(vec![10.into(), 20.into(), 30.into()]));
    assert_expression(&e, "${sum()}", 60);
    assert_expression(&e, "${max()}", 30);
    assert_expression(&e, "${min()}", 10);
    assert_expression(&e, "${average()}", 20);
}

#[test]
fn test_date_header_with_dash_or_dot() {
    let (_t, mut e) = exchange();
    let date = Date::new(0);
    for name in ["Last-Modified", "X-Date", "my.date"] {
        e.message_mut().set_header(name, date);
    }
    assert_eq!(
        date.to_value(),
        evaluate(&e, "${date:header.Last-Modified}")
    );
    assert_eq!(date.to_value(), evaluate(&e, "${date:header.X-Date}"));
    assert_eq!(date.to_value(), evaluate(&e, "${date:header.my.date}"));
    // an offset still works after a name with a dash
    assert_eq!(
        Date::new(1000).to_value(),
        evaluate(&e, "${date:header.Last-Modified+1s}")
    );
    assert_eq!(
        Date::new(-1000).to_value(),
        evaluate(&e, "${date:header.Last-Modified - 1s}")
    );
}

#[test]
fn test_date_with_timezone_offset() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("d", Date::new(0));
    assert_expression(
        &e,
        "${date-with-timezone:header.d:GMT+02:00:HH:mm}",
        "02:00",
    );
    assert_expression(&e, "${date-with-timezone:header.d:UTC:HH:mm}", "00:00");
}

#[test]
fn test_safe_quote_is_valid_json() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("He said \"hi\"");
    assert_expression(&e, "${safeQuote()}", "\"He said \\\"hi\\\"\"");
    e.message_mut().set_body("C:\\temp");
    assert_expression(&e, "${safeQuote()}", "\"C:\\\\temp\"");
    // already quoted once is not quoted twice
    e.message_mut().set_body("\"Hello\"");
    assert_expression(&e, "${safeQuote()}", "\"Hello\"");
    // numbers are not quoted (Java BigDecimal 12.50 is a decimal here)
    e.message_mut().set_body(12.5);
    assert_eq!(Value::Float(12.5), evaluate(&e, "${safeQuote()}"));
}

#[test]
fn test_kind_of_type() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(1.5);
    assert_expression(&e, "${kindOfType()}", "number");
    let mut m = ValueMap::new();
    m.insert("a".to_string(), 1.into());
    e.message_mut().set_body(Value::Map(m));
    assert_expression(&e, "${kindOfType()}", "object");
    e.message_mut().set_body(Value::List(vec!["a".into()]));
    assert_expression(&e, "${kindOfType()}", "array");
}

#[test]
fn test_substring_with_null_arguments() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("abc");
    let simple = e.context().resolve_language("simple").unwrap();
    let err = simple
        .create_expression("${substring(${header.none})}")
        .unwrap()
        .evaluate(&e)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("substring number expression evaluated to null"),
        "{err}"
    );
    // no delimiter then nothing comes before/after/between
    assert_expression(&e, "${substringBefore(${header.none})}", Value::Null);
    assert_expression(&e, "${substringAfter(${header.none})}", Value::Null);
    assert_expression(&e, "${substringBetween(${header.none},'c')}", Value::Null);
}

#[test]
fn test_pad_with_several_characters() {
    let (_t, e) = exchange();
    assert_expression(&e, "${pad('foo',6,'ab')}", "fooaba");
    assert_expression(&e, "${pad('foo',-6,'ab')}", "abafoo");
    assert_expression(&e, "${pad('42',-5,'0')}", "00042");
    assert_expression(&e, "${pad('Hi',4)}", "Hi  ");
}

#[test]
fn test_size_of_any_array() {
    // Java arrays are lists here; ${length()} of a list is its text length (as a Java List)
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(Value::List(vec![1.into(), 2.into(), 3.into()]));
    assert_expression(&e, "${size()}", 3);
    e.message_mut()
        .set_body(Value::List(vec!["a".into(), "b".into()]));
    assert_expression(&e, "${size()}", 2);
    e.message_mut().set_body(Value::List(vec![true.into()]));
    assert_expression(&e, "${size()}", 1);
}

#[test]
fn test_hidden_file_name() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("CamelFileName", ".bashrc");
    assert_expression(&e, "${file:name.noext}", ".bashrc");
    assert_expression(&e, "${file:ext}", Value::Null);
    e.message_mut().set_header("CamelFileName", ".route.yaml");
    assert_expression(&e, "${file:name.noext}", ".route");
    assert_expression(&e, "${file:ext}", "yaml");
}
