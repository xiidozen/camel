//! # camel-jsonpath
//!
//! The `jsonpath` language (Java `JsonPathLanguage`, on Jayway JsonPath): JSONPath
//! expressions and predicates on the JSON of the message body (a JSON text or bytes, or maps
//! and lists), or of a header, property or variable (`source`). A definite path (only
//! properties and single indexes, such as `$.store.bicycle.price`) gives a value, any other
//! (such as `$.store.book[*].author` or `$..book[?(@.price < 10)]`) a list; a predicate
//! matches a non empty list or a true value. Predicates may be written as "easy predicates"
//! (`store.book.price < 10` for `$.store.book[?(@.price < 10)]`), and simple functions
//! (`${header.cheap}`) are evaluated in the expression first. This crate is the Rust port of the
//! Java `components/camel-jsonpath` module; see [`path::JsonPath`] for the JSONPath engine.
//!
//! # Options (see [`JsonPathBuilder`])
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `suppressExceptions` | `false` | Whether to suppress exceptions such as PathNotFoundException. |
//! | `allowSimple` | `true` | Whether to allow in inlined Simple exceptions in the JSONPath expression. |
//! | `allowEasyPredicate` | `true` | Whether to allow using the easy predicate parser to pre-parse predicates. |
//! | `writeAsString` | `false` | Whether to write the output of each row/element as a JSON String value instead of a Map/POJO value. |
//! | `unpackArray` | `false` | Whether to unpack a single element json-array into an object. |
//! | `option` | | To configure additional options on JSONPath (`DEFAULT_PATH_LEAF_TO_NULL`, `ALWAYS_RETURN_LIST`, `SUPPRESS_EXCEPTIONS`). |
//! | `source` | | Source to use, instead of message body (`header:name`, `variable:name`, `exchangeProperty:name`). |

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod path;

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, Predicate, SharedExpression, SharedPredicate};
use camel_api::spi::language::Language;
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::Value;
use path::{JsonPath, PathOptions};
use std::sync::{Arc, Mutex};

/// The name of the language.
pub const NAME: &str = "jsonpath";

/// The header of the encoding of the JSON (Java `JsonPathConstants.HEADER_JSON_ENCODING`).
pub const HEADER_JSON_ENCODING: &str = "CamelJsonPathJsonEncoding";

const OPERATORS: [&str; 11] = [
    "==", "!=", "<", "<=", ">", ">=", "=~", "in", "nin", "size", "empty",
];

fn has_operator(exp: &str) -> bool {
    OPERATORS.iter().any(|o| exp.contains(&format!(" {o}")))
}

