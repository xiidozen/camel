//! The functions of the simple language (Java `SimpleFunctionExpression`, the
//! `DirectFunctionFactory` and the built-in function factories): `${body}`, `${header.foo}`,
//! `${exchangeProperty.foo}`, `${variable.foo}`, `${exchangeId}`, `${bodyAs(String)}`, ...
//!
//! Ported so far: the direct functions, the body, header, exchange property and variable
//! functions (with map / list navigation for their OGNL suffixes), and the `int:` / `long:`
//! / `boolean:` / `string:` conversions. The other function factories (date, math, string,
//! collection, random, ...) follow; an unknown function is a syntax error, as in Java.

use super::{ParseResult, ParserError};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::SharedExpression;
use camel_support::builder::{constant_expression, named_expression};
use camel_util::ognl_helper;
use camel_util::string_helper::{remove_leading_and_ending_quotes, remove_quotes};
use camel_util::value::Value;
use std::sync::{Arc, OnceLock};

/// The remainder of the text after the prefix, if it starts with the prefix and the remainder
/// is not empty (Java `SimpleFunctionHelper.ifStartsWithReturnRemainder`).
pub fn if_starts_with_return_remainder<'a>(prefix: &str, text: &'a str) -> Option<&'a str> {
    text.strip_prefix(prefix).filter(|r| !r.is_empty())
}

