//! The collection functions of the simple language (Java `CollectionFunctionFactory` and
//! `CollectionExpressionBuilder`): `${range(..)}`, `${distinct(..)}`, `${reverse(..)}`,
//! `${shuffle(..)}`, `${split(..)}`, `${sort(..)}`, `${forEach(..)}`, `${filter(..)}`,
//! `${list(..)}` and `${map(..)}`, and the functions changing the exchange: `${setHeader(..)}`,
//! `${setVariable(..)}`, `${listAdd(..)}`, `${listRemove(..)}`, `${mapAdd(..)}` and
//! `${mapRemove(..)}`.
//!
//! The functions changing the exchange only work with
//! [`Expression::evaluate_mut`] (processors
//! owning the exchange use it). Java changes the list or map of the exchange in place; Rust
//! values are not shared, so the list and map functions write the changed value back when
//! their source is the body, a header, a variable or an exchange property (`${body}`,
//! `${header.name}`, `${variable.name}`, `${exchangeProperty.name}`), and otherwise only return
//! it.

use super::functions::if_starts_with_return_remainder;
use super::math_functions::iterate;
use super::string_functions::Lazy;
use super::{ParseResult, ParserError};
use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, SharedExpression, SharedPredicate};
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
    if let Some(e) = mutating_function(function, index)? {
        return Ok(Some(e));
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

/// Where a list or map function changes the exchange.
#[derive(Clone, Debug, PartialEq)]
enum Place {
    Body,
    Header(String),
    Variable(String),
    Property(String),
    /// Another expression: the changed value is only returned.
    Other,
}

impl Place {
    /// The place of a source expression such as `${header.name}`.
    fn of(source: &str) -> Place {
        let Some(inner) = source
            .trim()
            .strip_prefix("${")
            .and_then(|s| s.strip_suffix('}'))
        else {
            return Place::Other;
        };
        let simple_name =
            |n: &str| !n.is_empty() && !n.contains(['.', '[', ']', '$', '{', '}', '(']);
        if inner == "body" || inner == "in.body" {
            return Place::Body;
        }
        for (prefix, make) in [
            ("header.", Place::Header as fn(String) -> Place),
            ("headers.", Place::Header),
            ("in.header.", Place::Header),
            ("in.headers.", Place::Header),
            ("variable.", Place::Variable),
            ("exchangeProperty.", Place::Property),
        ] {
            if let Some(n) = inner.strip_prefix(prefix) {
                if simple_name(n) {
                    return make(n.to_string());
                }
            }
        }
        Place::Other
    }

    fn write(&self, e: &mut Exchange, value: Value) {
        match self {
            Place::Body => e.message_mut().set_body(value),
            Place::Header(n) => e.message_mut().set_header(n, value),
            Place::Variable(n) => e.set_variable(n, value),
            Place::Property(n) => e.set_property(n.as_str(), value),
            Place::Other => {}
        }
    }
}

/// What a function changing the exchange does.
enum Change {
    SetHeader {
        name: String,
        type_name: Option<String>,
        exp: Lazy,
    },
    SetVariable {
        name: String,
        type_name: Option<String>,
        exp: Lazy,
    },
    ListAdd {
        source: Lazy,
        exp: Lazy,
    },
    ListRemove {
        source: Lazy,
        exp: Lazy,
    },
    MapAdd {
        source: Lazy,
        key: String,
        exp: Lazy,
    },
    MapRemove {
        source: Lazy,
        key: String,
    },
}

/// A function changing the exchange (Java `CollectionExpressionBuilder.setHeaderExpression`,
/// `setVariableExpression`, `listAddExpression`, `listRemoveExpression`, `mapAddExpression`
/// and `mapRemoveExpression`).
struct MutatingExpression {
    change: Change,
    description: String,
}

impl MutatingExpression {
    fn name(&self) -> &'static str {
        match self.change {
            Change::SetHeader { .. } => "setHeader",
            Change::SetVariable { .. } => "setVariable",
            Change::ListAdd { .. } => "listAdd",
            Change::ListRemove { .. } => "listRemove",
            Change::MapAdd { .. } => "mapAdd",
            Change::MapRemove { .. } => "mapRemove",
        }
    }
}

fn typed_value(exp: &Lazy, type_name: &Option<String>, e: &Exchange) -> CamelResult<Value> {
    let v = exp.evaluate(e)?;
    match type_name {
        Some(t) if !v.is_null() => super::functions::convert_to_type(&v, t),
        _ => Ok(v),
    }
}

impl Expression for MutatingExpression {
    fn evaluate(&self, _exchange: &Exchange) -> CamelResult<Value> {
        Err(CamelError::illegal_state(format!(
            "The simple function {} changes the exchange, so it must be evaluated with Expression::evaluate_mut: {}",
            self.name(),
            self.description
        )))
    }