/// Rewrites an easy predicate (`store.book.price < 10`) as a JSONPath filter
/// (`$.store.book[?(@.price < 10)]`) (Java `EasyPredicateParser`).
///
/// # Examples
///
/// ```
/// use camel_jsonpath::easy_predicate;
///
/// assert_eq!("$.store.book[?(@.price < 10)]", easy_predicate("store.book.price < 10"));
/// assert_eq!("$.store.book[?(@.price < 10)]", easy_predicate("$.store.book.price < 10"));
/// assert_eq!("$..*[?(@.price < 10)]", easy_predicate("price < 10"));
/// assert_eq!("$.store.book", easy_predicate("$.store.book"));
/// ```
pub fn easy_predicate(predicate: &str) -> String {
    let mut predicate = predicate.to_string();
    let mut rooted = false;
    if let Some(rest) = predicate.strip_prefix('$') {
        if predicate.contains("[?(") || !has_operator(&predicate) {
            return predicate;
        }
        rooted = true;
        predicate = rest.strip_prefix('.').unwrap_or(rest).to_string();
    }
    if !has_operator(&predicate) {
        return predicate;
    }
    // the tokens: the operand, the operator, the operand
    let mut parts: Vec<String> = Vec::new();
    let mut part = String::new();
    let mut i = 0;
    while i < predicate.len() {
        let rest = &predicate[i..];
        if let Some(op) = OPERATORS
            .iter()
            .filter(|o| rest.starts_with(&format!(" {o}")))
            .max_by_key(|o| o.len())
        {
            if !part.is_empty() {
                parts.push(std::mem::take(&mut part));
            }
            parts.push(op.to_string());
            i += op.len() + 2;
        } else {
            let ch = rest.chars().next().unwrap();
            part.push(ch);
            i += ch.len_utf8();
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    if parts.len() == 3 && OPERATORS.contains(&parts[1].as_str()) {
        let (prev, op, next) = (&parts[0], &parts[1], &parts[2]);
        let (before, after) = match prev.rfind('.') {
            None => (if rooted { "" } else { "..*" }, prev.as_str()),
            Some(pos) => (&prev[..pos], &prev[pos + 1..]),
        };
        let mut sb = String::from("$");
        if !before.is_empty() && !before.starts_with('.') {
            sb.push('.');
        }
        sb.push_str(before);
        sb.push_str(&format!("[?(@.{after} {op} {next})]"));
        return sb;
    }
    predicate
}

/// The hint for a comparison written on a path (Java `JsonPathExpression.comparisonHint`).
///
/// # Examples
///
/// ```
/// use camel_jsonpath::comparison_hint;
///
/// let hint = comparison_hint("$.status == 'paid'").unwrap();
/// assert!(hint.contains("$[?(@.status == 'paid')]"));
/// assert!(hint.contains("${body[status]} == 'paid'"));
/// assert!(comparison_hint("$.store.book[?(@.price < 10)]").is_none());
/// ```
pub fn comparison_hint(exp: &str) -> Option<String> {
    if exp.contains("[?(") || !has_operator(exp) {
        return None;
    }
    let field = exp
        .strip_prefix("$.")
        .or_else(|| exp.strip_prefix('$'))
        .unwrap_or(exp);
    let (left, rest) = match field.find(' ') {
        Some(sp) if sp > 0 => (&field[..sp], &field[sp..]),
        _ => (field, ""),
    };
    let simple = if left.contains('.') {
        String::new()
    } else {
        format!("; to keep the value use the path $.{left} and compare it in simple, ${{body[{left}]}}{rest}")
    };
    Some(format!(
        "a comparison is a predicate: use it where a predicate is expected (filter, choice when), or select with a JSONPath filter $[?(@.{left}{rest})]{simple}"
    ))
}

/// Decodes JSON bytes (RFC 4627 detection of the UTF-16 and UTF-32 encodings, Java
/// `JsonStream`), or with the encoding of the header.
fn decode_json(bytes: &[u8], encoding: Option<&str>) -> String {
    let utf16 = |be: bool, b: &[u8]| {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match encoding.map(|e| e.to_ascii_uppercase()) {
        Some(e) if e == "UTF-16BE" => return utf16(true, bytes),
        Some(e) if e == "UTF-16LE" => return utf16(false, bytes),
        Some(e) if e != "UTF-8" => {
            if let Ok(cs) = camel_util::charset::Charset::for_name(&e) {
                return cs.decode(bytes);
            }
        }
        _ => {}
    }
    match bytes {
        [0xFE, 0xFF, rest @ ..] => utf16(true, rest),
        [0xFF, 0xFE, rest @ ..] => utf16(false, rest),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).to_string(),
        [0, a, ..] if *a != 0 => utf16(true, bytes),
        [a, 0, b, ..] if *a != 0 && *b != 0 => utf16(false, bytes),
        _ => String::from_utf8_lossy(bytes).to_string(),
    }
}

/// The source of the JSON (Java `source`): the body, or a header, property or variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The message body.
    Body,
    /// The header.
    Header(String),
    /// The exchange property.
    Property(String),
    /// The variable.
    Variable(String),
}

impl Source {
    /// The source of the text: `header:name`, `exchangeProperty:name` (or `property:`),
    /// `variable:name`, else the body.
    pub fn parse(text: &str) -> Self {
        if let Some(h) = text.strip_prefix("header:") {
            Source::Header(h.to_string())
        } else if let Some(p) = text
            .strip_prefix("exchangeProperty:")
            .or_else(|| text.strip_prefix("property:"))
        {
            Source::Property(p.to_string())
        } else if let Some(v) = text.strip_prefix("variable:") {
            Source::Variable(v.to_string())
        } else {
            Source::Body
        }
    }

    fn value(&self, e: &Exchange) -> Value {
        match self {
            Source::Body => e.message().body().clone(),
            Source::Header(h) => e.message().header(h).cloned().unwrap_or_default(),
            Source::Property(p) => e.property(p).cloned().unwrap_or_default(),
            Source::Variable(v) => e.variable(v).cloned().unwrap_or_default(),
        }
    }

    fn describe(&self) -> String {
        match self {
            Source::Body => "body".to_string(),
            Source::Header(h) => format!("header:{h}"),
            Source::Property(p) => format!("exchangeProperty:{p}"),
            Source::Variable(v) => format!("variable:{v}"),
        }
    }
}

/// The options of a jsonpath expression.
#[derive(Debug, Clone)]
pub struct JsonPathOptions {
    /// The errors (such as a missing path) give null.
    pub suppress_exceptions: bool,
    /// The simple functions of the expression are evaluated first.
    pub allow_simple: bool,
    /// The predicates may be easy predicates.
    pub allow_easy_predicate: bool,
    /// The results are JSON texts.
    pub write_as_string: bool,
    /// A list of one element gives the element.
    pub unpack_array: bool,
    /// The Jayway options.
    pub path_options: PathOptions,
    /// The source of the JSON.
    pub source: Source,
    /// The type the result is converted to.
    pub result_type: Option<String>,
}

impl Default for JsonPathOptions {
    fn default() -> Self {
        JsonPathOptions {
            suppress_exceptions: false,
            allow_simple: true,
            allow_easy_predicate: true,
            write_as_string: false,
            unpack_array: false,
            path_options: PathOptions::default(),
            source: Source::Body,
            result_type: None,
        }
    }
}

/// A jsonpath expression (Java `JsonPathExpression`).
pub struct JsonPathExpression {
    expression: String,
    options: JsonPathOptions,
    /// The compiled path (without simple functions).
    compiled: Mutex<Option<Arc<JsonPath>>>,
}

impl JsonPathExpression {
    /// The expression (or predicate) with the options; a static path is compiled now.
    ///
    /// # Errors
    ///
    /// An invalid path (Java `ExpressionIllegalSyntaxException`).
    pub fn new(expression: &str, predicate: bool, options: JsonPathOptions) -> CamelResult<Self> {
        let mut exp = expression.to_string();
        if predicate && options.allow_easy_predicate {
            exp = easy_predicate(&exp);
        }
        let has_simple = options.allow_simple && exp.contains("${");
        let compiled = if has_simple {
            None
        } else {
            Some(Arc::new(JsonPath::compile(&exp).map_err(|e| {
                let hint = comparison_hint(&exp)
                    .map(|h| format!(" ({h})"))
                    .unwrap_or_default();
                CamelError::with_cause(
                    ErrorKind::ExpressionIllegalSyntax,
                    format!("Illegal syntax: {exp}{hint}"),
                    e,
                )
            })?))
        };
        Ok(JsonPathExpression {
            expression: exp,
            options,
            compiled: Mutex::new(compiled),
        })
    }

    /// The JSON of the source.
    fn json(&self, e: &Exchange) -> CamelResult<Value> {
        if let Source::Variable(v) = &self.options.source {
            if e.variable(v).is_none() {
                return Err(CamelError::camel_exchange(
                    &format!("No '{v}' variable available"),
                    e,
                ));
            }
        }
        let input = self.options.source.value(e);
        let encoding = e
            .message()
            .header(HEADER_JSON_ENCODING)
            .map(|v| v.to_string());
        Ok(match input {
            Value::Map(_) | Value::List(_) => input,
            Value::Text(t) => camel_support::json::parse(&t)?,
            Value::Bytes(b) => camel_support::json::parse(&decode_json(&b, encoding.as_deref()))?,
            Value::Null => {
                let message = if self.options.source == Source::Body {
                    "The jsonpath expression got no message body to evaluate (the body is null): a route reached with direct: has the body of its caller, so when the caller has none, read the data first - setBody with constant: resource:file:data.json for a known file, or poll:, pollEnrich or a from: consumer for one that is not".to_string()
                } else {
                    format!(
                        "The jsonpath expression got no input from {} to evaluate (it is null): set it before the step, or leave source unset to use the message body",
                        self.options.source.describe()
                    )
                };
                return Err(CamelError::camel_exchange(&message, e));
            }
            other => {
                let (input, what) = if self.options.source == Source::Body {
                    ("body".to_string(), "the body")
                } else {
                    (self.options.source.describe(), "it")
                };
                let type_name = match &other {
                    Value::Int(_) => "java.lang.Integer",
                    Value::Float(_) => "java.lang.Double",
                    Value::Bool(_) => "java.lang.Boolean",
                    o => o.type_name(),
                };
                return Err(CamelError::camel_exchange(
                    &format!("Cannot read {input} as supported JSON value ({what} has type: {type_name})"),
                    e,
                ));
            }
        })
    }

    fn read(&self, e: &Exchange) -> CamelResult<Value> {
        let path = match self.compiled.lock().unwrap().clone() {
            Some(p) => p,
            None => {
                // the simple functions evaluated first
                let simple = e
                    .context()
                    .resolve_language("simple")?
                    .create_expression(&self.expression)?;
                let text = simple.evaluate(e)?.to_string();
                Arc::new(JsonPath::compile(&text)?)
            }
        };
        let mut path_options = self.options.path_options;
        path_options.suppress_exceptions |= self.options.suppress_exceptions;
        let json = match self.json(e) {
            Ok(j) => j,
            Err(err)
                if path_options.suppress_exceptions || path_options.default_path_leaf_to_null =>
            {
                let _ = err;
                return Ok(if path_options.always_return_list {
                    Value::List(Vec::new())
                } else {
                    Value::Null
                });
            }
            Err(err) => return Err(err),
        };
        let mut answer = path.read(&json, path_options)?;
        if self.options.write_as_string {
            answer = match answer {
                Value::List(l) => Value::List(
                    l.iter()
                        .map(|v| match v {
                            Value::Text(t) => Value::Text(t.clone()),
                            other => Value::from(camel_support::json::stringify(other, false)),
                        })
                        .collect(),
                ),
                Value::Text(t) => Value::Text(t),
                Value::Null => Value::Null,
                other => Value::from(camel_support::json::stringify(&other, false)),
            };
        }
        Ok(answer)
    }

    fn evaluate_value(&self, e: &Exchange) -> CamelResult<Value> {
        let result = self.read(e).map_err(|err| {
            if err.is(ErrorKind::CamelExchange) {
                return err;
            }
            let hint = comparison_hint(&self.expression)
                .map(|h| format!(" ({h})"))
                .unwrap_or_default();
            CamelError::with_cause(
                ErrorKind::ExpressionEvaluation,
                format!("{}{hint}", err.message().unwrap_or_default()),
                err,
            )
        })?;
        let collection = matches!(
            self.options.result_type.as_deref(),
            Some("java.util.List" | "List" | "java.util.Collection" | "java.util.ArrayList")
        );
        let mut result = result;
        if self.options.unpack_array && !collection {
            if let Value::List(l) = &result {
                if l.len() == 1 {
                    result = l[0].clone();
                }
            }
        }
        let Some(t) = &self.options.result_type else {
            return Ok(result);
        };
        if collection {
            return Ok(match result {
                Value::List(l) => Value::List(l),
                other => Value::List(vec![other]),
            });
        }
        if let Value::List(l) = result {
            let mut converted: Vec<Value> = l
                .iter()
                .filter(|v| !v.is_null())
                .map(|v| camel_support::type_conversion::convert_to_type(v, t))
                .collect::<CamelResult<_>>()?;
            return Ok(if converted.len() == 1 {
                converted.remove(0)
            } else {
                Value::List(converted)
            });
        }
        camel_support::type_conversion::convert_to_type(&result, t)
    }
}

/// The truthiness of a predicate result (Java `ObjectHelper.evaluateValuePredicate`).
fn matches_value(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Text(t) if t.eq_ignore_ascii_case("true") => true,
        Value::Text(t) if t.eq_ignore_ascii_case("false") => false,
        Value::List(l) => !l.is_empty(),
        Value::Map(m) => !m.is_empty(),
        _ => true,
    }
}

