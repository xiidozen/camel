//! The date functions of the simple language (Java `DateFunctionFactory` and
//! `DateExpressionBuilder`): `${date:command}`, `${date:command:pattern}` and
//! `${date-with-timezone:command:timezone:pattern}`.
//!
//! The command is `now`, `millis`, `exchangeCreated`, `header.name`, `variable.name`,
//! `exchangeProperty.name` or `file` (the last modified time of the file being consumed),
//! with optional offsets such as `now-2h` or `header.birthday + 24h`. Without a pattern the
//! result is a [`Date`] (`millis` is a number), with a pattern the text formatted with a Java
//! `SimpleDateFormat` pattern.

use super::functions::if_starts_with_return_remainder;
use super::{ParseResult, ParserError};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::FILE_LAST_MODIFIED;
use camel_api::processor::SharedExpression;
use camel_support::builder::named_expression;
use camel_util::date::{Date, SimpleDateFormat, TimeZone};
use camel_util::value::Value;
use regex::Regex;
use std::sync::OnceLock;

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// The date functions (Java `DateFunctionFactory`).
///
/// # Errors
///
/// A syntax error for `date-with-timezone` without its three parts.
pub fn date_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(remainder) = if_starts_with_return_remainder("date-with-timezone:", function) {
        let parts: Vec<&str> = remainder.splitn(3, ':').collect();
        if parts.len() < 3 {
            return Err(ParserError::new(
                format!(
                    "Valid syntax: ${{date-with-timezone:command:timezone:pattern}} was: {function}"
                ),
                index,
            ));
        }
        // a timezone with an offset holds a colon itself, such as GMT+02:00
        static WITH_COLON: OnceLock<Regex> = OnceLock::new();
        let rest = &remainder[parts[0].len() + 1..];
        if let Some(c) =
            regex(&WITH_COLON, r"^((?:GMT|UTC)?[+-]\d{1,2}:\d{2}):(.+)$").captures(rest)
        {
            return Ok(Some(date_expression(parts[0], Some(&c[1]), Some(&c[2]))));
        }
        return Ok(Some(date_expression(
            parts[0],
            Some(parts[1]),
            Some(parts[2]),
        )));
    }
    if let Some(remainder) = if_starts_with_return_remainder("date:", function) {
        return Ok(Some(match remainder.split_once(':') {
            None => date_expression(remainder, None, None),
            Some((command, pattern)) => date_expression(command, None, Some(pattern)),
        }));
    }
    Ok(None)
}

/// The expression of the date command with its offsets, formatted with the pattern (Java
/// `DateExpressionBuilder.dateExpression`).
pub fn date_expression(
    command_with_offsets: &str,
    timezone: Option<&str>,
    pattern: Option<&str>,
) -> SharedExpression {
    // an offset starts with a digit, so a name such as header.Last-Modified is not an offset
    static OFFSET_START: OnceLock<Regex> = OnceLock::new();
    static OFFSET: OnceLock<Regex> = OnceLock::new();
    let command = match regex(&OFFSET_START, r"[+-]\s*\d").find(command_with_offsets) {
        Some(m) => &command_with_offsets[..m.start()],
        None => command_with_offsets,
    }
    .trim()
    .to_string();
    // the offsets are checked when evaluated, as Java does
    let offsets: Vec<Result<i64, String>> = regex(&OFFSET, r"([+-])\s*(\d[^+-]*)")
        .captures_iter(command_with_offsets)
        .map(|c| {
            camel_util::time_utils::to_milli_seconds(c[2].trim()).map(|v| {
                if &c[1] == "+" {
                    v
                } else {
                    -v
                }
            })
        })
        .collect();
    let description = match (timezone, pattern) {
        (Some(tz), Some(p)) => format!("date({command_with_offsets}:{tz}:{p})"),
        (_, Some(p)) => format!("date({command_with_offsets}:{p})"),
        _ => format!("date({command_with_offsets})"),
    };
    let timezone = timezone.filter(|t| !t.is_empty()).map(str::to_string);
    let pattern = pattern.filter(|p| !p.is_empty()).map(str::to_string);
    named_expression(description, move |e| {
        let date = eval_date(e, &command)?;
        let mut millis = date.millis();
        for offset in &offsets {
            millis += offset.clone().map_err(CamelError::illegal_argument)?;
        }
        let date = Date::new(millis);
        match &pattern {
            Some(p) => {
                let mut df = SimpleDateFormat::new(p).map_err(CamelError::illegal_argument)?;
                if let Some(tz) = &timezone {
                    df = df.with_time_zone(TimeZone::get(tz));
                }
                Ok(Value::from(df.format(&date)))
            }
            None if command == "millis" => Ok(Value::Int(date.millis())),
            None => Ok(date.to_value()),
        }
    })
}

/// The date of the command (Java `DateExpressionBuilder.evalDate`).
fn eval_date(e: &Exchange, command: &str) -> CamelResult<Date> {
    let keyed = |obj: Option<&Value>| -> CamelResult<Date> {
        obj.and_then(Date::from_value).ok_or_else(|| {
            CamelError::illegal_argument(format!(
                "Cannot find Date/long object at command: {command}"
            ))
        })
    };
    // the key may contain dots, such as header.my.date
    let key = || &command[command.find('.').map_or(0, |i| i + 1)..];
    if command == "now" || command == "millis" {
        Ok(Date::now())
    } else if command == "exchangeCreated" {
        Ok(Date::new(e.created()))
    } else if command.starts_with("header.") {
        keyed(e.message().header(key()))
    } else if command.starts_with("variable.") {
        keyed(e.variable(key()))
    } else if command.starts_with("exchangeProperty.") {
        keyed(e.property(key()))
    } else if command == "file" {
        // Java LanguageHelper.dateFromFileLastModified
        let header = e.message().header(FILE_LAST_MODIFIED);
        match header.and_then(Date::from_value) {
            Some(d) if d.millis() > 0 || !matches!(header, Some(Value::Int(_))) => Ok(d),
            _ => Err(CamelError::illegal_argument(format!(
                "Cannot find {FILE_LAST_MODIFIED} header at command: {command}"
            ))),
        }
    } else {
        Err(CamelError::illegal_argument(format!(
            "Command not supported for dateExpression: {command}"
        )))
    }
}
