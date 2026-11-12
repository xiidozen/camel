//! The YAML DSL (port of `dsl/camel-yaml-dsl`): routes written in YAML, parsed into the same
//! model as the Rust route DSL.
//!
//! ```yaml
//! - route:
//!     id: greetings
//!     from:
//!       uri: direct:start
//!       steps:
//!         - setBody:
//!             simple: "Hello ${body}"
//!         - choice:
//!             when:
//!               - simple: "${header.lang} == 'fr'"
//!                 steps:
//!                   - setBody:
//!                       constant: Bonjour
//!             otherwise:
//!               steps:
//!                 - to: mock:other
//!         - to: mock:result
//! ```
//!
//! The top level elements are `route`, `from` (a route without options), `onException`,
//! `errorHandler`, `intercept`, `interceptFrom`, `interceptSendToEndpoint` and `beans` (of the
//! known framework types, see [`YamlRoutes::bind_beans`]). The steps are the
//! EIPs of the Rust model with their YAML names (camelCase or kebab-case keys). As in Java, an
//! EIP with nested steps (such as `split` or `loop`) given without `steps` takes the steps
//! that follow it in the list.
//!
//! An expression is a language key with the text (`simple: "${body}"`, `constant: 3`,
//! `header: foo`), a language key with a map (`simple: {expression: "${body}"}`), or an
//! `expression` block holding one of these.
//!
//! Not available yet: `rest`, `routeTemplate`, `routeConfiguration`, `kamelet`, data
//! formats (`marshal`/`unmarshal`) and the EIPs not ported to the Rust model.
//!
//! # Examples
//!
//! ```
//! use camel_core::prelude::*;
//! use camel_yaml_dsl::YamlRoutes;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let routes = YamlRoutes::parse(r#"
//! - from:
//!     uri: direct:start
//!     steps:
//!       - setBody:
//!           simple: "Hello ${body}"
//!       - to: mock:result
//! "#).unwrap();
//! let context = camel_core::default_context();
//! context.add_routes(routes).await.unwrap();
//! context.start().await.unwrap();
//! let mock = MockEndpoint::resolve(context.as_ref(), "mock:result").unwrap();
//! mock.expected_bodies_received(["Hello World"]);
//! context.create_producer_template().send_body("direct:start", "World").await.unwrap();
//! mock.assert_is_satisfied().await;
//! context.stop().await.unwrap();
//! # });
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::{CamelError, CamelResult, ErrorKind, ErrorType};
use camel_core_model::builder::*;
use camel_core_model::errorhandler::{ErrorHandlerDefinition, OnExceptionDefinition};
use camel_core_model::expression::ExpressionDefinition;
use camel_core_model::processor::ProcessorDefinition;
use camel_core_model::route::RouteDefinition;
/// The YAML tree of the documents (`serde_yaml::Value`).
pub use serde_yaml::Value as Yaml;

/// The languages an expression may be written in (the keys of an expression).
pub const LANGUAGES: &[&str] = &[
    "simple",
    "constant",
    "header",
    "exchangeProperty",
    "variable",
    "method",
    "bean",
    "ref",
    "jq",
    "jsonpath",
    "xpath",
    "xquery",
    "groovy",
    "joor",
    "java",
    "js",
    "python",
    "mvel",
    "ognl",
    "spel",
    "tokenize",
    "xtokenize",
    "csimple",
    "datasonnet",
    "hl7terser",
    "wasm",
    "language",
];

/// The EIPs whose nested steps are the following steps when given without `steps`.
pub const OUTPUT_EIPS: &[&str] = &[
    "filter",
    "split",
    "multicast",
    "loop",
    "pipeline",
    "step",
    "idempotentConsumer",
    "loadBalance",
    "resequence",
    "aggregate",
];

/// A YAML error (an `IllegalArgumentException`).
fn error(message: impl Into<String>) -> CamelError {
    CamelError::illegal_argument(message.into())
}