impl Expression for JsonPathExpression {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.evaluate_value(exchange)
    }
}

impl Predicate for JsonPathExpression {
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        Ok(matches_value(&self.evaluate_value(exchange)?))
    }
}

impl std::fmt::Display for JsonPathExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "jsonpath[{}]", self.expression)
    }
}

/// The jsonpath language (Java `JsonPathLanguage`), with its default options.
#[derive(Default)]
pub struct JsonPathLanguage {
    options: Mutex<JsonPathOptions>,
}

impl JsonPathLanguage {
    /// The language with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// The language with the default options of its expressions.
    pub fn with_options(options: JsonPathOptions) -> Self {
        JsonPathLanguage {
            options: Mutex::new(options),
        }
    }
}

impl Language for JsonPathLanguage {
    fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        let options = self.options.lock().unwrap().clone();
        Ok(Arc::new(JsonPathExpression::new(
            expression, true, options,
        )?))
    }

    fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
        let options = self.options.lock().unwrap().clone();
        Ok(Arc::new(JsonPathExpression::new(
            expression, false, options,
        )?))
    }

    fn set_camel_context(&self, context: std::sync::Weak<dyn CamelContext>) {
        let _ = context;
    }
}

/// Builds a jsonpath expression or predicate with options for the Rust DSL (Java
/// `.jsonpath(expression, suppressExceptions, resultType, headerName)`).
#[derive(Debug, Clone)]
pub struct JsonPathBuilder {
    expression: String,
    options: JsonPathOptions,
}

