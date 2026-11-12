//! The functions of the simple language (Java `SimpleFunctionExpression`, the
//! `DirectFunctionFactory` and the built-in function factories): `${body}`, `${header.foo}`,
//! `${exchangeProperty.foo}`, `${variable.foo}`, `${exchangeId}`, `${bodyAs(String)}`, ...
//!
//! Ported so far: the direct functions, the body, header, exchange property and variable
//! functions (with map / list navigation for their OGNL suffixes), and the `int:` / `long:`
//! / `boolean:` / `string:` conversions. The other function factories (date, math, string,
//! collection, random, ...) follow; an unknown function is a syntax error, as in Java.

use super::ognl::invoke_ognl;
use super::{ParseResult, ParserError};
use camel_api::error::{CamelError, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::SharedExpression;
use camel_api::spi::simple_function::SimpleFunctionRegistry;
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

/// The key and the OGNL methods of a keyed OGNL expression (Java
/// `KeyedOgnlExpressionAdapter`): `foo.bar` is the key `foo` and the methods `.bar`, `[a.b].c`
/// the key `a.b` and the methods `.c`.
fn split_keyed_ognl(ognl: &str) -> (String, Option<String>) {
    let methods = ognl_helper::split_ognl(ognl).unwrap_or_default();
    let first = methods.first().cloned().unwrap_or_else(|| ognl.to_string());
    let mut key = first.clone();
    let mut suffix = String::new();
    // a key inside brackets (eg: [foo.bar]) keeps its dots
    if key.starts_with('[') && key.ends_with(']') {
        key = remove_leading_and_ending_quotes(&key[1..key.len() - 1]);
        suffix = camel_util::string_helper::after(&first, &key).unwrap_or_default();
    }
    // remove any OGNL operators so we got the pure key name
    let key = ognl_helper::remove_operators(&key);
    let method =
        camel_util::string_helper::after(ognl, &format!("{key}{suffix}")).filter(|m| !m.is_empty());
    (key, method)
}

/// The value of a header, exchange property or variable navigated with OGNL (Java
/// `KeyedOgnlExpressionAdapter`): the full text (with its nested functions evaluated) is
/// tried as the name first, then the first part is the name and the rest is invoked on its
/// value.
fn ognl_expression(kind: &'static str, ognl: &str, lookup: Lookup) -> SharedExpression {
    let (key, method) = split_keyed_ognl(ognl);
    let full = name_expression(ognl);
    let key = name_expression(&key);
    named_expression(format!("{kind}Ognl({ognl})"), move |e| {
        // try with full name first
        if let Some(v) = lookup(e, &full.evaluate(e)?.to_string()) {
            return Ok(v);
        }
        let Some(v) = lookup(e, &key.evaluate(e)?.to_string()) else {
            return Ok(Value::Null);
        };
        match &method {
            Some(m) if !v.is_null() => invoke_ognl(&v, m, e),
            _ => Ok(v),
        }
    })
}

pub use camel_support::type_conversion::convert_to_type;

fn typed(exp: SharedExpression, type_name: &'static str) -> SharedExpression {
    named_expression(exp.describe(), move |e| {
        convert_to_type(&exp.evaluate(e)?, type_name)
    })
}

thread_local! {
    /// The custom functions of the context of the expression being parsed.
    static CUSTOM_FUNCTIONS: std::cell::RefCell<Option<Arc<SimpleFunctionRegistry>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs the parsing with the custom functions of the registry known (Java: the parser looks
/// up `SimpleFunctionRegistry` of its context).
pub fn with_custom_functions<R>(
    registry: Option<Arc<SimpleFunctionRegistry>>,
    parse: impl FnOnce() -> R,
) -> R {
    let previous = CUSTOM_FUNCTIONS.with(|c| c.replace(registry));
    let answer = parse();
    CUSTOM_FUNCTIONS.with(|c| *c.borrow_mut() = previous);
    answer
}

/// A call of a custom function without the `function(...)` wrapper: `name` or
/// `name(arguments)` of a registered function.
fn custom_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let (name, arguments) = match function.split_once('(') {
        Some((n, a)) => (n, Some(a)),
        None => (function, None),
    };
    let known = CUSTOM_FUNCTIONS.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|r| r.function(name).is_some())
    });
    if !known {
        return Ok(None);
    }
    let call = match arguments {
        Some(a) if a.trim() == ")" => format!("{name})"),
        Some(a) => format!("{name},{a}"),
        None => format!("{name})"),
    };
    super::init_block::function_call(&call, index).map(Some)
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

