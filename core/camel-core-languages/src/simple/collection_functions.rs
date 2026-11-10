//! The collection functions of the simple language (Java `CollectionFunctionFactory` and
//! `CollectionExpressionBuilder`): `${range(..)}`, `${distinct(..)}`, `${reverse(..)}`,
//! `${shuffle(..)}`, `${split(..)}`, `${sort(..)}`, `${forEach(..)}`, `${filter(..)}`,
//! `${list(..)}` and `${map(..)}`.
//!
//! Not ported yet: the functions changing the exchange while evaluating (`setHeader`,
//! `setVariable`, `listAdd`, `listRemove`, `mapAdd`, `mapRemove`), as expressions evaluate on a
//! shared exchange in Rust (see the handoff).

use super::functions::if_starts_with_return_remainder;
use super::math_functions::iterate;
use super::string_functions::Lazy;
use super::{ParseResult, ParserError};
use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{SharedExpression, SharedPredicate};
use camel_support::builder::named_expression;
use camel_util::string_helper::{before_last, remove_leading_and_ending_quotes};
use camel_util::string_quote_helper::split_safe_quote_full;
use camel_util::value::{Value, ValueMap};
use std::sync::{Arc, OnceLock};

/// Java `splitArguments`: split keeping quotes, trimmed unless blank, then unquoted.
fn split_arguments(values: &str) -> Vec<String> {
    split_safe_quote_full(values, ',', false, true, false)
        .into_iter()
        .map(|t| {
            let trimmed = t.trim();
            let t = if trimmed.is_empty() {
                t.as_str()
            } else {
                trimmed
            };
            remove_leading_and_ending_quotes(t)
        })
        .collect()
}

fn expressions(values: Option<String>) -> Vec<Lazy> {
    match values.filter(|v| !v.is_empty()) {
        Some(v) => split_safe_quote_full(&v, ',', true, false, false)
            .iter()
            .map(|t| Lazy::new(t))
            .collect(),
        None => vec![Lazy::new("${body}")],
    }
}

fn items(exps: &[Lazy], e: &Exchange) -> CamelResult<Vec<Value>> {
    let mut out = Vec::new();
    for x in exps {
        out.extend(iterate(x.evaluate(e)?));
    }
    Ok(out)
}

fn texts(exps: &[Lazy]) -> String {
    exps.iter().map(|x| x.text()).collect::<Vec<_>>().join(", ")
}

/// A predicate parsed on first use.
#[derive(Clone)]
struct LazyPredicate(String, Arc<OnceLock<SharedPredicate>>);

impl LazyPredicate {
    fn matches(&self, e: &Exchange) -> CamelResult<bool> {
        if let Some(p) = self.1.get() {
            return p.matches(e);
        }
        let p = super::language::SimpleLanguage::new().parse_predicate(&self.0)?;
        self.1.get_or_init(|| p).matches(e)
    }
}