/// The index of the `)` closing the text, skipping quoted text and nested parentheses (Java
/// `SimpleFunctionHelper.indexOfClosingParenthesis`).
pub fn index_of_closing_parenthesis(text: &str) -> Option<usize> {
    let mut depth = 0;
    let (mut single, mut double) = (false, false);
    for (i, c) in text.char_indices() {
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '(' if !single && !double => depth += 1,
            ')' if !single && !double => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn key_remainder(remainder: Option<&str>) -> Option<&str> {
    let r = remainder?;
    match r.chars().next() {
        Some('.' | ':' | '?' | '[') => Some(r),
        Some(_) => None,
        None => Some(r),
    }
}

/// The key part of `header.foo`, `headers:foo`, `in.header[foo]`, ... (Java `parseInHeader`).
pub fn parse_in_header(function: &str) -> Option<&str> {
    let remainder = if_starts_with_return_remainder("in.headers", function)
        .or_else(|| if_starts_with_return_remainder("in.header", function))
        .or_else(|| if_starts_with_return_remainder("headers", function))
        .or_else(|| if_starts_with_return_remainder("header", function));
    key_remainder(remainder)
}

/// The key part of `variable.foo` / `variables.foo` (Java `parseVariable`).
pub fn parse_variable(function: &str) -> Option<&str> {
    let remainder = if_starts_with_return_remainder("variables", function)
        .or_else(|| if_starts_with_return_remainder("variable", function));
    key_remainder(remainder)
}

/// The expression of a name that may contain simple functions (Java
/// `ExpressionBuilder.simpleExpression`): the name is evaluated with the simple language.
fn name_expression(name: &str) -> SharedExpression {
    if !super::tokenizer::has_function_start_token(name) {
        return constant_expression(name);
    }
    let text = name.to_string();
    let parsed: Arc<OnceLock<SharedExpression>> = Arc::new(OnceLock::new());
    named_expression(format!("simple({name})"), move |e| {
        let exp = match parsed.get() {
            Some(x) => x.clone(),
            None => {
                let x = super::language::SimpleLanguage::new().parse_expression(&text)?;
                parsed.get_or_init(|| x).clone()
            }
        };
        exp.evaluate(e)
    })
}

type Lookup = fn(&Exchange, &str) -> Option<Value>;

fn scoped_expression(kind: &'static str, name: &str, lookup: Lookup) -> SharedExpression {
    let key = name_expression(name);
    named_expression(format!("{kind}({name})"), move |e| {
        let k = key.evaluate(e)?.to_string();
        Ok(lookup(e, &k).unwrap_or_default())
    })
}

fn header_lookup(e: &Exchange, k: &str) -> Option<Value> {
    e.message().header(k).cloned()
}

fn property_lookup(e: &Exchange, k: &str) -> Option<Value> {
    e.property(k).cloned()
}

fn variable_lookup(e: &Exchange, k: &str) -> Option<Value> {
    e.variable(k).cloned()
}

/// Navigates into a value with an OGNL expression (Java `OgnlExpressionBuilder` on maps and
/// lists): `foo`, `[0]`, `[key]`, `.size()`, `.length()`, `?.foo` (null safe).
fn navigate(value: Value, ognl: &str, description: &str) -> CamelResult<Value> {
    let parts = ognl_helper::split_ognl(ognl)
        .map_err(|m| CamelError::new(ErrorKind::ExpressionIllegalSyntax, m))?;
    let parts = if parts.is_empty() {
        vec![ognl.to_string()]
    } else {
        parts
    };
    let mut current = value;
    for part in parts {
        let null_safe = part.starts_with("?.") || part.starts_with('?');
        let p = ognl_helper::remove_leading_operators(&part).to_string();
        if current.is_null() {
            if null_safe {
                return Ok(Value::Null);
            }
            return Err(CamelError::new(
                ErrorKind::RuntimeCamel,
                format!("Null object returned by {description} when evaluating: {p}"),
            ));
        }
        current = step(current, &p)?;
    }
    Ok(current)
}

fn step(value: Value, part: &str) -> CamelResult<Value> {
    // index: [0], [key], name[0]
    if let Some(start) = part.find('[') {
        if part.ends_with(']') {
            let (name, index) = (&part[..start], &part[start + 1..part.len() - 1]);
            let base = if name.is_empty() {
                value
            } else {
                step(value, name)?
            };
            return Ok(index_into(&base, &remove_leading_and_ending_quotes(index)));
        }
    }
    match part {
        "size()" | "size" | "length()" | "length" => Ok(value
            .size()
            .map(|s| Value::Int(s as i64))
            .unwrap_or_else(|| Value::Int(value.to_string().chars().count() as i64))),
        "isEmpty()" => Ok(Value::Bool(camel_util::object_helper::is_empty(&value))),
        "toString()" => Ok(Value::from(value.to_string())),
        "toUpperCase()" => Ok(Value::from(value.to_string().to_uppercase())),
        "toLowerCase()" => Ok(Value::from(value.to_string().to_lowercase())),
        "trim()" => Ok(Value::from(value.to_string().trim().to_string())),
        key => match &value {
            Value::Map(m) => Ok(m.get(key).cloned().unwrap_or_default()),
            _ => Err(CamelError::new(
                ErrorKind::RuntimeCamel,
                format!(
                    "Cannot invoke method: {key} on {} of type: {}",
                    value,
                    value.type_name()
                ),
            )),
        },
    }
}

fn index_into(value: &Value, index: &str) -> Value {
    match value {
        Value::List(l) => {
            let i = if index == "last" {
                l.len().checked_sub(1)
            } else if let Some(rest) = index.strip_prefix("last-") {
                rest.parse::<usize>()
                    .ok()
                    .and_then(|n| l.len().checked_sub(1 + n))
            } else {
                index.parse::<usize>().ok()
            };
            i.and_then(|i| l.get(i).cloned()).unwrap_or_default()
        }
        Value::Map(m) => m.get(index).cloned().unwrap_or_default(),
        Value::Text(t) => index
            .parse::<usize>()
            .ok()
            .and_then(|i| t.chars().nth(i))
            .map(|c| Value::from(c.to_string()))
            .unwrap_or_default(),
        _ => Value::Null,
    }
}

fn ognl_expression(kind: &'static str, key: &str, lookup: Lookup) -> SharedExpression {
    // the first part is the name, the rest navigates into the value
    let (name, rest) = split_first_ognl(key);
    let name = name.to_string();
    let rest = rest.to_string();
    let description = format!("{kind}({key})");
    let d = description.clone();
    named_expression(description, move |e| {
        let v = lookup(e, &name).unwrap_or_default();
        if rest.is_empty() {
            Ok(v)
        } else {
            navigate(v, &rest, &d)
        }
    })
}

/// Splits `foo.bar[0]` into `foo` and `.bar[0]`, and `foo[0]` into `foo` and `[0]`.
fn split_first_ognl(key: &str) -> (&str, &str) {
    let end = key.find(['.', '[', '?']).unwrap_or(key.len());
    (&key[..end], &key[end..])
}

pub use camel_support::type_conversion::convert_to_type;

fn typed(exp: SharedExpression, type_name: &'static str) -> SharedExpression {
    named_expression(exp.describe(), move |e| {
        convert_to_type(&exp.evaluate(e)?, type_name)
    })
}

/// Creates the expression of the function text (the text between `${` and `}`).
///
/// # Errors
///
/// Returns a syntax error for an unknown function or an invalid syntax.
pub fn create_function(function: &str, index: usize) -> ParseResult<SharedExpression> {
    let (type_name, f) = if let Some(r) = function.strip_prefix("int:") {
        (Some("int"), r)
    } else if let Some(r) = function.strip_prefix("integer:") {
        (Some("int"), r)
    } else if let Some(r) = function.strip_prefix("long:") {
        (Some("long"), r)
    } else if let Some(r) = function.strip_prefix("boolean:") {
        (Some("boolean"), r)
    } else if let Some(r) = function.strip_prefix("string:") {
        (Some("String"), r)
    } else {
        (None, function)
    };
    let exp = do_create(f, index)?;
    Ok(match type_name {
        Some(t) => typed(exp, t),
        None => exp,
    })
}

fn do_create(function: &str, index: usize) -> ParseResult<SharedExpression> {
    if function.len() > 1 && function.starts_with('!') {
        let name = function[1..].trim().to_string();
        let exp = do_create(&name, index)?;
        return Ok(named_expression(format!("!${{{name}}}"), move |e| {
            Ok(Value::Bool(
                !camel_util::object_helper::evaluate_value_predicate(&exp.evaluate(e)?),
            ))
        }));
    }
    if let Some(e) = direct_function(function) {
        return Ok(e);
    }
    for factory in [
        body_function,
        header_function,
        variable_function,
        exchange_function,
        // the order of the Java SimpleFunctionDispatcher
        super::other_functions::other_function,
        super::math_functions::math_function,
        super::string_functions::string_function,
        super::collection_functions::collection_function,
        super::misc_functions::misc_function,
    ] {
        if let Some(e) = factory(function, index)? {
            return Ok(e);
        }
    }
    Err(ParserError::new(
        format!("Unknown function: {function}"),
        index,
    ))
}

/// The exception of the exchange, or the caught one (Java
/// `ExpressionBuilder.exchangeExceptionExpression`: the `CamelExceptionCaught` property).
fn exchange_exception(e: &Exchange) -> Option<CamelError> {
    e.exception().cloned().or_else(|| {
        e.property(camel_api::exchange_constants::EXCEPTION_CAUGHT)
            .and_then(|v| v.downcast_ref::<CamelError>())
            .cloned()
    })
}

/// The functions without arguments (Java `DirectFunctionFactory`).
fn direct_function(function: &str) -> Option<SharedExpression> {
    let e: SharedExpression = match function {
        // the message id defaults to the exchange id (Java MessageSupport.createMessageId)
        "id" => named_expression("id", |e| {
            Ok(Value::from(
                e.message().message_id().unwrap_or(e.exchange_id()),
            ))
        }),
        "messageTimestamp" => named_expression("messageTimestamp", |e| {
            Ok(Value::Int(e.message().message_timestamp()))
        }),
        "exchangeId" => named_expression("exchangeId", |e| Ok(Value::from(e.exchange_id()))),
        "exception" => named_expression("exception", |e| {
            Ok(exchange_exception(e)
                .as_ref()
                .map(|x| Value::from(format!("{}: {x}", camel_api::error::error_type_name(x))))
                .unwrap_or_default())
        }),
        "exception.message" => named_expression("exception.message", |e| {
            Ok(exchange_exception(e)
                .as_ref()
                .map(|x| Value::from(x.to_string()))
                .unwrap_or_default())
        }),
        "exception.stacktrace" => named_expression("exception.stacktrace", |e| {
            Ok(exchange_exception(e)
                .as_ref()
                .map(|x| {
                    let mut lines = vec![format!("{}: {x}", camel_api::error::error_type_name(x))];
                    let mut cause = std::error::Error::source(x);
                    while let Some(c) = cause {
                        lines.push(format!(
                            "Caused by: {}: {c}",
                            camel_api::error::error_type_name(c)
                        ));
                        cause = c.source();
                    }
                    Value::from(lines.join("\n"))
                })
                .unwrap_or_default())
        }),
        "threadId" => named_expression("threadId", |_| {
            Ok(Value::from(format!("{:?}", std::thread::current().id())))
        }),
        "threadName" => named_expression("threadName", |_| {
            Ok(Value::from(
                std::thread::current().name().unwrap_or("main").to_string(),
            ))
        }),
        "hostname" => named_expression("hostname", |_| {
            // Java InetAddressUtil.getLocalHostNameSafe
            Ok(camel_util::host_utils::get_local_host_name_safe()
                .map(Value::from)
                .unwrap_or_default())
        }),
        "camelId" => named_expression("camelId", |e| Ok(Value::from(e.context().name()))),
        "routeId" | "fromRouteId" => {
            let name = function.to_string();
            named_expression(name, |e| {
                Ok(e.from_route_id().map(Value::from).unwrap_or_default())
            })
        }
        "routeGroup" => named_expression("routeGroup", |_| Ok(Value::Null)),
        "stepId" => named_expression("stepId", |e| {
            Ok(e.property("CamelStepId").cloned().unwrap_or_default())
        }),
        "null" => constant_expression(Value::Null),
        _ => return None,
    };
    Some(e)
}

fn starts_ognl(r: &str) -> bool {
    r.starts_with('.') || r.starts_with('?') || r.starts_with('[')
}

/// The body functions (Java `BodyFunctionFactory`).
fn body_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    match function {
        "body" | "in.body" => {
            return Ok(Some(named_expression("body", |e| {
                Ok(e.message().body().clone())
            })))
        }
        "bodyType" => {
            return Ok(Some(named_expression("bodyType", |e| {
                let b = e.message().body();
                Ok(if b.is_null() {
                    Value::Null
                } else {
                    Value::from(b.type_name())
                })
            })))
        }
        "bodyOneLine" => {
            return Ok(Some(named_expression("bodyOneLine", |e| {
                let b = e.message().body();
                Ok(if b.is_null() {
                    Value::Null
                } else {
                    Value::from(b.to_string().replace(['\n', '\r'], ""))
                })
            })))
        }
        _ => {}
    }
    for (prefix, mandatory) in [("bodyAs(", false), ("mandatoryBodyAs(", true)] {
        if let Some(remainder) = if_starts_with_return_remainder(prefix, function) {
            let name = if mandatory {
                "mandatoryBodyAs"
            } else {
                "bodyAs"
            };
            let Some(end) = remainder.find(')') else {
                return Err(ParserError::new(
                    format!("Valid syntax: ${{{name}(type)}} was: {function}"),
                    index,
                ));
            };
            // the type may contain nested functions: ${bodyAs(${header.type})}
            let type_text = remove_quotes(&remainder[..end]);
            let type_name = name_expression(&type_text);
            let rest = remainder[end + 1..].to_string();
            if !rest.is_empty() && ognl_helper::is_invalid_valid_ognl_expression(&rest) {
                return Err(ParserError::new(
                    format!("Valid syntax: ${{{name}(type).OGNL}} was: {function}"),
                    index,
                ));
            }
            let description = function.to_string();
            return Ok(Some(named_expression(description.clone(), move |e| {
                let body = e.message().body().clone();
                let type_name = type_name.evaluate(e)?.to_string();
                let v = convert_to_type(&body, &type_name)?;
                if mandatory && v.is_null() {
                    return Err(CamelError::new(
                        ErrorKind::InvalidPayload,
                        format!("No body available of type: {type_name} but has type: {} on: Message[{}]. Exchange[{}]",
                            if body.is_null() { "null".to_string() } else { body.type_name().to_string() },
                            e.message().message_id().unwrap_or_default(),
                            e.exchange_id()),
                    ));
                }
                if rest.is_empty() {
                    Ok(v)
                } else {
                    navigate(v, &rest, &description)
                }
            })));
        }
    }
    let remainder = if_starts_with_return_remainder("body", function)
        .or_else(|| if_starts_with_return_remainder("in.body", function));
    if let Some(r) = remainder {
        if !starts_ognl(r) {
            return Ok(None);
        }
        if ognl_helper::is_invalid_valid_ognl_expression(r) {
            return Err(ParserError::new(
                format!("Valid syntax: ${{body.OGNL}} was: {function}"),
                index,
            ));
        }
        let r = r.to_string();
        return Ok(Some(named_expression(format!("bodyOgnl({r})"), move |e| {
            navigate(e.message().body().clone(), &r, "body")
        })));
    }
    Ok(None)
}

fn key_and_type(remainder: &str) -> Option<(String, String, String)> {
    let end = index_of_closing_parenthesis(remainder)?;
    let key_and_type = &remainder[..end];
    let (key, ty) = key_and_type.split_once(',')?;
    Some((
        remove_quotes(key),
        remove_quotes(ty),
        remainder[end + 1..].to_string(),
    ))
}

fn scoped_function(
    kind: &'static str,
    valid: &str,
    remainder: &str,
    function: &str,
    index: usize,
    lookup: Lookup,
) -> ParseResult<SharedExpression> {
    let mut r = remainder;
    if r.starts_with('.') || r.starts_with(':') || r.starts_with('?') {
        r = &r[1..];
    }
    if r.starts_with('[') && r.ends_with(']') {
        let inner = &r[1..r.len() - 1];
        let unquoted = remove_leading_and_ending_quotes(inner);
        if unquoted != inner {
            return Ok(scoped_expression(kind, &unquoted, lookup));
        }
        r = inner;
    }
    let key = remove_leading_and_ending_quotes(r);
    if ognl_helper::is_invalid_valid_ognl_expression(&key) {
        return Err(ParserError::new(
            format!("Valid syntax: {valid} was: {function}"),
            index,
        ));
    }
    // a key with nested functions is evaluated (Java simpleExpression), not navigated
    if ognl_helper::is_valid_ognl_expression(&key)
        && !super::tokenizer::has_function_start_token(&key)
    {
        Ok(ognl_expression(kind, &key, lookup))
    } else {
        Ok(scoped_expression(kind, &key, lookup))
    }
}

/// The header functions (Java `HeaderFunctionFactory`).
fn header_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(remainder) = if_starts_with_return_remainder("headerAs(", function) {
        let invalid = || {
            ParserError::new(
                format!("Valid syntax: ${{headerAs(key, type)}} was: {function}"),
                index,
            )
        };
        let (key, ty, rest) = key_and_type(remainder).ok_or_else(invalid)?;
        if key.is_empty() || ty.is_empty() || !rest.is_empty() {
            return Err(invalid());
        }
        let k = name_expression(&key);
        return Ok(Some(named_expression(
            format!("headerAs({key}, {ty})"),
            move |e| {
                let name = k.evaluate(e)?.to_string();
                convert_to_type(&e.message().header(&name).cloned().unwrap_or_default(), &ty)
            },
        )));
    }
    match function {
        "in.headers" | "headers" => {
            return Ok(Some(named_expression("headers", |e| {
                let mut m = camel_util::value::ValueMap::new();
                for (k, v) in e.message().headers().iter() {
                    m.insert(k.to_string(), v.clone());
                }
                Ok(Value::Map(m))
            })))
        }
        "headers.size" | "headers.size()" | "headers.length" | "headers.length()" => {
            return Ok(Some(named_expression("headers.size", |e| {
                Ok(Value::Int(e.message().headers().len() as i64))
            })))
        }
        _ => {}
    }
    match parse_in_header(function) {
        Some(r) => scoped_function(
            "header",
            "${header.name[key]}",
            r,
            function,
            index,
            header_lookup,
        )
        .map(Some),
        None => Ok(None),
    }
}