    fn evaluate_mut(&self, e: &mut Exchange) -> CamelResult<Value> {
        match &self.change {
            Change::SetHeader {
                name,
                type_name,
                exp,
            } => {
                let v = typed_value(exp, type_name, e)?;
                if v.is_null() {
                    e.message_mut().remove_header(name);
                } else {
                    e.message_mut().set_header(name, v);
                }
                // does not return anything
                Ok(Value::Null)
            }
            Change::SetVariable {
                name,
                type_name,
                exp,
            } => {
                let v = typed_value(exp, type_name, e)?;
                if v.is_null() {
                    e.remove_variable(name);
                } else {
                    e.set_variable(name, v);
                }
                Ok(Value::Null)
            }
            Change::ListAdd { source, exp } => {
                let Value::List(mut list) = source.evaluate(e)? else {
                    return Ok(Value::Null);
                };
                let v = exp.evaluate(e)?;
                if !v.is_null() {
                    list.push(v);
                }
                let list = Value::List(list);
                Place::of(source.text()).write(e, list.clone());
                Ok(list)
            }
            Change::ListRemove { source, exp } => {
                let Value::List(mut list) = source.evaluate(e)? else {
                    return Ok(Value::Null);
                };
                let v = exp.evaluate(e)?;
                if !v.is_null() {
                    match list.iter().position(|x| *x == v) {
                        Some(i) => {
                            list.remove(i);
                        }
                        None => {
                            // the special name to remove the last, or an index
                            let pos = if v == "last" {
                                Some(list.len() as i64 - 1)
                            } else {
                                i64::from_value(&v).ok().flatten()
                            };
                            if let Some(pos) = pos {
                                if pos >= 0 && (pos as usize) < list.len() {
                                    list.remove(pos as usize);
                                }
                            }
                        }
                    }
                }
                let list = Value::List(list);
                Place::of(source.text()).write(e, list.clone());
                Ok(list)
            }
            Change::MapAdd { source, key, exp } => {
                let Value::Map(mut map) = source.evaluate(e)? else {
                    return Ok(Value::Null);
                };
                let v = exp.evaluate(e)?;
                if !v.is_null() {
                    map.insert(key.clone(), v);
                }
                let map = Value::Map(map);
                Place::of(source.text()).write(e, map.clone());
                Ok(map)
            }
            Change::MapRemove { source, key } => {
                let Value::Map(mut map) = source.evaluate(e)? else {
                    return Ok(Value::Null);
                };
                map.shift_remove(key);
                let map = Value::Map(map);
                Place::of(source.text()).write(e, map.clone());
                Ok(map)
            }
        }
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// The functions changing the exchange (Java `CollectionFunctionFactory`).
fn mutating_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let err = |m: &str| ParserError::new(format!("Valid syntax: {m} was: {function}"), index);
    let values = |remainder: &str| before_last(remainder, ")").filter(|v| !v.is_empty());
    let joined = |tokens: &[String], skip: usize| tokens[skip..].join(",");
    let make = |change: Change, description: String| -> ParseResult<Option<SharedExpression>> {
        Ok(Some(Arc::new(MutatingExpression {
            change,
            description,
        })))
    };
    for (prefix, header) in [("setHeader(", true), ("setVariable(", false)] {
        if let Some(r) = if_starts_with_return_remainder(prefix, function) {
            let fname = &prefix[..prefix.len() - 1];
            let syntax = format!("${{{fname}(name,exp)}} or ${{{fname}(name,type,exp)}}");
            let tokens = split_arguments(&values(r).ok_or_else(|| err(&syntax))?);
            if tokens.len() < 2 || tokens.len() > 3 {
                return Err(err(&syntax));
            }
            let name = tokens[0].clone();
            let (type_name, exp) = if tokens.len() == 3 {
                (
                    Some(camel_util::string_helper::remove_quotes(&tokens[1])),
                    tokens[2].clone(),
                )
            } else {
                (None, tokens[1].clone())
            };
            let description = format!("{fname}({name},{exp})");
            let exp = Lazy::new(&exp);
            let change = if header {
                Change::SetHeader {
                    name,
                    type_name,
                    exp,
                }
            } else {
                Change::SetVariable {
                    name,
                    type_name,
                    exp,
                }
            };
            return make(change, description);
        }
    }
    for (prefix, add) in [("listAdd(", true), ("listRemove(", false)] {
        if let Some(r) = if_starts_with_return_remainder(prefix, function) {
            let fname = &prefix[..prefix.len() - 1];
            let syntax = format!("${{{fname}(exp)}} or ${{{fname}(exp,exp)}}");
            let tokens = split_arguments(&values(r).ok_or_else(|| err(&syntax))?);
            let (source, skip) = if tokens.len() > 1 {
                (tokens[0].clone(), 1)
            } else {
                ("${body}".to_string(), 0)
            };
            let exp = joined(&tokens, skip);
            let description = format!("{fname}({source}, {exp})");
            let (source, exp) = (Lazy::new(&source), Lazy::new(&exp));
            let change = if add {
                Change::ListAdd { source, exp }
            } else {
                Change::ListRemove { source, exp }
            };
            return make(change, description);
        }
    }
    if let Some(r) = if_starts_with_return_remainder("mapAdd(", function) {
        let syntax = "${mapAdd(key,exp)} or ${mapAdd(exp,key,exp)}";
        let tokens = split_arguments(&values(r).ok_or_else(|| err(syntax))?);
        let (source, key, skip) = match tokens.len() {
            n if n > 2 => (tokens[0].clone(), tokens[1].clone(), 2),
            2 => ("${body}".to_string(), tokens[0].clone(), 1),
            _ => return Err(err(syntax)),
        };
        let exp = joined(&tokens, skip);
        let description = format!("mapAdd({source}, {key}, {exp})");
        let change = Change::MapAdd {
            source: Lazy::new(&source),
            key,
            exp: Lazy::new(&exp),
        };
        return make(change, description);
    }
    if let Some(r) = if_starts_with_return_remainder("mapRemove(", function) {
        let syntax = "${mapRemove(key)} or ${mapRemove(exp,key)}";
        let tokens = split_arguments(&values(r).ok_or_else(|| err(syntax))?);
        let (source, key) = match tokens.len() {
            2 => (tokens[0].clone(), tokens[1].clone()),
            1 => ("${body}".to_string(), tokens[0].clone()),
            _ => return Err(err(syntax)),
        };
        let description = format!("mapRemove({source}, {key})");
        let change = Change::MapRemove {
            source: Lazy::new(&source),
            key,
        };
        return make(change, description);
    }
    Ok(None)
}
