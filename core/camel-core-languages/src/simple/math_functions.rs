//! The math functions of the simple language (Java `MathFunctionFactory` and
//! `MathExpressionBuilder`): `${abs()}`, `${floor()}`, `${ceil()}`, `${sum(..)}`, `${max(..)}`,
//! `${min(..)}` and `${average(..)}`.

use super::functions::if_starts_with_return_remainder;
use super::string_functions::{single_argument, value_of, Lazy};
use super::ParseResult;
use camel_api::convert::FromValue;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::SharedExpression;
use camel_support::builder::named_expression;
use camel_util::string_helper::before_last;
use camel_util::string_quote_helper::split_safe_quote_full;
use camel_util::value::Value;

/// The items of a value: a list, the comma separated parts of a text, or the value itself
/// (Java `ObjectHelper.createIterable`).
pub(crate) fn iterate(value: Value) -> Vec<Value> {
    match value {
        Value::Null => vec![],
        Value::List(l) => l,
        Value::Map(m) => m.into_values().collect(),
        Value::Text(t) => t
            .split(',')
            .map(|s| Value::from(s.trim()))
            .filter(|v| v.as_str() != Some(""))
            .collect(),
        other => vec![other],
    }
}

fn longs(exps: &[Lazy], e: &Exchange) -> CamelResult<Vec<i64>> {
    let mut out = Vec::new();
    for exp in exps {
        for item in iterate(exp.evaluate(e)?) {
            if let Ok(Some(v)) = i64::from_value(&item) {
                out.push(v);
            }
        }
    }
    Ok(out)
}

/// A function of the numbers of its arguments.
type VariadicFunction = (&'static str, fn(&[i64]) -> Option<i64>);

const VARIADIC_FUNCTIONS: &[VariadicFunction] = &[
    ("sum", |v| {
        if v.is_empty() {
            None
        } else {
            Some(v.iter().sum())
        }
    }),
    ("max", |v| v.iter().copied().max()),
    ("min", |v| v.iter().copied().min()),
    ("average", |v| {
        if v.is_empty() {
            None
        } else {
            Some(v.iter().sum::<i64>() / v.len() as i64)
        }
    }),
];

fn variadic(remainder: &str) -> Vec<Lazy> {
    match before_last(remainder, ")").filter(|v| !v.is_empty()) {
        Some(values) => split_safe_quote_full(&values, ',', true, false, false)
            .iter()
            .map(|t| Lazy::new(t))
            .collect(),
        None => vec![Lazy::new("${body}")],
    }
}

fn description(name: &str, exps: &[Lazy]) -> String {
    let texts: Vec<&str> = exps.iter().map(|e| e.text()).collect();
    format!("{name}([{}])", texts.join(", "))
}

/// Creates the math function, if the function is one.
///
/// # Errors
///
/// Never fails for now (the Java factory has no syntax errors).
pub fn math_function(function: &str, _index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(r) = if_starts_with_return_remainder("abs(", function) {
        let exp = single_argument(r);
        let d = match &exp {
            Some(x) => format!("abs({})", x.text()),
            None => "abs()".to_string(),
        };
        return Ok(Some(named_expression(d, move |e| {
            Ok(i64::from_value(&value_of(&exp, e)?)?
                .map(|v| Value::Int(v.abs()))
                .unwrap_or_default())
        })));
    }
    for (name, ceil) in [("floor", false), ("ceil", true)] {
        if let Some(r) = function
            .strip_prefix(name)
            .and_then(|r| if_starts_with_return_remainder("(", r))
        {
            let exp = single_argument(r);
            let d = match &exp {
                Some(x) => format!("{name}({})", x.text()),
                None => format!("{name}()"),
            };
            return Ok(Some(named_expression(d, move |e| {
                Ok(f64::from_value(&value_of(&exp, e)?)?
                    .map(|v| Value::Int(if ceil { v.ceil() } else { v.floor() } as i64))
                    .unwrap_or_default())
            })));
        }
    }
    for (name, f) in VARIADIC_FUNCTIONS {
        if let Some(r) = function
            .strip_prefix(name)
            .and_then(|r| if_starts_with_return_remainder("(", r))
        {
            let exps = variadic(r);
            return Ok(Some(named_expression(description(name, &exps), move |e| {
                Ok((*f)(&longs(&exps, e)?).map(Value::Int).unwrap_or_default())
            })));
        }
    }
    Ok(None)
}
