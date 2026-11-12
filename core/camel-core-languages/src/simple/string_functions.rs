//! The string functions of the simple language (Java `StringFunctionFactory` and
//! `StringExpressionBuilder`): `${replace(a,b)}`, `${substring(2,1)}`, `${trim()}`, `${pad(..)}`,
//! `${uppercase()}`, `${concat(..)}`, `${quote()}`, `${escape(json)}`, ...
//!
//! The arguments are simple expressions themselves (such as `${header.foo}`); without an
//! expression argument the functions work on the message body.

use super::functions::if_starts_with_return_remainder;
use super::{ParseResult, ParserError};
use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::SharedExpression;
use camel_support::builder::named_expression;
use camel_util::escape_helper::{self, Kind};
use camel_util::string_helper::{
    after, before, before_last, between, between_index, capitalize_all, is_double_quoted,
    normalize_whitespace, remove_leading_and_ending_quotes, xml_decode,
};
use camel_util::string_quote_helper::{double_quote, split_safe_quote_full};
use camel_util::value::Value;
use std::sync::{Arc, OnceLock};

/// A simple expression parsed on first use (Java `context.resolveLanguage("simple")
/// .createExpression(text)` in `init`).
#[derive(Clone)]
pub(crate) struct Lazy {
    text: String,
    parsed: Arc<OnceLock<SharedExpression>>,
}

