//! The smaller function factories of the simple language (Java `JoinFunctionFactory`,
//! `CollateFunctionFactory`, `SkipFunctionFactory`, `RandomFunctionFactory` and
//! `SystemFunctionFactory`): `${join(..)}`, `${collate(n)}`, `${skip(n)}`, `${random(..)}`,
//! `${sys.name}` and `${env.NAME}`.

use super::functions::if_starts_with_return_remainder;
use super::math_functions::iterate;
use super::string_functions::Lazy;
use super::{ParseResult, ParserError};
use camel_api::convert::FromValue;
use camel_api::error::CamelError;
use camel_api::processor::SharedExpression;
use camel_support::builder::named_expression;
use camel_util::string_helper::before_last;
use camel_util::string_quote_helper::split_safe_quote_full;
use camel_util::value::Value;

/// Creates the function, if the function is one of these factories.
///
/// # Errors
///
/// Returns a syntax error for invalid arguments.
pub fn other_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(r) = if_starts_with_return_remainder("join(", function) {
        let (mut separator, mut prefix, mut exp) = (",".to_string(), None, "${body}".to_string());
        if let Some(values) = before_last(r, ")").filter(|v| !v.is_empty()) {
            let tokens = split_safe_quote_full(&values, ',', false, false, false);
            match tokens.len() {
                1 => separator = tokens[0].clone(),
                2 => {
                    separator = tokens[0].clone();
                    prefix = Some(tokens[1].clone());
                }
                3 => {
                    separator = tokens[0].clone();
                    prefix = Some(tokens[1].clone());
                    exp = tokens[2].clone();
                }
                _ => {
                    return Err(ParserError::new(
                        format!(
                            "Valid syntax: ${{join(separator,prefix,expression)}} was: {function}"
                        ),
                        index,
                    ))
                }
            }
        }
        let d = match &prefix {
            Some(p) => format!("join({exp},{separator},{p})"),
            None => format!("join({exp},{separator})"),
        };
        let x = Lazy::new(&exp);
        return Ok(Some(named_expression(d, move |e| {
            let mut sb = String::new();
            let mut first = true;
            for item in iterate(x.evaluate(e)?) {
                if item.is_null() {
                    continue;
                }
                if let Some(s) = String::from_value(&item)? {
                    if !first {
                        sb.push_str(&separator);
                    }
                    first = false;
                    if let Some(p) = &prefix {
                        sb.push_str(p);
                    }
                    sb.push_str(&s);
                }
            }
            Ok(Value::from(sb))
        })));
    }
    for (name, collate) in [("collate", true), ("skip", false)] {
        if let Some(r) = function
            .strip_prefix(name)
            .and_then(|r| if_starts_with_return_remainder("(", r))
        {
            let values = before_last(r, ")")
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    ParserError::new(
                        format!(
                            "Valid syntax: ${{{name}({})}} was: {function}",
                            if collate { "group" } else { "number" }
                        ),
                        index,
                    )
                })?;
            let n = Lazy::new(values.trim());
            let body = Lazy::new("${body}");
            return Ok(Some(named_expression(
                format!("{name}(${{body}},{})", values.trim()),
                move |e| {
                    let Some(count) = i64::from_value(&n.evaluate(e)?)? else {
                        return Err(CamelError::illegal_argument(format!(
                            "{name} number expression evaluated to null: {}",
                            n.text()
                        )));
                    };
                    let items = iterate(body.evaluate(e)?);
                    Ok(Value::List(if collate {
                        // Java groupIteratorExpression: lists of the group size
                        let size = count.max(1) as usize;
                        items
                            .chunks(size)
                            .map(|c| Value::List(c.to_vec()))
                            .collect()
                    } else {
                        items.into_iter().skip(count.max(0) as usize).collect()
                    }))
                },
            )));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("random(", function) {
        let syntax = || {
            ParserError::new(
                format!("Valid syntax: ${{random(min,max)}} or ${{random(max)}} was: {function}"),
                index,
            )
        };
        let values = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .ok_or_else(syntax)?;
        let (min, max) = match values.split_once(',') {
            Some((a, b)) => {
                if b.contains(',') {
                    return Err(syntax());
                }
                (a.trim().to_string(), b.trim().to_string())
            }
            None => ("0".to_string(), values.trim().to_string()),
        };
        let (x, y) = (Lazy::new(&min), Lazy::new(&max));
        return Ok(Some(named_expression(
            format!("random({min},{max})"),
            move |e| {
                let a = i64::from_value(&x.evaluate(e)?)?;
                let b = i64::from_value(&y.evaluate(e)?)?;
                match (a, b) {
                    (Some(a), Some(b)) if b > a => Ok(Value::Int(
                        camel_util::secure_random_helper::next_below((b - a) as u64) as i64 + a,
                    )),
                    _ => Err(CamelError::illegal_argument(format!(
                        "random(min,max) requires max to be greater than min, was: {},{}",
                        a.map(|v| v.to_string()).unwrap_or_else(|| "null".into()),
                        b.map(|v| v.to_string()).unwrap_or_else(|| "null".into())
                    ))),
                }
            },
        )));
    }
    if let Some(name) = if_starts_with_return_remainder("sys.", function) {
        let n = Lazy::new(name);
        return Ok(Some(named_expression(
            format!("systemProperty({name})"),
            move |e| {
                let key = n.evaluate(e)?.to_string();
                Ok(camel_util::system_properties::get_property(&key)
                    .map(Value::from)
                    .unwrap_or_default())
            },
        )));
    }
    let env = ["sysenv.", "sysenv:", "env.", "env:"]
        .iter()
        .find_map(|p| if_starts_with_return_remainder(p, function));
    if let Some(name) = env {
        let n = Lazy::new(name);
        return Ok(Some(named_expression(
            format!("systemEnvironment({name})"),
            move |e| {
                let key = n.evaluate(e)?.to_string();
                Ok(camel_util::io_helper::lookup_environment_variable(&key)
                    .map(Value::from)
                    .unwrap_or_default())
            },
        )));
    }
    Ok(None)
}