/// The key in camelCase (`do-while` is `doWhile`).
fn camel_case(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The text of a scalar.
fn text(v: &Yaml) -> Option<String> {
    match v {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Number(n) => Some(n.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        Yaml::Tagged(t) => text(&t.value),
        _ => None,
    }
}

/// A node with its keys in camelCase.
struct Node {
    entries: Vec<(String, Yaml)>,
}

impl Node {
    fn of(v: &Yaml) -> Node {
        let entries = match v {
            Yaml::Mapping(m) => m
                .iter()
                .filter_map(|(k, v)| text(k).map(|k| (camel_case(&k), v.clone())))
                .collect(),
            _ => Vec::new(),
        };
        Node { entries }
    }

    fn get(&self, key: &str) -> Option<&Yaml> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn text(&self, key: &str) -> Option<String> {
        self.get(key).and_then(text)
    }

    fn required(&self, key: &str, eip: &str) -> CamelResult<String> {
        self.text(key)
            .ok_or_else(|| error(format!("{eip}: the {key} option is required")))
    }

    fn bool(&self, key: &str) -> CamelResult<Option<bool>> {
        match self.get(key) {
            None => Ok(None),
            Some(Yaml::Bool(b)) => Ok(Some(*b)),
            Some(v) => match text(v).as_deref() {
                Some("true") => Ok(Some(true)),
                Some("false") => Ok(Some(false)),
                _ => Err(error(format!("{key}: expected a boolean"))),
            },
        }
    }

    fn number<T: std::str::FromStr>(&self, key: &str) -> CamelResult<Option<T>> {
        match self.text(key) {
            None => Ok(None),
            Some(t) => t
                .trim()
                .parse::<T>()
                .map(Some)
                .map_err(|_| error(format!("{key}: expected a number, was: {t}"))),
        }
    }

    /// The expression of the node: a language key, or an `expression` block (exactly one, as
    /// the Java schema validation).
    fn expression(&self) -> CamelResult<Option<ExpressionDefinition>> {
        let inline: Vec<&(String, Yaml)> = self
            .entries
            .iter()
            .filter(|(k, _)| LANGUAGES.contains(&k.as_str()))
            .collect();
        let block = self.get("expression");
        let count = inline.len() + usize::from(block.is_some());
        if count > 1 {
            return Err(error(format!(
                "the expression must be valid to one and only one schema, but {count} are valid"
            )));
        }
        if let Some(block) = block {
            let b = Node::of(block);
            return match b.expression()? {
                Some(e) => Ok(Some(e)),
                None => Err(error(
                    "the expression must be valid to one and only one schema, but 0 are valid",
                )),
            };
        }
        match inline.first() {
            Some((k, v)) => language_expression(k, v).map(Some),
            None => Ok(None),
        }
    }

    fn required_expression(&self, eip: &str) -> CamelResult<ExpressionDefinition> {
        self.expression()?
            .ok_or_else(|| error(format!("{eip}: an expression is required")))
    }

    /// The expression of the option (`correlationExpression: {simple: ..}`).
    fn expression_option(&self, key: &str) -> CamelResult<Option<ExpressionDefinition>> {
        match self.get(key) {
            Some(v) => Node::of(v).expression(),
            None => Ok(None),
        }
    }
}

/// The expression of a language key and its value (a text, or a map with `expression`).
fn language_expression(language: &str, value: &Yaml) -> CamelResult<ExpressionDefinition> {
    let node = Node::of(value);
    if language == "method" || language == "bean" {
        // method: {ref: myBean, method: hello} or method: "myBean.hello"
        if let Some(t) = text(value) {
            return Ok(ExpressionDefinition::language("bean", &t));
        }
        let bean = node
            .text("ref")
            .or_else(|| node.text("beanType"))
            .ok_or_else(|| error("method: the ref option is required"))?;
        return Ok(match node.text("method") {
            Some(m) => method(&bean, &m),
            None => method_ref(&bean),
        });
    }
    if language == "language" {
        let name = node.required("language", "language")?;
        let expression = node.required("expression", "language")?;
        return Ok(ExpressionDefinition::language(&name, &expression));
    }
    let expression = match text(value) {
        Some(t) => t,
        None => node.required("expression", language)?,
    };
    let mut def = ExpressionDefinition::language(language, &expression);
    if let Some(t) = node.text("resultType") {
        def = def.result_type(&t);
    }
    if node.bool("trim")? == Some(true) {
        def = def.trim(true);
    }
    if node.bool("pretty")? == Some(true) {
        def = def.pretty(true);
    }
    if node.bool("nested")? == Some(true) {
        def = def.nested(true);
    }
    Ok(def)
}

/// The uri with the `parameters` of the node as its query.
fn endpoint_uri(node: &Node, eip: &str) -> CamelResult<String> {
    let mut uri = node.required("uri", eip)?;
    if let Some(Yaml::Mapping(params)) = node.get("parameters") {
        let mut query = Vec::new();
        for (k, v) in params {
            if let (Some(k), Some(v)) = (text(k), text(v)) {
                query.push(format!("{k}={v}"));
            }
        }
        if !query.is_empty() {
            uri.push(if uri.contains('?') { '&' } else { '?' });
            uri.push_str(&query.join("&"));
        }
    }
    Ok(uri)
}

/// The uri of a step given as a text or with `uri`.
fn uri_of(value: &Yaml, eip: &str) -> CamelResult<String> {
    match text(value) {
        Some(t) => Ok(t),
        None => endpoint_uri(&Node::of(value), eip),
    }
}

fn push_all(mut dsl: StepsDsl, steps: Vec<ProcessorDefinition>) -> StepsDsl {
    for s in steps {
        dsl = dsl.push(s);
    }
    dsl
}

fn nested(steps: Vec<ProcessorDefinition>) -> impl FnOnce(StepsDsl) -> StepsDsl {
    move |s| push_all(s, steps)
}

/// Parses the steps of a list (Java `steps:`).
///
/// # Errors
///
/// An unknown EIP, or an invalid option.
pub fn parse_steps(list: &Yaml) -> CamelResult<Vec<ProcessorDefinition>> {
    let items: Vec<Yaml> = match list {
        Yaml::Sequence(s) => s.clone(),
        Yaml::Null => Vec::new(),
        _ => return Err(error("steps: expected a list")),
    };
    parse_step_items(&items)
}

fn parse_step_items(items: &[Yaml]) -> CamelResult<Vec<ProcessorDefinition>> {
    let mut answer = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let node = Node::of(item);
        let Some((eip, value)) = node.entries.first().cloned() else {
            return Err(error("a step must be a map with the name of the EIP"));
        };
        let has_steps = Node::of(&value).get("steps").is_some();
        if OUTPUT_EIPS.contains(&eip.as_str()) && !has_steps {
            // the following steps are the nested steps (as the Java DSL without end())
            let following = parse_step_items(&items[i + 1..])?;
            answer.push(step(&eip, &value, Some(following))?);
            return Ok(answer);
        }
        answer.push(step(&eip, &value, None)?);
    }
    Ok(answer)
}

fn steps_of(
    node: &Node,
    following: Option<Vec<ProcessorDefinition>>,
) -> CamelResult<Vec<ProcessorDefinition>> {
    match following {
        Some(f) => Ok(f),
        None => match node.get("steps") {
            Some(s) => parse_steps(s),
            None => Ok(Vec::new()),
        },
    }
}

/// The only step of the DSL.
fn one(dsl: StepsDsl) -> ProcessorDefinition {
    dsl.into_steps().pop().expect("the step created by the DSL")
}

fn error_type(name: &str) -> ErrorType {
    ErrorType::from_java_name(name)
}

fn error_types(node: &Node) -> Vec<ErrorType> {
    match node.get("exception") {
        Some(Yaml::Sequence(l)) => l.iter().filter_map(text).map(|t| error_type(&t)).collect(),
        Some(v) => text(v).map(|t| vec![error_type(&t)]).unwrap_or_default(),
        None => vec![ErrorType::Any],
    }
}

/// Creates the processor definition of one EIP.
fn step(
    eip: &str,
    value: &Yaml,
    following: Option<Vec<ProcessorDefinition>>,
) -> CamelResult<ProcessorDefinition> {
    let node = Node::of(value);
    let d = StepsDsl::default();
    let mut def = match eip {
        "to" => match &node.text("pattern") {
            Some(p) => {
                let pattern: ExchangePattern = p
                    .parse()
                    .map_err(|_| error(format!("to: unknown pattern {p}")))?;
                one(d.to_with_pattern(pattern, &uri_of(value, eip)?))
            }
            None => one(d.to(&uri_of(value, eip)?)),
        },
        "toD" => one(d.to_d(&uri_of(value, eip)?)),
        "wireTap" => one(d.wire_tap(&uri_of(value, eip)?)),
        "log" => match text(value) {
            Some(m) => one(d.log(&m)),
            None => {
                let message = node.required("message", eip)?;
                match (node.text("loggingLevel"), node.text("logName")) {
                    (None, None) => one(d.log(&message)),
                    (level, name) => {
                        let level: LoggingLevel = level
                            .as_deref()
                            .unwrap_or("INFO")
                            .parse()
                            .map_err(|_| error("log: unknown loggingLevel"))?;
                        one(d.log_with(level, name.as_deref().unwrap_or(""), &message))
                    }
                }
            }
        },
        "setBody" => one(d.set_body(node.required_expression(eip)?)),
        "transform" => one(d.transform(node.required_expression(eip)?)),
        "setHeader" => {
            one(d.set_header(&node.required("name", eip)?, node.required_expression(eip)?))
        }
        "setProperty" => {
            one(d.set_property(&node.required("name", eip)?, node.required_expression(eip)?))
        }
        "setVariable" => {
            one(d.set_variable(&node.required("name", eip)?, node.required_expression(eip)?))
        }
        "setHeaders" | "setVariables" => {
            let key = if eip == "setHeaders" {
                "headers"
            } else {
                "variables"
            };
            let mut items = Vec::new();
            if let Some(Yaml::Sequence(l)) = node.get(key) {
                for item in l {
                    let n = Node::of(item);
                    items.push((n.required("name", eip)?, n.required_expression(eip)?));
                }
            }
            if eip == "setHeaders" {
                one(d.set_headers(items))
            } else {
                one(d.set_variables(items))
            }
        }
        "removeHeader" => {
            one(d.remove_header(&text(value).map_or_else(|| node.required("name", eip), Ok)?))
        }
        "removeHeaders" => {
            let pattern = text(value).map_or_else(|| node.required("pattern", eip), Ok)?;
            match node.text("excludePattern") {
                Some(e) => {
                    let excludes: Vec<&str> = e.split(',').collect();
                    one(d.remove_headers_excluding(&pattern, &excludes))
                }
                None => one(d.remove_headers(&pattern)),
            }
        }
        "removeProperty" => {
            one(d.remove_property(&text(value).map_or_else(|| node.required("name", eip), Ok)?))
        }
        "removeProperties" => {
            let pattern = text(value).map_or_else(|| node.required("pattern", eip), Ok)?;
            match node.text("excludePattern") {
                Some(e) => {
                    let excludes: Vec<&str> = e.split(',').collect();
                    one(d.remove_properties_excluding(&pattern, &excludes))
                }
                None => one(d.remove_properties(&pattern)),
            }
        }
        "removeVariable" => {
            one(d.remove_variable(&text(value).map_or_else(|| node.required("name", eip), Ok)?))
        }
        "convertBodyTo" => {
            let type_name = text(value).map_or_else(|| node.required("type", eip), Ok)?;
            if node.bool("mandatory")? == Some(false) {
                one(d.convert_body_to_optional(&type_name))
            } else {
                one(d.convert_body_to(&type_name))
            }
        }
        "convertHeaderTo" => {
            one(d.convert_header_to(&node.required("name", eip)?, &node.required("type", eip)?))
        }
        "convertVariableTo" => {
            one(d.convert_variable_to(&node.required("name", eip)?, &node.required("type", eip)?))
        }
        "setExchangePattern" => {
            let p = text(value).map_or_else(|| node.required("pattern", eip), Ok)?;
            let pattern: ExchangePattern = p
                .parse()
                .map_err(|_| error(format!("setExchangePattern: unknown pattern {p}")))?;
            one(d.set_exchange_pattern(pattern))
        }
        "filter" => {
            let predicate = node.required_expression(eip)?;
            one(d.filter(predicate, nested(steps_of(&node, following)?)))
        }
        "choice" => {
            let mut whens = Vec::new();
            if let Some(Yaml::Sequence(l)) = node.get("when") {
                for (i, w) in l.iter().enumerate() {
                    let n = Node::of(w);
                    for (k, _) in &n.entries {
                        let known = LANGUAGES.contains(&k.as_str())
                            || matches!(
                                k.as_str(),
                                "expression" | "steps" | "id" | "description" | "disabled"
                            );
                        if !known {
                            return Err(error(format!(
                                "choice/when/{i}: property '{k}' is not defined in the schema and the schema does not allow additional properties"
                            )));
                        }
                    }
                    let predicate = n.expression()?.ok_or_else(|| {
                        error(format!("choice/when/{i}: an expression is required"))
                    })?;
                    whens.push((predicate, steps_of(&n, None)?));
                }
            }
            let otherwise = match node.get("otherwise") {
                Some(o) => Some(steps_of(&Node::of(o), None)?),
                None => None,
            };
            one(d.choice(move |mut c| {
                for (p, s) in whens {
                    c = c.when(p, nested(s));
                }
                if let Some(o) = otherwise {
                    c = c.otherwise(nested(o));
                }
                c
            }))
        }
        "pipeline" => one(d.pipeline(nested(steps_of(&node, following)?))),
        "step" => one(d.step(
            &node.text("id").unwrap_or_default(),
            nested(steps_of(&node, following)?),
        )),
        "split" => {
            let mut s = d.split(
                node.required_expression(eip)?,
                nested(steps_of(&node, following)?),
            );
            s = multicast_options(s, &node)?;
            if let Some(delim) = node.text("delimiter") {
                s = s.delimiter(&delim);
            }
            one(s)
        }
        "multicast" => one(multicast_options(
            d.multicast(nested(steps_of(&node, following)?)),
            &node,
        )?),
        "recipientList" => {
            let mut s = multicast_options(d.recipient_list(node.required_expression(eip)?), &node)?;
            if let Some(delim) = node.text("delimiter") {
                s = s.uri_delimiter(&delim);
            }
            one(s)
        }
        "routingSlip" | "dynamicRouter" => {
            let e = node.required_expression(eip)?;
            let mut s = if eip == "routingSlip" {
                d.routing_slip(e)
            } else {
                d.dynamic_router(e)
            };
            if let Some(delim) = node.text("uriDelimiter") {
                s = s.uri_delimiter(&delim);
            }
            if node.bool("ignoreInvalidEndpoints")? == Some(true) {
                s = s.ignore_invalid_endpoints();
            }
            one(s)
        }
        "delay" => match text(value) {
            Some(t) => one(d.delay(language("constant", &t))),
            None => one(d.delay(node.required_expression(eip)?)),
        },
        "loop" => {
            let steps = steps_of(&node, following)?;
            let e = node.required_expression(eip)?;
            let mut s = if node.bool("doWhile")? == Some(true) {
                d.loop_do_while(e, nested(steps))
            } else {
                d.loop_count(e, nested(steps))
            };
            if node.bool("copy")? == Some(true) {
                s = s.copy();
            }
            one(s)
        }
        "stop" => one(d.stop()),
        "bean" => {
            let bean = node
                .text("ref")
                .or_else(|| node.text("beanType"))
                .ok_or_else(|| error("bean: the ref option is required"))?;
            match node.text("method") {
                Some(m) => one(d.bean_method(bean.as_str(), &m)),
                None => one(d.bean(bean.as_str())),
            }
        }
        "process" => {
            one(d.process_ref(&text(value).map_or_else(|| node.required("ref", eip), Ok)?))
        }
        "throwException" => {
            let message = node.text("message").unwrap_or_default();
            let error = match node.text("exceptionType") {
                Some(t) => match ErrorKind::from_java_name(t.rsplit('.').next().unwrap_or(&t)) {
                    Some(kind) => CamelError::new(kind, message),
                    None => CamelError::new(ErrorKind::Runtime, message),
                },
                None => CamelError::new(ErrorKind::Runtime, message),
            };
            one(d.throw_exception(error))
        }
        "doTry" => {
            let steps = steps_of(&node, None)?;
            let mut catches = Vec::new();
            if let Some(Yaml::Sequence(l)) = node.get("doCatch") {
                for c in l {
                    let n = Node::of(c);
                    let on_when = match n.get("onWhen") {
                        Some(w) => Node::of(w).expression()?,
                        None => None,
                    };
                    catches.push((error_types(&n), on_when, steps_of(&n, None)?));
                }
            }
            let finally = match node.get("doFinally") {
                Some(f) => Some(steps_of(&Node::of(f), None)?),
                None => None,
            };
            one(d.do_try(move |t| {
                let mut t = push_all_try(t, steps);
                for (types, on_when, s) in catches {
                    t = match on_when {
                        Some(w) => t.do_catch_when(types, w, nested(s)),
                        None => t.do_catch(types, nested(s)),
                    };
                }
                if let Some(f) = finally {
                    t = t.do_finally(nested(f));
                }
                t
            }))
        }
        "enrich" | "pollEnrich" => {
            let uri = match node.expression()? {
                Some(ExpressionDefinition::Language { expression, .. }) => expression,
                _ => node.required("uri", eip)?,
            };
            let mut s = if eip == "enrich" {
                d.enrich(&uri)
            } else {
                d.poll_enrich(&uri, node.number::<i64>("timeout")?.unwrap_or(-1))
            };
            if let Some(r) = node.text("aggregationStrategy") {
                s = s.aggregation_strategy_ref(&r);
            }
            if node.bool("aggregateOnException")? == Some(true) {
                s = s.aggregate_on_exception(true);
            }
            if node.bool("ignoreInvalidEndpoint")? == Some(true) {
                s = s.ignore_invalid_endpoints();
            }
            one(s)
        }
        "validate" => one(d.validate(node.required_expression(eip)?)),
        "script" => one(d.script(node.required_expression(eip)?)),
        "sort" => match node.expression()? {
            Some(e) => one(d.sort(e)),
            None => one(d.sort_body()),
        },
        "sample" => match (
            node.number::<u64>("messageFrequency")?,
            node.text("samplePeriod"),
        ) {
            (Some(n), _) => one(d.sample_messages(n)),
            (None, Some(p)) => {
                let millis = camel_util::time_utils::to_milli_seconds(&p)
                    .map_err(|e| error(format!("sample: {e}")))?;
                one(d.sample_period(millis as u64))
            }
            (None, None) => one(d.sample()),
        },
        "throttle" => {
            let mut s = d.throttle(node.required_expression(eip)?);
            if let Some(p) = node.text("timePeriodMillis") {
                let millis = camel_util::time_utils::to_milli_seconds(&p)
                    .map_err(|e| error(format!("throttle: {e}")))?;
                s = s.time_period_millis(millis as u64);
            }
            if node.text("mode").as_deref() == Some("ConcurrentRequests") {
                s = s.concurrent_requests_mode();
            }
            if let Some(c) = node.expression_option("correlationExpression")? {
                s = s.correlation_expression(c);
            }
            if node.bool("rejectExecution")? == Some(true) {
                s = s.reject_execution(true);
            }
            one(s)
        }
        "idempotentConsumer" => {
            let repository = node.required("idempotentRepository", eip)?;
            let mut s = d.idempotent_consumer(
                node.required_expression(eip)?,
                repository.as_str(),
                nested(steps_of(&node, following)?),
            );
            if let Some(b) = node.bool("eager")? {
                s = s.eager(b);
            }
            if let Some(b) = node.bool("skipDuplicate")? {
                s = s.skip_duplicate(b);
            }
            if let Some(b) = node.bool("removeOnFailure")? {
                s = s.remove_on_failure(b);
            }
            if let Some(b) = node.bool("completionEager")? {
                s = s.completion_eager(b);
            }
            one(s)
        }
        "loadBalance" => {
            let balancer = load_balancer(&node)?;
            one(d.load_balance(balancer, nested(steps_of(&node, following)?)))
        }
        "resequence" => {
            let mut s = d.resequence(
                node.required_expression(eip)?,
                nested(steps_of(&node, following)?),
            );
            if let Some(stream) = node.get("streamConfig") {
                s = s.stream();
                let n = Node::of(stream);
                if let Some(c) = n.number::<usize>("capacity")? {
                    s = s.capacity(c);
                }
                if let Some(t) = n.number::<u64>("timeout")? {
                    s = s.timeout(t);
                }
                if n.bool("rejectOld")? == Some(true) {
                    s = s.reject_old();
                }
                if n.bool("ignoreInvalidExchanges")? == Some(true) {
                    s = s.ignore_invalid_exchanges();
                }
            } else if let Some(batch) = node.get("batchConfig") {
                let n = Node::of(batch);
                if let Some(size) = n.number::<usize>("batchSize")? {
                    s = s.batch_size(size);
                }
                if let Some(t) = n.number::<u64>("batchTimeout")? {
                    s = s.timeout(t);
                }
                if n.bool("allowDuplicates")? == Some(true) {
                    s = s.allow_duplicates();
                }
                if n.bool("reverse")? == Some(true) {
                    s = s.reverse();
                }
                if n.bool("ignoreInvalidExchanges")? == Some(true) {
                    s = s.ignore_invalid_exchanges();
                }
            }
            one(s)
        }
        "aggregate" => {
            let correlation = node
                .expression_option("correlationExpression")?
                .ok_or_else(|| error("aggregate: the correlationExpression is required"))?;
            let strategy = node.required("aggregationStrategy", eip)?;
            let mut s =
                d.aggregate_ref(correlation, &strategy, nested(steps_of(&node, following)?));
            if let Some(n) = node.number::<i32>("completionSize")? {
                s = s.completion_size(n);
            }
            if let Some(t) = node.text("completionTimeout") {
                let millis = camel_util::time_utils::to_milli_seconds(&t)
                    .map_err(|e| error(format!("aggregate: {e}")))?;
                s = s.completion_timeout(millis);
            }
            if let Some(p) = node.expression_option("completionPredicate")? {
                s = s.completion_predicate(p);
            }
            one(s)
        }
        "claimCheck" => one(d.claim_check(
            &node.required("operation", eip)?,
            node.text("key").as_deref(),
            node.text("filter").as_deref(),
        )),
        "rollback" => {
            if node.bool("markRollbackOnlyLast")? == Some(true) {
                one(d.mark_rollback_only_last())
            } else if node.bool("markRollbackOnly")? == Some(true) {
                one(d.mark_rollback_only())
            } else {
                match node.text("message") {
                    Some(m) => one(d.rollback_message(&m)),
                    None => one(d.rollback()),
                }
            }
        }
        "markRollbackOnly" => one(d.mark_rollback_only()),
        "threads" => {
            let mut s = d.threads();
            if let Some(n) = node.number::<i32>("poolSize")? {
                s = s.pool_size(n);
            }
            if let Some(n) = node.number::<i32>("maxPoolSize")? {
                s = s.max_pool_size(n);
            }
            if let Some(n) = node.number::<i32>("maxQueueSize")? {
                s = s.max_queue_size(n);
            }
            if let Some(n) = node.number::<u64>("keepAliveTime")? {
                s = s.keep_alive_time(n);
            }
            if let Some(n) = node.text("threadName") {
                s = s.thread_name(&n);
            }
            if let Some(p) = node.text("rejectedPolicy") {
                let policy: camel_api::enums::ThreadPoolRejectedPolicy = p
                    .parse()
                    .map_err(|_| error(format!("threads: unknown rejectedPolicy {p}")))?;
                s = s.rejected_policy(policy);
            }
            if let Some(b) = node.bool("callerRunsWhenRejected")? {
                s = s.caller_runs_when_rejected(b);
            }
            one(s)
        }
        other => {
            return Err(error(format!(
                "Unknown or not yet supported YAML DSL node: {other}"
            )))
        }
    };
    if let Some(id) = node.text("id") {
        if eip != "step" {
            def.id = Some(id);
        }
    }
    if let Some(desc) = node.text("description") {
        def.description = Some(desc);
    }
    if node.bool("disabled")? == Some(true) {
        def.disabled = true;
    }
    Ok(def)
}

fn push_all_try(mut t: TryDsl, steps: Vec<ProcessorDefinition>) -> TryDsl {
    for s in steps {
        t = t.push(s);
    }
    t
}

/// The options of split, multicast and recipient list.
fn multicast_options(mut s: StepsDsl, node: &Node) -> CamelResult<StepsDsl> {
    if node.bool("parallelProcessing")? == Some(true) {
        s = s.parallel_processing();
    }
    if node.bool("streaming")? == Some(true) {
        s = s.streaming();
    }
    if node.bool("stopOnException")? == Some(true) {
        s = s.stop_on_exception();
    }
    if let Some(t) = node.number::<u64>("timeout")? {
        s = s.timeout(t);
    }
    if let Some(r) = node.text("aggregationStrategy") {
        s = s.aggregation_strategy_ref(&r);
    }
    Ok(s)
}

fn load_balancer(node: &Node) -> CamelResult<LoadBalancerDefinition> {
    if node.get("roundRobin").is_some() {
        return Ok(LoadBalancerDefinition::RoundRobin);
    }
    if node.get("random").is_some() {
        return Ok(LoadBalancerDefinition::Random);
    }
    if node.get("topic").is_some() {
        return Ok(LoadBalancerDefinition::Topic);
    }
    if let Some(s) = node.get("sticky") {
        let correlation = Node::of(s)
            .expression_option("correlationExpression")?
            .ok_or_else(|| error("sticky: the correlationExpression is required"))?;
        return Ok(LoadBalancerDefinition::Sticky(correlation));
    }
    if let Some(f) = node.get("failover") {
        let n = Node::of(f);
        let exceptions = match n.get("exception") {
            Some(_) => error_types(&n),
            None => Vec::new(),
        };
        return Ok(LoadBalancerDefinition::Failover {
            exceptions,
            round_robin: n.bool("roundRobin")?.unwrap_or(false),
            sticky: n.bool("sticky")?.unwrap_or(false),
            maximum_failover_attempts: n.number::<i32>("maximumFailoverAttempts")?.unwrap_or(-1),
        });
    }
    if let Some(w) = node.get("weighted") {
        let n = Node::of(w);
        let ratio = n.required("distributionRatio", "weighted")?;
        return Ok(LoadBalancerDefinition::Weighted {
            round_robin: n.bool("roundRobin")?.unwrap_or(false),
            distribution_ratio: ratio,
            delimiter: n
                .text("distributionRatioDelimiter")
                .unwrap_or_else(|| ",".into()),
        });
    }
    Err(error("loadBalance: the load balancer is required (roundRobin, random, sticky, topic, failover, weighted)"))
}

/// The redelivery options of an error handler or onException.
fn redelivery<T: RedeliveryOptions>(mut t: T, node: &Node) -> CamelResult<T> {
    let policy = match node.get("redeliveryPolicy") {
        Some(p) => Node::of(p),
        None => Node {
            entries: node.entries.clone(),
        },
    };
    if let Some(n) = policy.number::<i32>("maximumRedeliveries")? {
        t = t.maximum_redeliveries(n);
    }
    if let Some(d) = policy.text("redeliveryDelay") {
        let millis = camel_util::time_utils::to_milli_seconds(&d)
            .map_err(|e| error(format!("redeliveryDelay: {e}")))?;
        t = t.redelivery_delay(millis as u64);
    }
    if policy.bool("useExponentialBackOff")? == Some(true) {
        t = t.use_exponential_back_off();
    }
    if let Some(m) = policy.number::<f64>("backOffMultiplier")? {
        t = t.back_off_multiplier(m);
    }
    Ok(t)
}

fn error_handler(value: &Yaml) -> CamelResult<ErrorHandlerDefinition> {
    let node = Node::of(value);
    if let Some(dlc) = node.get("deadLetterChannel") {
        let n = Node::of(dlc);
        let uri = n.required("deadLetterUri", "deadLetterChannel")?;
        let mut b = redelivery(dead_letter_channel(&uri), &n)?;
        if n.bool("useOriginalMessage")? == Some(true) {
            b = b.use_original_message();
        }
        return Ok(b.into());
    }
    if let Some(def) = node.get("defaultErrorHandler") {
        return Ok(redelivery(default_error_handler(), &Node::of(def))?.into());
    }
    if node.get("noErrorHandler").is_some() {
        return Ok(no_error_handler().into());
    }
    Err(error(
        "errorHandler: one of deadLetterChannel, defaultErrorHandler or noErrorHandler is required",
    ))
}

fn on_exception_definition(value: &Yaml) -> CamelResult<OnExceptionDefinition> {
    let node = Node::of(value);
    let mut c = redelivery(on_exception(error_types(&node)), &node)?;
    if let Some(h) = node.get("handled") {
        c = match text(h).as_deref() {
            Some("true") => c.handled(true),
            Some("false") => c.handled(false),
            _ => c.handled_when(Node::of(h).required_expression("handled")?),
        };
    }
    if let Some(h) = node.get("continued") {
        c = match text(h).as_deref() {
            Some("true") => c.continued(true),
            Some("false") => c.continued(false),
            _ => c.continued_when(Node::of(h).required_expression("continued")?),
        };
    }
    if let Some(w) = node.get("onWhen") {
        if let Some(p) = Node::of(w).expression()? {
            c = c.on_when(p);
        }
    }
    if node.bool("useOriginalMessage")? == Some(true) {
        c = c.use_original_message();
    }
    for s in steps_of(&node, None)? {
        c = c.push(s);
    }
    Ok(c.into())
}

/// The route of a `from` node (`uri`, `parameters`, `steps`).
fn route(from: &Yaml) -> CamelResult<RouteDefinition> {
    let node = Node::of(from);
    let uri = match text(from) {
        Some(t) => t,
        None => endpoint_uri(&node, "from")?,
    };
    let mut def = RouteDefinition::new(uri);
    def.steps = steps_of(&node, None)?;
    if let Some(id) = node.text("id") {
        def.id = Some(id);
    }
    Ok(def)
}

/// A bean of the `beans` element: its name and type.
#[derive(Debug, Clone)]
pub struct BeanDefinition {
    /// The name in the registry.
    pub name: String,
    /// The Java type name.
    pub type_name: String,
}

/// The value of a bean of a known Java type (Rust has no class loading: only the types of
/// the framework are known).
fn bean_of_type(type_name: &str) -> CamelResult<camel_util::value::Value> {
    match type_name {
        "org.apache.camel.support.processor.idempotent.MemoryIdempotentRepository"
        | "MemoryIdempotentRepository" => Ok(camel_support::idempotent::repository_value(
            camel_support::idempotent::memory_idempotent_repository(),
        )),
        other => Err(error(format!(
            "beans: the type {other} cannot be created (the Rust YAML DSL has no class loading; bind the bean in the registry instead)"
        ))),
    }
}

/// One element of the top level list.
enum Element {
    Beans(Vec<BeanDefinition>),
    Route(Box<RouteDefinition>),
    ErrorHandler(ErrorHandlerDefinition),
    OnException(Box<OnExceptionDefinition>),
    Intercept(Option<ExpressionDefinition>, Vec<ProcessorDefinition>),
    InterceptFrom(
        Option<String>,
        Option<ExpressionDefinition>,
        Vec<ProcessorDefinition>,
    ),
    InterceptSendToEndpoint {
        uri: String,
        on_when: Option<ExpressionDefinition>,
        skip: bool,
        after_uri: Option<String>,
        steps: Vec<ProcessorDefinition>,
    },
}

/// The routes and the route builder options of a YAML document; add it to a context with
/// `add_routes`, as a [`RouteBuilder`].
pub struct YamlRoutes {
    elements: Vec<Element>,
}

impl YamlRoutes {
    /// Parses the YAML document.
    ///
    /// # Errors
    ///
    /// Invalid YAML, an unknown node or an invalid option (an `IllegalArgumentException`).
    pub fn parse(yaml: &str) -> CamelResult<Self> {
        let doc: Yaml =
            serde_yaml::from_str(yaml).map_err(|e| error(format!("Invalid YAML: {e}")))?;
        Self::from_value(doc)
    }

    /// The routes of a YAML document already parsed (used by the XML DSL, which converts its
    /// elements to the same tree).
    ///
    /// # Errors
    ///
    /// An unknown node or an invalid option (an `IllegalArgumentException`).
    pub fn from_value(doc: Yaml) -> CamelResult<Self> {
        let items = match doc {
            Yaml::Sequence(s) => s,
            Yaml::Null => Vec::new(),
            Yaml::Mapping(m) => vec![Yaml::Mapping(m)],
            _ => return Err(error("The YAML routes must be a list")),
        };
        let mut elements = Vec::new();
        for item in &items {
            let node = Node::of(item);
            let Some((key, value)) = node.entries.first() else {
                continue;
            };
            elements.push(match key.as_str() {
                "route" => {
                    let n = Node::of(value);
                    let from = n
                        .get("from")
                        .ok_or_else(|| error("route: the from option is required"))?;
                    let mut def = route(from)?;
                    if let Some(id) = n.text("id") {
                        def.id = Some(id);
                    }
                    if let Some(g) = n.text("group") {
                        def.group = Some(g);
                    }
                    if let Some(d) = n.text("description") {
                        def.description = Some(d);
                    }
                    if let Some(a) = n.bool("autoStartup")? {
                        def.auto_startup = a;
                    }
                    if let Some(o) = n.number::<i32>("startupOrder")? {
                        def.startup_order = Some(o);
                    }
                    if let Some(eh) = n.get("errorHandler") {
                        def.error_handler = Some(error_handler(eh)?);
                    }
                    Element::Route(Box::new(def))
                }
                "from" => Element::Route(Box::new(route(value)?)),
                "beans" => {
                    let mut beans = Vec::new();
                    if let Yaml::Sequence(l) = value {
                        for b in l {
                            let n = Node::of(b);
                            beans.push(BeanDefinition {
                                name: n.required("name", "beans")?,
                                type_name: n.required("type", "beans")?,
                            });
                        }
                    }
                    Element::Beans(beans)
                }
                "errorHandler" => Element::ErrorHandler(error_handler(value)?),
                "onException" => Element::OnException(Box::new(on_exception_definition(value)?)),
                "intercept" => {
                    let n = Node::of(value);
                    Element::Intercept(n.expression_option("onWhen")?, steps_of(&n, None)?)
                }
                "interceptFrom" => {
                    let n = Node::of(value);
                    Element::InterceptFrom(
                        n.text("uri"),
                        n.expression_option("onWhen")?,
                        steps_of(&n, None)?,
                    )
                }
                "interceptSendToEndpoint" => {
                    let n = Node::of(value);
                    Element::InterceptSendToEndpoint {
                        uri: n.required("uri", "interceptSendToEndpoint")?,
                        on_when: n.expression_option("onWhen")?,
                        skip: n.bool("skipSendToOriginalEndpoint")?.unwrap_or(false),
                        after_uri: n.text("afterUri"),
                        steps: steps_of(&n, None)?,
                    }
                }
                other => {
                    return Err(error(format!(
                        "Unknown or not yet supported YAML DSL element: {other}"
                    )))
                }
            });
        }
        Ok(YamlRoutes { elements })
    }

    /// The route definitions (with the route builder options applied).
    pub fn route_definitions(&self) -> Vec<RouteDefinition> {
        self.build()
    }

    /// The beans of the `beans` elements.
    pub fn beans(&self) -> Vec<BeanDefinition> {
        self.elements
            .iter()
            .flat_map(|e| match e {
                Element::Beans(b) => b.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// Binds the beans of the `beans` elements in the registry of the context; call it before
    /// adding the routes.
    ///
    /// # Errors
    ///
    /// A bean of an unknown type, or a context without registry.
    pub fn bind_beans(&self, context: &dyn camel_api::context::CamelContext) -> CamelResult<()> {
        let beans = self.beans();
        if beans.is_empty() {
            return Ok(());
        }
        let registry = context
            .registry()
            .ok_or_else(|| CamelError::illegal_state("The context has no registry"))?;
        for b in beans {
            registry.bind(&b.name, bean_of_type(&b.type_name)?)?;
        }
        Ok(())
    }
}

impl RouteBuilder for YamlRoutes {
    fn configure(&self, routes: &mut Routes) {
        for e in &self.elements {
            match e {
                Element::Beans(_) => {}
                Element::Route(r) => {
                    routes.add((**r).clone());
                }
                Element::ErrorHandler(eh) => {
                    routes.error_handler(eh.clone());
                }
                Element::OnException(c) => {
                    routes.on_exception((**c).clone());
                }
                Element::Intercept(on_when, steps) => {
                    let (on_when, steps) = (on_when.clone(), steps.clone());
                    routes.intercept(move |mut i| {
                        if let Some(w) = on_when {
                            i = i.on_when(w);
                        }
                        push_all_intercept(i, steps)
                    });
                }
                Element::InterceptFrom(uri, on_when, steps) => {
                    let (on_when, steps) = (on_when.clone(), steps.clone());
                    routes.intercept_from(uri.as_deref(), move |mut i| {
                        if let Some(w) = on_when {
                            i = i.on_when(w);
                        }
                        push_all_intercept(i, steps)
                    });
                }
                Element::InterceptSendToEndpoint {
                    uri,
                    on_when,
                    skip,
                    after_uri,
                    steps,
                } => {
                    let (on_when, skip, after_uri, steps) =
                        (on_when.clone(), *skip, after_uri.clone(), steps.clone());
                    routes.intercept_send_to_endpoint(uri, move |mut i| {
                        if let Some(w) = on_when {
                            i = i.on_when(w);
                        }
                        if skip {
                            i = i.skip_send_to_original_endpoint();
                        }
                        if let Some(a) = after_uri {
                            i = i.after_uri(&a);
                        }
                        push_all_intercept(i, steps)
                    });
                }
            }
        }
    }
}

fn push_all_intercept(mut i: InterceptDsl, steps: Vec<ProcessorDefinition>) -> InterceptDsl {
    for s in steps {
        i = i.push(s);
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_expressions() {
        assert_eq!("doWhile", camel_case("do-while"));
        let routes = YamlRoutes::parse(
            r#"
- from:
    uri: direct:start
    parameters:
      a: 1
    steps:
      - loop:
          constant: "3"
          do-while: true
      - to: "mock:result"
"#,
        )
        .unwrap();
        let defs = routes.route_definitions();
        assert_eq!("direct:start?a=1", defs[0].from);
        // the step after the loop is its nested step
        assert_eq!(
            "Loop[constant{3} -> [to[mock:result]]]",
            defs[0].steps[0].to_string()
        );
        assert!(YamlRoutes::parse("- from:\n    uri: x\n    steps:\n      - nope: {}\n").is_err());
        assert!(YamlRoutes::parse("- what: {}").is_err());
    }
}