/// A jsonpath expression or predicate of the Rust DSL; see [`JsonPathBuilder`].
pub fn jsonpath(expression: &str) -> JsonPathBuilder {
    JsonPathBuilder {
        expression: expression.to_string(),
        options: JsonPathOptions::default(),
    }
}

impl JsonPathBuilder {
    /// With the errors (such as a missing path) giving null.
    pub fn suppress_exceptions(mut self, suppress: bool) -> Self {
        self.options.suppress_exceptions = suppress;
        self
    }

    /// With the simple functions of the expression evaluated first (default `true`).
    pub fn allow_simple(mut self, allow: bool) -> Self {
        self.options.allow_simple = allow;
        self
    }

    /// With the easy predicates (default `true`).
    pub fn allow_easy_predicate(mut self, allow: bool) -> Self {
        self.options.allow_easy_predicate = allow;
        self
    }

    /// With the results as JSON texts.
    pub fn write_as_string(mut self, write: bool) -> Self {
        self.options.write_as_string = write;
        self
    }

    /// With a list of one element unpacked.
    pub fn unpack_array(mut self, unpack: bool) -> Self {
        self.options.unpack_array = unpack;
        self
    }

    /// With the Jayway options.
    pub fn path_options(mut self, options: PathOptions) -> Self {
        self.options.path_options = options;
        self
    }