impl Lazy {
    pub(crate) fn new(text: &str) -> Self {
        Lazy {
            text: text.to_string(),
            parsed: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn evaluate(&self, e: &Exchange) -> CamelResult<Value> {
        if let Some(x) = self.parsed.get() {
            return x.evaluate(e);
        }
        let registry = camel_api::context::context_plugin::<
            camel_api::spi::simple_function::SimpleFunctionRegistry,
        >(e.context().as_ref());
        let x = super::functions::with_custom_functions(registry, || {
            super::language::SimpleLanguage::new().parse_expression(&self.text)
        })?;
        self.parsed.get_or_init(|| x).evaluate(e)
    }

    pub(crate) fn evaluate_string(&self, e: &Exchange) -> CamelResult<Option<String>> {
        let v = self.evaluate(e)?;
        String::from_value(&v)
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

/// The value of the optional expression, or the body.
pub(crate) fn value_of(exp: &Option<Lazy>, e: &Exchange) -> CamelResult<Value> {
    match exp {
        Some(x) => x.evaluate(e),
        None => Ok(e.message().body().clone()),
    }
}

pub(crate) fn string_of(exp: &Option<Lazy>, e: &Exchange) -> CamelResult<Option<String>> {
    String::from_value(&value_of(exp, e)?)
}

fn text_value(s: Option<String>) -> Value {
    s.map(Value::Text).unwrap_or_default()
}

/// The optional single argument of `name(arg)`, without quotes.
pub(crate) fn single_argument(remainder: &str) -> Option<Lazy> {
    before_last(remainder, ")")
        .filter(|v| !v.is_empty())
        .map(|v| Lazy::new(&remove_leading_and_ending_quotes(&v)))
}

fn described(name: &str, exp: &Option<Lazy>) -> String {
    match exp {
        Some(x) => format!("{name}({})", x.text()),
        None => format!("{name}()"),
    }
}

/// A function of one optional expression applying `f` to its text value.
fn string_unary<F>(name: &'static str, exp: Option<Lazy>, f: F) -> SharedExpression
where
    F: Fn(String) -> String + Send + Sync + 'static,
{
    named_expression(described(name, &exp), move |e| {
        Ok(text_value(string_of(&exp, e)?.map(&f)))
    })
}

/// A function on the text of its argument (or the body).
type UnaryFunction = (&'static str, fn(String) -> String);

/// The functions of one optional argument applying a text function.
const UNARY_FUNCTIONS: &[UnaryFunction] = &[
    ("trim", |s| s.trim().to_string()),
    ("capitalize", |s| capitalize_all(&s)),
    ("uppercase", |s| s.to_uppercase()),
    ("lowercase", |s| s.to_lowercase()),
    ("unquote", |s| remove_leading_and_ending_quotes(&s)),
    ("normalizeWhitespace", |s| normalize_whitespace(&s)),
];

fn arguments(remainder: &str) -> Option<String> {
    before_last(remainder, ")").filter(|v| !v.is_empty())
}

/// Creates the string function, if the function is one.
///
/// # Errors
///
/// Returns a syntax error for invalid arguments.
pub fn string_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let err = |m: &str| ParserError::new(format!("Valid syntax: {m} was: {function}"), index);
    if let Some(r) = if_starts_with_return_remainder("replace(", function) {
        let values = arguments(r)
            .ok_or_else(|| err("${replace(from,to)} or ${replace(from,to,expression)}"))?;
        let tokens = split_safe_quote_full(&values, ',', false, false, false);
        if tokens.len() < 2 || tokens.len() > 3 {
            return Err(err("${replace(from,to)} or ${replace(from,to,expression)}"));
        }
        let from = xml_decode(&tokens[0]);
        let mut to = xml_decode(&tokens[1]);
        if to == "&empty;" {
            to = String::new();
        }
        let exp = Lazy::new(tokens.get(2).map(String::as_str).unwrap_or("${body}"));
        let description = format!("replace({},{from},{to})", exp.text());
        return Ok(Some(named_expression(description, move |e| {
            Ok(text_value(
                exp.evaluate_string(e)?.map(|t| t.replace(&from, &to)),
            ))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("substring(", function) {
        let syntax =
            "${substring(num)}, ${substring(num,num)}, or ${substring(num,num,expression)}";
        let values = arguments(r).ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', false, false, false);
        if tokens.len() > 3 {
            return Err(err("${substring(num,num,expression)}"));
        }
        let head = Lazy::new(&tokens[0]);
        let tail = Lazy::new(tokens.get(1).map(String::as_str).unwrap_or("0"));
        let exp = Lazy::new(tokens.get(2).map(String::as_str).unwrap_or("${body}"));
        let description = format!("substring({},{},{})", exp.text(), head.text(), tail.text());
        return Ok(Some(named_expression(description, move |e| {
            let n1 = i64::from_value(&head.evaluate(e)?)?;
            let n2 = i64::from_value(&tail.evaluate(e)?)?;
            let (Some(mut n1), Some(mut n2)) = (n1, n2) else {
                return Err(CamelError::illegal_argument(format!(
                    "substring number expression evaluated to null: {},{}",
                    head.text(),
                    tail.text()
                )));
            };
            if n1 < 0 && n2 == 0 {
                n2 = n1;
                n1 = 0;
            }
            Ok(text_value(exp.evaluate_string(e)?.map(|t| {
                between_index(&t, n1.unsigned_abs() as usize, n2.unsigned_abs() as usize)
            })))
        })));
    }
    for (name, before_mode) in [("substringBefore(", true), ("substringAfter(", false)] {
        if let Some(r) = if_starts_with_return_remainder(name, function) {
            let n = &name[..name.len() - 1];
            let syntax = format!("${{{n}(exp)}} or ${{{n}(exp,exp)}}");
            let values = arguments(r).ok_or_else(|| err(&syntax))?;
            let tokens = split_safe_quote_full(&values, ',', false, false, false);
            if tokens.len() > 2 {
                return Err(err(&syntax));
            }
            let (exp, token) = if tokens.len() == 2 {
                (Lazy::new(&tokens[0]), Lazy::new(&tokens[1]))
            } else {
                (Lazy::new("${body}"), Lazy::new(&tokens[0]))
            };
            let description = format!("{n}({},{})", exp.text(), token.text());
            return Ok(Some(named_expression(description, move |e| {
                let Some(body) = exp.evaluate_string(e)? else {
                    return Ok(Value::Null);
                };
                let Some(t) = token.evaluate_string(e)? else {
                    return Ok(Value::Null);
                };
                Ok(text_value(if before_mode {
                    before(&body, &t)
                } else {
                    after(&body, &t)
                }))
            })));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("substringBetween(", function) {
        let syntax = "${substringBetween(after,before)} or ${substringBetween(exp,after,before)}";
        let values = arguments(r).ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', false, false, false);
        if tokens.len() < 2 || tokens.len() > 3 {
            return Err(err(syntax));
        }
        let (exp, a, b) = if tokens.len() == 3 {
            (
                Lazy::new(&tokens[0]),
                Lazy::new(&tokens[1]),
                Lazy::new(&tokens[2]),
            )
        } else {
            (
                Lazy::new("${body}"),
                Lazy::new(&tokens[0]),
                Lazy::new(&tokens[1]),
            )
        };
        let description = format!("substringBetween({},{},{})", exp.text(), a.text(), b.text());
        return Ok(Some(named_expression(description, move |e| {
            let Some(body) = exp.evaluate_string(e)? else {
                return Ok(Value::Null);
            };
            let (Some(a), Some(b)) = (a.evaluate_string(e)?, b.evaluate_string(e)?) else {
                return Ok(Value::Null);
            };
            Ok(text_value(between(&body, &a, &b)))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("contains(", function) {
        let syntax = "${contains(text)} or ${contains(exp,text)}";
        let values = arguments(r).ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', false, false, false);
        if tokens.is_empty() || tokens.len() > 2 {
            return Err(err(syntax));
        }
        let (exp, pattern) = if tokens.len() == 1 {
            (Lazy::new("${body}"), Lazy::new(&tokens[0]))
        } else {
            (Lazy::new(&tokens[0]), Lazy::new(&tokens[1]))
        };
        let description = format!("contains({},{})", exp.text(), pattern.text());
        return Ok(Some(named_expression(description, move |e| {
            let (l, r) = (exp.evaluate(e)?, pattern.evaluate(e)?);
            Ok(Value::Bool(match (l.is_null(), r.is_null()) {
                (true, true) => true,
                (false, false) => camel_support::object_helper::type_coerce_contains(&l, &r, true),
                _ => false,
            }))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("val(", function) {
        let value = arguments(r).ok_or_else(|| err("${val(exp)}"))?;
        let exp = Lazy::new(&value);
        return Ok(Some(named_expression(
            format!("simple({value})"),
            move |e| exp.evaluate(e),
        )));
    }
    for (name, f) in UNARY_FUNCTIONS {
        if let Some(r) = function
            .strip_prefix(name)
            .and_then(|r| if_starts_with_return_remainder("(", r))
        {
            return Ok(Some(string_unary(name, single_argument(r), *f)));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("quote(", function) {
        return Ok(Some(string_unary("quote", single_argument(r), |s| {
            if is_double_quoted(&s) {
                s
            } else {
                double_quote(&remove_leading_and_ending_quotes(&s))
            }
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("safeQuote(", function) {
        let exp = single_argument(r);
        return Ok(Some(named_expression(
            described("safeQuote", &exp),
            move |e| {
                let value = value_of(&exp, e)?;
                Ok(match &value {
                    Value::Null => Value::Null,
                    Value::Int(_) | Value::Float(_) | Value::Bool(_) => value,
                    _ => {
                        let mut body = String::from_value(&value)?.unwrap_or_default();
                        let quoted_once = |t: &str, q: char| {
                            t.len() >= 2
                                && t.starts_with(q)
                                && t[1..].find(q).map(|i| i + 1) == Some(t.len() - 1)
                        };
                        if quoted_once(&body, '"') || quoted_once(&body, '\'') {
                            body = body[1..body.len() - 1].to_string();
                        }
                        Value::from(format!("\"{}\"", escape_helper::escape(Kind::Json, &body)))
                    }
                })
            },
        )));
    }
    if let Some(r) = if_starts_with_return_remainder("pad(", function) {
        let values = arguments(r)
            .ok_or_else(|| err("${pad(len)} or ${pad(exp,len)} or ${pad(exp,len,separator)}"))?;
        let tokens = split_safe_quote_full(&values, ',', true, true, false);
        if tokens.len() < 2 || tokens.len() > 3 {
            return Err(err("${pad(exp,len)} or ${pad(exp,len,separator)}"));
        }
        let exp = Lazy::new(&remove_leading_and_ending_quotes(&tokens[0]));
        let len = Lazy::new(&remove_leading_and_ending_quotes(&tokens[1]));
        let separator = tokens.get(2).map(|s| remove_leading_and_ending_quotes(s));
        let description = match &separator {
            Some(s) => format!("pad({},{},{s})", exp.text(), len.text()),
            None => format!("pad({},{})", exp.text(), len.text()),
        };
        return Ok(Some(named_expression(description, move |e| {
            let Some(answer) = exp.evaluate_string(e)? else {
                return Ok(Value::Null);
            };
            let Some(width) = i64::from_value(&len.evaluate(e)?)? else {
                return Err(CamelError::illegal_argument(format!(
                    "pad length expression evaluated to null: {}",
                    len.text()
                )));
            };
            let sep = separator
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| " ".to_string());
            let max = width.unsigned_abs() as usize;
            let len = answer.chars().count();
            if max <= len {
                return Ok(Value::from(answer));
            }
            let mut padding = String::new();
            while padding.chars().count() < max - len {
                padding.push_str(&sep);
            }
            let padding: String = padding.chars().take(max - len).collect();
            Ok(Value::from(if width > 0 {
                answer + &padding
            } else {
                padding + &answer
            }))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("concat(", function) {
        let syntax = "${concat(exp)} or ${concat(exp,exp)} or ${concat(exp,exp,separator)}";
        let values = arguments(r).ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', true, true, false);
        if tokens.len() > 3 {
            return Err(err(syntax));
        }
        let (first, second, separator) = if tokens.len() >= 2 {
            (
                remove_leading_and_ending_quotes(&tokens[0]),
                remove_leading_and_ending_quotes(&tokens[1]),
                tokens.get(2).map(|s| remove_leading_and_ending_quotes(s)),
            )
        } else {
            (
                "${body}".to_string(),
                remove_leading_and_ending_quotes(&tokens[0]),
                None,
            )
        };
        let (a, b) = (Lazy::new(&first), Lazy::new(&second));
        let description = match &separator {
            Some(s) => format!("concat({first},{second},{s})"),
            None => format!("concat({first},{second})"),
        };
        return Ok(Some(named_expression(description, move |e| {
            let (v1, v2) = (a.evaluate_string(e)?, b.evaluate_string(e)?);
            Ok(text_value(match (v1, v2) {
                (Some(x), Some(y)) => Some(format!("{x}{}{y}", separator.as_deref().unwrap_or(""))),
                (x, y) => x.or(y),
            }))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("length(", function) {
        let exp = single_argument(r);
        return Ok(Some(named_expression(
            described("length", &exp),
            move |e| {
                let v = value_of(&exp, e)?;
                Ok(match &v {
                    Value::Null => Value::Null,
                    Value::Bytes(b) => Value::Int(b.len() as i64),
                    other => Value::Int(other.to_string().len() as i64),
                })
            },
        )));
    }
    if let Some(r) = if_starts_with_return_remainder("size(", function) {
        let exp = single_argument(r);
        return Ok(Some(named_expression(described("size", &exp), move |e| {
            let v = value_of(&exp, e)?;
            Ok(Value::Int(match &v {
                Value::Null => 0,
                Value::List(l) => l.len() as i64,
                Value::Map(m) => m.len() as i64,
                Value::Bytes(b) => b.len() as i64,
                _ => 1,
            }))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("escape(", function) {
        let values =
            arguments(r).ok_or_else(|| err("${escape(kind)} or ${escape(kind,expression)}"))?;
        let kind_name = before(&values, ",")
            .unwrap_or_else(|| values.clone())
            .trim()
            .to_string();
        let Some(kind) = Kind::from_name(&kind_name) else {
            return Err(ParserError::new(
                format!("Unknown escape kind: {kind_name} (valid kinds: html, xml, json, js, sql, url) was: {function}"),
                index,
            ));
        };
        let exp = after(&values, ",")
            .filter(|s| !s.trim().is_empty())
            .map(|s| Lazy::new(&remove_leading_and_ending_quotes(s.trim())));
        let name = kind_name.to_lowercase();
        let description = match &exp {
            Some(x) => format!("escape({name},{})", x.text()),
            None => format!("escape({name})"),
        };
        return Ok(Some(named_expression(description, move |e| {
            Ok(text_value(
                string_of(&exp, e)?.map(|s| escape_helper::escape(kind, &s).into_owned()),
            ))
        })));
    }
    Ok(None)
}