/// The variable functions (Java `VariableFunctionFactory`).
fn variable_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(remainder) = if_starts_with_return_remainder("variableAs(", function) {
        let invalid = || {
            ParserError::new(
                format!("Valid syntax: ${{variableAs(key, type)}} was: {function}"),
                index,
            )
        };
        let (key, ty, rest) = key_and_type(remainder).ok_or_else(invalid)?;
        if key.is_empty() || ty.is_empty() || !rest.is_empty() {
            return Err(invalid());
        }
        return Ok(Some(named_expression(
            format!("variableAs({key}, {ty})"),
            move |e| convert_to_type(&e.variable(&key).cloned().unwrap_or_default(), &ty),
        )));
    }
    match function {
        "variables" => {
            return Ok(Some(named_expression("variables", |e| {
                Ok(Value::Map(e.variables().clone()))
            })))
        }
        "variables.size" | "variables.size()" | "variables.length" | "variables.length()" => {
            return Ok(Some(named_expression("variables.size", |e| {
                Ok(Value::Int(e.variables().len() as i64))
            })))
        }
        _ => {}
    }
    match parse_variable(function) {
        Some(r) => scoped_function(
            "variable",
            "${variable.name[key]}",
            r,
            function,
            index,
            variable_lookup,
        )
        .map(Some),
        None => Ok(None),
    }
}

/// The exchange property functions (Java `ExchangeFunctionFactory`).
fn exchange_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(r) = if_starts_with_return_remainder("exchangeProperty", function) {
        if starts_ognl(r) || r.starts_with(':') {
            return scoped_function(
                "exchangeProperty",
                "${exchangeProperty.OGNL}",
                r,
                function,
                index,
                property_lookup,
            )
            .map(Some);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(
            Some("foo"),
            if_starts_with_return_remainder("header.", "header.foo")
        );
        assert_eq!(None, if_starts_with_return_remainder("header.", "header."));
        assert_eq!(Some(9), index_of_closing_parenthesis("a,'b)'(c)) x"));
        assert_eq!(Some(".foo"), parse_in_header("header.foo"));
        assert_eq!(Some(":foo"), parse_in_header("headers:foo"));
        assert_eq!(None, parse_in_header("headerX"));
        assert_eq!(Some(".x"), parse_variable("variable.x"));
        assert_eq!(("foo", ".bar[0]"), split_first_ognl("foo.bar[0]"));
        assert_eq!(
            Value::Int(12),
            convert_to_type(&Value::from("12"), "java.lang.Integer").unwrap()
        );
        assert!(convert_to_type(&Value::from("12"), "com.Foo").is_err());
    }
}
