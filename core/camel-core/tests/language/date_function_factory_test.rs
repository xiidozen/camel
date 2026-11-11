//! Port of `functions/DateFunctionFactoryTest`: `${date:..}` and `${date-with-timezone:..}`.
//!
//! Java `Calendar.getInstance()` dates are in the default time zone: [`Date::of`] with
//! [`TimeZone::default_zone`]. `testDateWithConverterExpressions` (a custom type converter
//! of the dates) waits for the type converter registry.

use super::simple_test::{evaluate, exchange};
use camel_core::prelude::*;
use camel_core::util::date::{Date, TimeZone};

fn local(year: i64, month: u32, day: u32) -> Date {
    Date::of(&TimeZone::default_zone(), year, month, day, 0, 0, 0, 0)
}

fn evaluate_err(e: &Exchange, text: &str) -> CamelError {
    let simple = e.context().resolve_language("simple").unwrap();
    simple
        .create_expression(text)
        .unwrap()
        .evaluate(e)
        .unwrap_err()
}

#[test]
fn test_date_expressions() {
    let (_t, mut e) = exchange();
    let birthday = local(1974, 4, 20);
    e.message_mut().set_header("birthday", birthday);
    let property = local(1976, 6, 22);
    e.set_property("birthday", property);

    assert_eq!(birthday.to_value(), evaluate(&e, "${date:header.birthday}"));
    assert_eq!("19740420", evaluate(&e, "${date:header.birthday:yyyyMMdd}"));
    assert_eq!(
        "19740421",
        evaluate(&e, "${date:header.birthday+24h:yyyyMMdd}")
    );
    // as long
    let v = evaluate(&e, "${date:exchangeProperty.birthday}");
    assert_eq!(
        Some(property.millis()),
        <i64 as camel_core::api::convert::FromValue>::from_value(&v).unwrap()
    );
    // as date
    assert_eq!(property.to_value(), v);
    assert_eq!(
        "19760622",
        evaluate(&e, "${date:exchangeProperty.birthday:yyyyMMdd}")
    );
    assert_eq!(
        "19760623",
        evaluate(&e, "${date:exchangeProperty.birthday+24h:yyyyMMdd}")
    );

    let err = evaluate_err(&e, "${date:yyyyMMdd}");
    assert!(err.is(ErrorKind::IllegalArgument));
    assert_eq!(
        Some("Command not supported for dateExpression: yyyyMMdd"),
        err.message()
    );
}

#[test]
fn test_date_and_time_expressions() {
    let (_t, mut e) = exchange();
    let zone = TimeZone::default_zone();
    e.message_mut()
        .set_header("birthday", Date::of(&zone, 1974, 4, 20, 8, 55, 47, 123));
    assert_eq!(
        "1974-04-20T08:55:37:123",
        evaluate(
            &e,
            "${date:header.birthday - 10s:yyyy-MM-dd'T'HH:mm:ss:SSS}"
        )
    );
    assert_eq!(
        "1974-04-20T08:55:47:123",
        evaluate(&e, "${date:header.birthday:yyyy-MM-dd'T'HH:mm:ss:SSS}")
    );
}

#[test]
fn test_date_with_a_header_that_is_not_a_date() {
    // the error part of Java testDateWithConverterExpressions
    let (_t, mut e) = exchange();
    e.message_mut().set_header("other", Value::List(vec![]));
    let err = evaluate_err(&e, "${date:header.other:yyyyMMdd}");
    assert_eq!(
        Some("Cannot find Date/long object at command: header.other"),
        err.message()
    );
}

#[test]
fn test_date_with_timezone() {
    let (_t, mut e) = exchange();
    let zone = TimeZone::get("GMT+8");
    e.message_mut()
        .set_header("birthday", Date::of(&zone, 1974, 4, 20, 8, 55, 47, 123));
    assert_eq!(
        "1974-04-20T08:55:47:123",
        evaluate(
            &e,
            "${date-with-timezone:header.birthday:GMT+8:yyyy-MM-dd'T'HH:mm:ss:SSS}"
        )
    );
    assert_eq!(
        "1974-04-20T00:55:47:123",
        evaluate(
            &e,
            "${date-with-timezone:header.birthday:GMT:yyyy-MM-dd'T'HH:mm:ss:SSS}"
        )
    );
}

#[test]
fn test_date_now() {
    let (_t, e) = exchange();
    let out = evaluate(&e, "${date:now}");
    assert!(Date::from_value(&out).is_some() && matches!(out, Value::Object(_)));
    for exp in [
        "${date:now:hh:mm:ss a}",
        "${date:now:hh:mm:ss}",
        "${date:now-2h:hh:mm:ss}",
    ] {
        assert!(!evaluate(&e, exp).is_null());
    }
}

#[test]
fn test_date_millis() {
    let (_t, e) = exchange();
    assert!(matches!(evaluate(&e, "${date:millis}"), Value::Int(_)));
}

#[test]
fn test_date_exchange_created() {
    let (_t, e) = exchange();
    assert!(!evaluate(&e, "${date:exchangeCreated:hh:mm:ss a}").is_null());
}