    /// With the source of the JSON (`header:name`, `exchangeProperty:name`, `variable:name`).
    pub fn source(mut self, source: &str) -> Self {
        self.options.source = Source::parse(source);
        self
    }

    /// With the header as the source of the JSON (Java `headerName`).
    pub fn header_name(mut self, header: &str) -> Self {
        self.options.source = Source::Header(header.to_string());
        self
    }

    /// With the type the result is converted to (such as `String`, `int`, `java.util.List`).
    pub fn result_type(mut self, result_type: &str) -> Self {
        self.options.result_type = Some(result_type.to_string());
        self
    }

    /// The expression.
    ///
    /// # Panics
    ///
    /// An invalid path (as Java fails creating the route).
    pub fn expression(self) -> camel_core_model::expression::ExpressionDefinition {
        let e = JsonPathExpression::new(&self.expression, false, self.options)
            .unwrap_or_else(|e| panic!("{e}"));
        camel_core_model::expression::ExpressionDefinition::Expression(Arc::new(e))
    }

    /// The predicate.
    ///
    /// # Panics
    ///
    /// An invalid path (as Java fails creating the route).
    pub fn predicate(self) -> camel_core_model::expression::ExpressionDefinition {
        let p = JsonPathExpression::new(&self.expression, true, self.options)
            .unwrap_or_else(|e| panic!("{e}"));
        camel_core_model::expression::ExpressionDefinition::Predicate(Arc::new(p))
    }
}

impl From<JsonPathBuilder> for camel_core_model::expression::ExpressionDefinition {
    /// The expression (use [`JsonPathBuilder::predicate`] where a predicate is expected).
    fn from(b: JsonPathBuilder) -> Self {
        b.expression()
    }
}

/// Registers the language on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_language_factory(
        NAME,
        Arc::new(|| Arc::new(JsonPathLanguage::new()) as Arc<dyn Language>),
    );
}