/// Creates the collection function, if the function is one.
///
/// # Errors
///
/// Returns a syntax error for invalid arguments.
pub fn collection_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let err = |m: &str| ParserError::new(format!("Valid syntax: {m} was: {function}"), index);
    for name in [
        "setHeader(",
        "setVariable(",
        "listAdd(",
        "listRemove(",
        "mapAdd(",
        "mapRemove(",
    ] {
        if if_starts_with_return_remainder(name, function).is_some() {
            return Err(ParserError::new(
                format!("The function {} changes the exchange and is not supported yet by the Rust port: {function}", &name[..name.len() - 1]),
                index,
            ));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("range(", function) {
        let values = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| err("${range(min,max)} or ${range(max)}"))?;
        let (min, max) = match values.split_once(',') {
            Some((a, b)) => {
                if b.contains(',') {
                    return Err(err("${range(min,max)} or ${range(max)}"));
                }
                (a.trim().to_string(), b.trim().to_string())
            }
            None => ("0".to_string(), values.trim().to_string()),
        };
        let (x, y) = (Lazy::new(&min), Lazy::new(&max));
        return Ok(Some(named_expression(
            format!("range({min},{max})"),
            move |e| {
                let (Some(a), Some(b)) = (
                    i64::from_value(&x.evaluate(e)?)?,
                    i64::from_value(&y.evaluate(e)?)?,
                ) else {
                    return Err(CamelError::illegal_argument(format!(
                        "range expression evaluated to null: {},{}",
                        x.text(),
                        y.text()
                    )));
                };
                Ok(if a < b {
                    Value::List((a..b).map(Value::Int).collect())
                } else {
                    Value::Null
                })
            },
        )));
    }
    type ListFn = fn(Vec<Value>) -> Vec<Value>;
    let list_functions: [(&str, ListFn); 3] = [
        ("distinct", |v| {
            let mut out: Vec<Value> = Vec::new();
            for i in v {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
            out
        }),
        ("reverse", |mut v| {
            v.reverse();
            v
        }),
        ("shuffle", |mut v| {
            // Fisher-Yates with the secure random helper
            for i in (1..v.len()).rev() {
                let j = camel_util::secure_random_helper::next_below(i as u64 + 1) as usize;
                v.swap(i, j);
            }
            v
        }),
    ];
    for (name, f) in list_functions {
        if let Some(r) = function
            .strip_prefix(name)
            .and_then(|r| if_starts_with_return_remainder("(", r))
        {
            let exps = expressions(before_last(r, ")"));
            return Ok(Some(named_expression(
                format!("{name}([{}])", texts(&exps)),
                move |e| Ok(Value::List(f(items(&exps, e)?))),
            )));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("split(", function) {
        let values = before_last(r, ")");
        let (mut exp, mut separator) = ("${body}".to_string(), ",".to_string());
        match values.as_deref() {
            Some("\n") => separator = "\n".to_string(),
            Some(v) if !v.is_empty() => {
                let tokens = split_arguments(v);
                if tokens.len() > 2 {
                    return Err(err("${split(separator)} or ${split(exp,separator)}"));
                }
                if tokens.len() == 2 {
                    exp = tokens[0].clone();
                    separator = tokens[1].clone();
                } else {
                    separator = tokens[0].clone();
                }
            }
            _ => {}
        }
        let sep = separator
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\t", "\t");
        let x = Lazy::new(&exp);
        return Ok(Some(named_expression(
            format!("split({exp},{separator})"),
            move |e| {
                Ok(match x.evaluate_string(e)? {
                    // Java Pattern.split drops the trailing empty strings
                    Some(t) => {
                        let mut parts: Vec<&str> = t.split(sep.as_str()).collect();
                        while parts.len() > 1 && parts.last() == Some(&"") {
                            parts.pop();
                        }
                        Value::List(parts.into_iter().map(Value::from).collect())
                    }
                    None => Value::Null,
                })
            },
        )));
    }
    if let Some(r) = if_starts_with_return_remainder("sort(", function) {
        let (mut exp, mut reverse) = ("${body}".to_string(), false);
        if let Some(v) = before_last(r, ")").filter(|v| !v.is_empty()) {
            let tokens = split_arguments(&v);
            if tokens.len() > 2 {
                return Err(err("${sort(exp)} or ${sort(exp,reverse)}"));
            }
            if tokens.len() == 2 {
                exp = tokens[0].clone();
                reverse = tokens[1].eq_ignore_ascii_case("true");
            } else if tokens[0].eq_ignore_ascii_case("true")
                || tokens[0].eq_ignore_ascii_case("false")
            {
                reverse = tokens[0].eq_ignore_ascii_case("true");
            } else {
                exp = tokens[0].clone();
            }
        }
        let x = Lazy::new(&exp);
        return Ok(Some(named_expression(
            format!("sort({exp},{reverse})"),
            move |e| {
                let mut v = iterate(x.evaluate(e)?);
                v.sort_by(|a, b| camel_support::object_helper::compare(a, b, false));
                if reverse {
                    v.reverse();
                }
                Ok(Value::List(v))
            },
        )));
    }
    for (name, filter) in [("forEach", false), ("filter", true)] {
        if let Some(r) = function
            .strip_prefix(name)
            .and_then(|r| if_starts_with_return_remainder("(", r))
        {
            let syntax = format!("${{{name}(exp,exp)}}");
            let values = before_last(r, ")")
                .filter(|v| !v.is_empty())
                .ok_or_else(|| err(&syntax))?;
            let tokens = split_arguments(&values);
            if tokens.len() < 2 {
                return Err(err(&syntax));
            }
            let source = Lazy::new(&tokens[0]);
            let body_function = tokens[1..].join(",");
            let d = format!("{name}({},{body_function})", tokens[0]);
            let each = Lazy::new(&body_function);
            let predicate = LazyPredicate(body_function, Arc::new(OnceLock::new()));
            return Ok(Some(named_expression(d, move |e| {
                let mut answer = Vec::new();
                for item in iterate(source.evaluate(e)?) {
                    let mut dummy = e.clone();
                    dummy.message_mut().set_body(item.clone());
                    if filter {
                        if predicate.matches(&dummy)? {
                            answer.push(item);
                        }
                    } else {
                        let out = each.evaluate(&dummy)?;
                        if !out.is_null() {
                            answer.push(out);
                        }
                    }
                }
                Ok(Value::List(answer))
            })));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("list(", function) {
        let exps: Vec<Lazy> = match before_last(r, ")").filter(|v| !v.is_empty()) {
            Some(v) => split_safe_quote_full(&v, ',', true, false, false)
                .iter()
                .map(|t| Lazy::new(t))
                .collect(),
            None => vec![],
        };
        return Ok(Some(named_expression(
            format!("list([{}])", texts(&exps)),
            move |e| {
                let mut out = Vec::new();
                for x in &exps {
                    out.push(x.evaluate(e)?);
                }
                Ok(Value::List(out))
            },
        )));
    }
    if let Some(r) = if_starts_with_return_remainder("map(", function) {
        let tokens: Vec<String> = match before_last(r, ")").filter(|v| !v.is_empty()) {
            Some(v) => split_safe_quote_full(&v, ',', true, false, false),
            None => vec![],
        };
        if tokens.len() % 2 == 1 {
            return Err(ParserError::new(
                format!(
                    "Map function must have an even number of values, was: {} values.",
                    tokens.len()
                ),
                index,
            ));
        }
        let pairs: Vec<(Lazy, Lazy)> = tokens
            .chunks(2)
            .map(|p| (Lazy::new(&p[0]), Lazy::new(&p[1])))
            .collect();
        return Ok(Some(named_expression(
            format!("map({})", tokens.join(",")),
            move |e| {
                let mut m = ValueMap::new();
                for (k, v) in &pairs {
                    m.insert(k.evaluate(e)?.to_string(), v.evaluate(e)?);
                }
                Ok(Value::Map(m))
            },
        )));
    }
    Ok(None)
}