/// A predicate written inside the braces (`${body != null && body.size() > 0}`), as an
/// expression that answers whether it matches; `None` when the text is not a predicate but a
/// plain function (Java `SimpleFunctionExpression.createPredicateExpression`, CAMEL-24921).
///
/// An operator counts only when whitespace surrounds it outside quotes, so
/// `${header.Content-Length}` and `${date:now:yyyy-MM-dd}` are names, not arithmetic.
fn predicate_expression(function: &str) -> ParseResult<Option<SharedExpression>> {
    if super::syntax_hints::operators_outside(function).is_none() {
        return Ok(None);
    }
    let text = super::syntax_hints::wrap_functions(function);
    let predicate = super::predicate_parser::SimplePredicateParser::new(&text, true)
        .parse_predicate()
        .map_err(|e| super::ast::nested_error(&text, &e))?;
    Ok(Some(named_expression(text, move |e| {
        Ok(Value::Bool(predicate.matches(e)?))
    })))
}

fn do_create(function: &str, index: usize) -> ParseResult<SharedExpression> {
    // ${body != null && body.size() > 0}: the braces hold a predicate, which is what they hold
    // in EL, Groovy and a JavaScript template, so read it as one (CAMEL-24921)
    if let Some(p) = predicate_expression(function)? {
        return Ok(p);
    }
    // ${!body.isEmpty()}: a ! in front of a single function negates what it answers. It is
    // read after the predicate above, so a ! in ${!a && b} negates a and not the whole
    // predicate (CAMEL-24984)
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
    if let Some(r) = if_starts_with_return_remainder("function(", function) {
        return super::init_block::function_call(r, index);
    }
    if let Some(e) = super::bean_functions::bean_function(function, index)? {
        return Ok(e);
    }
    if let Some(e) = super::properties_functions::properties_function(function, index)? {
        return Ok(e);
    }
    if let Some(remainder) = if_starts_with_return_remainder("file:", function) {
        return super::file_functions::file_function(remainder, index);
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
        super::date_functions::date_function,
    ] {
        if let Some(e) = factory(function, index)? {
            return Ok(e);
        }
    }
    if let Some(e) = custom_function(function, index)? {
        return Ok(e);
    }
    let hint = super::syntax_hints::unknown_function(function)
        .map(|h| format!(" ({h})"))
        .unwrap_or_default();
    Err(ParserError::new(
        format!("Unknown function: {function}{hint}"),
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
        // the message history table, with the exchange (Java messageHistoryExpression)
        "messageHistory" | "messageHistory(true)" => {
            named_expression("messageHistory(true)", |e| {
                use camel_support::exchange_formatter::{DefaultExchangeFormatter, OutputStyle};
                let formatter = DefaultExchangeFormatter {
                    show_exchange_id: true,
                    multiline: true,
                    show_headers: true,
                    style: OutputStyle::Fixed,
                    ..Default::default()
                };
                Ok(Value::from(
                    camel_support::message_helper::dump_message_history_stacktrace(
                        e,
                        Some(&formatter),
                        false,
                    ),
                ))
            })
        }
        "messageHistory(false)" => named_expression("messageHistory(false)", |e| {
            Ok(Value::from(
                camel_support::message_helper::dump_message_history_stacktrace(e, None, false),
            ))
        }),
        // the body of the in message when the unit of work started (Java
        // originalBodyExpression), null without one
        "originalBody" => named_expression("originalBody", |e| {
            Ok(e.original_message()
                .map(|m| m.body().clone())
                .unwrap_or_default())
        }),
        // the exception object, whose text is the Java toString(): `type: message`
        "exception" => named_expression("exception", |e| {
            Ok(exchange_exception(e)
                .as_ref()
                .map(camel_api::beans::exception_value)
                .unwrap_or_default())
        }),
        "exchange" => named_expression("exchange", |e| Ok(camel_api::beans::exchange_value(e))),
        "logExchange" => named_expression("logExchange", |e| {
            use camel_api::spi::exchange_formatter::ExchangeFormatter;
            use camel_support::exchange_formatter::DefaultExchangeFormatter;
            Ok(Value::from(DefaultExchangeFormatter::default().format(e)))
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
        "routeId" => named_expression("routeId", |e| {
            Ok(e.route_id().map(Value::from).unwrap_or_default())
        }),
        "fromRouteId" => named_expression("fromRouteId", |e| {
            Ok(e.from_route_id().map(Value::from).unwrap_or_default())
        }),
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
                if rest.is_empty() || v.is_null() {
                    Ok(v)
                } else {
                    invoke_ognl(&v, &rest, e)
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
            let body = e.message().body();
            if body.is_null() {
                return Ok(Value::Null);
            }
            invoke_ognl(body, &r, e)
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
    if ognl_helper::is_valid_ognl_expression(&key) {
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
    if let Some(r) = if_starts_with_return_remainder("camelContext", function) {
        if starts_ognl(r) {
            if ognl_helper::is_invalid_valid_ognl_expression(r) {
                return Err(ParserError::new(
                    format!("Valid syntax: ${{camelContext.OGNL}} was: {function}"),
                    index,
                ));
            }
            let r = r.to_string();
            return Ok(Some(named_expression(
                format!("camelContextOgnl({r})"),
                move |e| {
                    invoke_ognl(
                        &camel_api::beans::context_value(e.context().as_ref()),
                        &r,
                        e,
                    )
                },
            )));
        }
    }
    // ${exception.OGNL} and ${exchange.OGNL} (exchangeProperty is checked before exchange)
    if let Some(r) = if_starts_with_return_remainder("exception", function) {
        if starts_ognl(r) {
            if ognl_helper::is_invalid_valid_ognl_expression(r) {
                return Err(ParserError::new(
                    format!("Valid syntax: ${{exception.OGNL}} was: {function}"),
                    index,
                ));
            }
            let r = r.to_string();
            return Ok(Some(named_expression(
                format!("exchangeExceptionOgnl({r})"),
                move |e| match exchange_exception(e) {
                    Some(x) => invoke_ognl(&camel_api::beans::exception_value(&x), &r, e),
                    None => Ok(Value::Null),
                },
            )));
        }
    }
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
    if let Some(r) = if_starts_with_return_remainder("exchange", function) {
        if starts_ognl(r) {
            if ognl_helper::is_invalid_valid_ognl_expression(r) {
                return Err(ParserError::new(
                    format!("Valid syntax: ${{exchange.OGNL}} was: {function}"),
                    index,
                ));
            }
            let r = r.to_string();
            return Ok(Some(named_expression(
                format!("exchangeOgnl({r})"),
                move |e| invoke_ognl(&camel_api::beans::exchange_value(e), &r, e),
            )));
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
        let keyed = |s: &str| {
            let (k, m) = split_keyed_ognl(s);
            (k, m.unwrap_or_default())
        };
        assert_eq!(("foo".into(), ".bar[0]".into()), keyed("foo.bar[0]"));
        assert_eq!(("a.b".into(), ".c".into()), keyed("[a.b].c"));
        assert_eq!(("a b".into(), ".c".into()), keyed("['a b'].c"));
        assert_eq!(
            ("ITEMS".into(), "[${exchangeProperty.i}]".into()),
            keyed("ITEMS[${exchangeProperty.i}]")
        );
        assert_eq!(
            Value::Int(12),
            convert_to_type(&Value::from("12"), "java.lang.Integer").unwrap()
        );
        assert!(convert_to_type(&Value::from("12"), "com.Foo").is_err());
    }
}
