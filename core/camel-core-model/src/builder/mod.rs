//! The Rust route DSL (Java `org.apache.camel.builder.RouteBuilder` and the fluent methods of
//! `ProcessorDefinition`).
//!
//! A route starts with [`from`] and chains steps. The steps of a nested block (the steps of a
//! filter, the `when` and `otherwise` clauses of a choice) are given by a closure instead of
//! Java's `.end()`, so the compiler checks the nesting:
//!
//! | Java | Rust |
//! |---|---|
//! | `from("direct:a").to("mock:a")` | `from("direct:a").to("mock:a")` |
//! | `.filter(header("foo").isEqualTo("bar")).to("mock:b").end()` | `.filter(header("foo").is_equal_to("bar"), \|f\| f.to("mock:b"))` |
//! | `.choice().when(p).to("a").otherwise().to("b").end()` | `.choice(\|c\| c.when(p, \|w\| w.to("a")).otherwise(\|o\| o.to("b")))` |
//! | `.setHeader("foo", constant("bar"))` | `.set_header("foo", constant("bar"))` |
//! | `.log("Got ${body}")` | `.log("Got ${body}")` |
//! | `.process(exchange -> ...)` | `.process_fn(\|exchange\| { ...; Ok(()) })` |
//! | `.routeId("myRoute")` | `.route_id("myRoute")` |
//! | `.id("myNode")` (on the last step) | `.id("myNode")` |
//!
//! Routes are collected by a [`RouteBuilder`] (a closure or a type) and added to the context
//! with `add_routes` of the engine:
//!
//! ```
//! use camel_core_model::builder::*;
//!
//! struct MyRoutes;
//!
//! impl RouteBuilder for MyRoutes {
//!     fn configure(&self, routes: &mut Routes) {
//!         routes.add(from("direct:a").to("mock:a"));
//!         routes.add(from("direct:b").set_header("foo", constant(123)).to("direct:a"));
//!     }
//! }
//!
//! let defs = MyRoutes.build();
//! assert_eq!(2, defs.len());
//! assert_eq!("Route[From[direct:b] -> [SetHeader[foo, 123], to[direct:a]]]", defs[1].to_string());
//! ```

use crate::expression::ExpressionDefinition;
use crate::processor::{CatchDefinition, Eip, ProcessRef, ProcessorDefinition, WhenDefinition};
use crate::route::RouteDefinition;
use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::{CamelError, CamelResult, ErrorType};
use camel_api::exchange::Exchange;
use camel_api::processor::{processor_fn, SharedProcessor};
use std::sync::Arc;

pub use camel_support::builder::{
    and, body, constant, exchange_pattern, exchange_property, header, not, or, variable,
    ValueBuilder,
};

/// An expression in the simple language (Java `simple("${body}")`).
pub fn simple(expression: &str) -> ExpressionDefinition {
    ExpressionDefinition::language("simple", expression)
}

/// An expression in the language (Java `language("groovy", "...")`).
pub fn language(language: &str, expression: &str) -> ExpressionDefinition {
    ExpressionDefinition::language(language, expression)
}

/// Starts a route consuming from the endpoint.
pub fn from(uri: &str) -> RouteDsl {
    RouteDsl {
        def: RouteDefinition::new(uri),
    }
}

/// The steps of a route or a nested block. The methods add a step and return the builder.
pub trait Dsl: Sized {
    /// Adds the step.
    fn push(self, step: ProcessorDefinition) -> Self;

    /// The last step added, if any.
    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition>;

    /// Adds a step doing the EIP.
    fn eip(self, eip: Eip) -> Self {
        self.push(ProcessorDefinition::new(eip))
    }

    /// Sets the id of the last step (Java `.id(..)`).
    fn id(mut self, id: &str) -> Self {
        if let Some(s) = self.last_mut() {
            s.id = Some(id.to_string());
        }
        self
    }

    /// Sets the description of the last step.
    fn description(mut self, text: &str) -> Self {
        if let Some(s) = self.last_mut() {
            s.description = Some(text.to_string());
        }
        self
    }

    /// Disables the last step: it is left out of the route.
    fn disabled(mut self) -> Self {
        if let Some(s) = self.last_mut() {
            s.disabled = true;
        }
        self
    }

    /// Sends to the endpoint.
    fn to(self, uri: &str) -> Self {
        self.eip(Eip::To {
            uri: uri.to_string(),
            pattern: None,
        })
    }

    /// Sends to the endpoint with the exchange pattern (Java `.to(ExchangePattern, uri)`).
    fn to_with_pattern(self, pattern: ExchangePattern, uri: &str) -> Self {
        self.eip(Eip::To {
            uri: uri.to_string(),
            pattern: Some(pattern),
        })
    }

    /// Continues with the steps only for the exchanges matching the predicate.
    fn filter<F>(self, predicate: impl Into<ExpressionDefinition>, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Filter {
            predicate: predicate.into(),
            steps,
            status_property_name: None,
        })
    }

    /// The content based router: the first matching `when`, else the `otherwise`.
    fn choice<F>(self, clauses: F) -> Self
    where
        F: FnOnce(ChoiceDsl) -> ChoiceDsl,
    {
        let c = clauses(ChoiceDsl::default());
        self.eip(Eip::Choice {
            whens: c.whens,
            otherwise: c.otherwise,
        })
    }

    /// The steps in sequence, as one step.
    fn pipeline<F>(self, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Pipeline { steps })
    }

    /// `doTry`: the steps of the try block, with the `do_catch` and `do_finally` clauses
    /// added on the [`TryDsl`] (Java `.doTry()...doCatch(..)...doFinally()...end()`).
    ///
    /// ```
    /// use camel_api::error::ErrorKind;
    /// use camel_core_model::builder::*;
    ///
    /// let route = from("direct:start")
    ///     .do_try(|t| {
    ///         t.to("direct:risky")
    ///             .do_catch(ErrorKind::IllegalArgument, |c| c.to("mock:catch"))
    ///             .do_finally(|f| f.to("mock:finally"))
    ///     })
    ///     .build();
    /// assert_eq!(
    ///     "Route[From[direct:start] -> [DoTry[[to[direct:risky], DoCatch[org.apache.camel.IllegalArgumentException -> [to[mock:catch]]], DoFinally[[to[mock:finally]]]]]]]",
    ///     route.to_string()
    /// );
    /// ```
    fn do_try<F>(self, block: F) -> Self
    where
        F: FnOnce(TryDsl) -> TryDsl,
    {
        let t = block(TryDsl::default());
        self.eip(Eip::DoTry {
            steps: t.steps,
            catches: t.catches,
            finally: t.finally,
        })
    }

    /// Sets the message body.
    fn set_body(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::SetBody(expression.into()))
    }

    /// Transforms the message body.
    fn transform(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::Transform(expression.into()))
    }

    /// Sets a header.
    fn set_header(self, name: &str, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::SetHeader {
            name: name.to_string(),
            expression: expression.into(),
        })
    }

    /// Sets an exchange property.
    fn set_property(self, name: &str, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::SetProperty {
            name: name.to_string(),
            expression: expression.into(),
        })
    }

    /// Sets a variable.
    fn set_variable(self, name: &str, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::SetVariable {
            name: name.to_string(),
            expression: expression.into(),
        })
    }

    /// Removes a header.
    fn remove_header(self, name: &str) -> Self {
        self.eip(Eip::RemoveHeader(name.to_string()))
    }

    /// Removes the headers matching the pattern.
    fn remove_headers(self, pattern: &str) -> Self {
        self.eip(Eip::RemoveHeaders {
            pattern: pattern.to_string(),
            exclude_patterns: vec![],
        })
    }

    /// Removes the headers matching the pattern, except those matching an exclude pattern.
    fn remove_headers_excluding(self, pattern: &str, exclude_patterns: &[&str]) -> Self {
        self.eip(Eip::RemoveHeaders {
            pattern: pattern.to_string(),
            exclude_patterns: exclude_patterns.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Logs the message (in the simple language) at `INFO` with the route id as logger name.
    fn log(self, message: &str) -> Self {
        self.eip(Eip::Log {
            message: message.to_string(),
            level: LoggingLevel::INFO,
            log_name: None,
        })
    }

    /// Logs the message at the level with the logger name.
    fn log_with(self, level: LoggingLevel, log_name: &str, message: &str) -> Self {
        self.eip(Eip::Log {
            message: message.to_string(),
            level,
            log_name: Some(log_name.to_string()),
        })
    }

    /// Stops routing the exchange.
    fn stop(self) -> Self {
        self.eip(Eip::Stop)
    }

    /// Runs the processor.
    fn process(self, processor: SharedProcessor) -> Self {
        self.eip(Eip::Process(ProcessRef::Processor(processor)))
    }

    /// Runs the closure as a processor.
    fn process_fn<F>(self, f: F) -> Self
    where
        F: Fn(&mut Exchange) -> CamelResult<()> + Send + Sync + 'static,
    {
        self.process(Arc::new(processor_fn(f)))
    }

    /// Runs the processor bean with the name from the registry (Java `.process("ref")`).
    fn process_ref(self, name: &str) -> Self {
        self.eip(Eip::Process(ProcessRef::Ref(name.to_string())))
    }

    /// Fails the exchange with the error.
    fn throw_exception(self, error: CamelError) -> Self {
        self.eip(Eip::ThrowException(error))
    }
}

/// The steps of a nested block.
#[derive(Default)]
pub struct StepsDsl {
    steps: Vec<ProcessorDefinition>,
}

impl StepsDsl {
    /// The steps.
    pub fn into_steps(self) -> Vec<ProcessorDefinition> {
        self.steps
    }
}

impl Dsl for StepsDsl {
    fn push(mut self, step: ProcessorDefinition) -> Self {
        self.steps.push(step);
        self
    }

    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition> {
        self.steps.last_mut()
    }
}

/// The error types of a `doCatch`: one or more [`ErrorType`]s.
pub trait IntoErrorTypes {
    /// The error types.
    fn into_error_types(self) -> Vec<ErrorType>;
}

impl IntoErrorTypes for ErrorType {
    fn into_error_types(self) -> Vec<ErrorType> {
        vec![self]
    }
}

impl IntoErrorTypes for camel_api::error::ErrorKind {
    fn into_error_types(self) -> Vec<ErrorType> {
        vec![ErrorType::Kind(self)]
    }
}

impl IntoErrorTypes for &str {
    fn into_error_types(self) -> Vec<ErrorType> {
        vec![ErrorType::from_java_name(self)]
    }
}

impl<T: Into<ErrorType> + Clone, const N: usize> IntoErrorTypes for [T; N] {
    fn into_error_types(self) -> Vec<ErrorType> {
        self.into_iter().map(Into::into).collect()
    }
}

impl<T: Into<ErrorType>> IntoErrorTypes for Vec<T> {
    fn into_error_types(self) -> Vec<ErrorType> {
        self.into_iter().map(Into::into).collect()
    }
}

/// The try block of a `doTry`: its steps, then the catch and finally clauses.
#[derive(Default)]
pub struct TryDsl {
    steps: Vec<ProcessorDefinition>,
    catches: Vec<CatchDefinition>,
    finally: Option<Vec<ProcessorDefinition>>,
}

impl TryDsl {
    /// Adds a `doCatch` clause for the error types.
    pub fn do_catch<F>(mut self, exceptions: impl IntoErrorTypes, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let exceptions = exceptions.into_error_types();
        self.catches.push(CatchDefinition {
            exceptions,
            on_when: None,
            steps: steps(StepsDsl::default()).steps,
        });
        self
    }

    /// Adds a `doCatch` clause for the error types when the predicate also matches (Java
    /// `.doCatch(..).onWhen(..)`).
    pub fn do_catch_when<F>(
        mut self,
        exceptions: impl IntoErrorTypes,
        on_when: impl Into<ExpressionDefinition>,
        steps: F,
    ) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let exceptions = exceptions.into_error_types();
        self.catches.push(CatchDefinition {
            exceptions,
            on_when: Some(on_when.into()),
            steps: steps(StepsDsl::default()).steps,
        });
        self
    }

    /// Sets the `doFinally` clause.
    pub fn do_finally<F>(mut self, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        self.finally = Some(steps(StepsDsl::default()).steps);
        self
    }
}

impl Dsl for TryDsl {
    fn push(mut self, step: ProcessorDefinition) -> Self {
        self.steps.push(step);
        self
    }

    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition> {
        self.steps.last_mut()
    }
}

/// The clauses of a choice.
#[derive(Default)]
pub struct ChoiceDsl {
    whens: Vec<WhenDefinition>,
    otherwise: Option<Vec<ProcessorDefinition>>,
}

impl ChoiceDsl {
    /// Adds a `when` clause: the steps for the exchanges matching the predicate.
    pub fn when<F>(mut self, predicate: impl Into<ExpressionDefinition>, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        self.whens.push(WhenDefinition {
            predicate: predicate.into(),
            steps: steps(StepsDsl::default()).steps,
        });
        self
    }

    /// Sets the `otherwise` clause: the steps when no `when` matches.
    pub fn otherwise<F>(mut self, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        self.otherwise = Some(steps(StepsDsl::default()).steps);
        self
    }
}

/// A route being built (started with [`from`]).
pub struct RouteDsl {
    def: RouteDefinition,
}

impl RouteDsl {
    /// Sets the route id.
    pub fn route_id(mut self, id: &str) -> Self {
        self.def.id = Some(id.to_string());
        self
    }

    /// Sets the route group.
    pub fn route_group(mut self, group: &str) -> Self {
        self.def.group = Some(group.to_string());
        self
    }

    /// Sets the route description.
    pub fn route_description(mut self, description: &str) -> Self {
        self.def.description = Some(description.to_string());
        self
    }

    /// Sets whether the route starts with the context.
    pub fn auto_startup(mut self, auto_startup: bool) -> Self {
        self.def.auto_startup = auto_startup;
        self
    }

    /// Sets the startup order.
    pub fn startup_order(mut self, order: i32) -> Self {
        self.def.startup_order = Some(order);
        self
    }

    /// Sets a route property.
    pub fn route_property(mut self, key: &str, value: impl Into<camel_util::value::Value>) -> Self {
        self.def.properties.insert(key.to_string(), value.into());
        self
    }

    /// The route definition.
    pub fn build(self) -> RouteDefinition {
        self.def
    }
}

impl Dsl for RouteDsl {
    fn push(mut self, step: ProcessorDefinition) -> Self {
        self.def.steps.push(step);
        self
    }

    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition> {
        self.def.steps.last_mut()
    }
}

impl From<RouteDsl> for RouteDefinition {
    fn from(r: RouteDsl) -> Self {
        r.def
    }
}

/// The routes collected by a [`RouteBuilder`].
#[derive(Default)]
pub struct Routes {
    definitions: Vec<RouteDefinition>,
}

impl Routes {
    /// Adds the route.
    pub fn add(&mut self, route: impl Into<RouteDefinition>) -> &mut Self {
        self.definitions.push(route.into());
        self
    }

    /// The route definitions.
    pub fn definitions(&self) -> &[RouteDefinition] {
        &self.definitions
    }

    /// Takes the route definitions.
    pub fn into_definitions(self) -> Vec<RouteDefinition> {
        self.definitions
    }
}

/// Defines routes (Java `RouteBuilder.configure()`). Implemented by closures taking
/// `&mut Routes`.
pub trait RouteBuilder {
    /// Adds the routes.
    fn configure(&self, routes: &mut Routes);

    /// The route definitions.
    fn build(&self) -> Vec<RouteDefinition> {
        let mut routes = Routes::default();
        self.configure(&mut routes);
        routes.into_definitions()
    }
}

impl<F: Fn(&mut Routes)> RouteBuilder for F {
    fn configure(&self, routes: &mut Routes) {
        self(routes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_options_and_steps() {
        let def = from("direct:a")
            .route_id("r")
            .route_group("g")
            .route_description("d")
            .auto_startup(false)
            .startup_order(5)
            .route_property("k", "v")
            .to("mock:a")
            .id("toA")
            .description("send")
            .to_with_pattern(ExchangePattern::InOut, "mock:b")
            .log("Got ${body}")
            .log_with(LoggingLevel::DEBUG, "my.log", "x")
            .set_property("p", simple("${body}"))
            .set_variable("v", constant(1))
            .transform(body().append("!"))
            .remove_header("h")
            .remove_headers("Camel*")
            .remove_headers_excluding("x*", &["xy"])
            .pipeline(|p| p.to("mock:p"))
            .process_fn(|_| Ok(()))
            .process_ref("bean")
            .stop()
            .disabled()
            .throw_exception(CamelError::illegal_argument("Forced"))
            .build();
        assert_eq!(Some("r"), def.id.as_deref());
        assert_eq!(
            (Some("g"), Some("d"), false, Some(5)),
            (
                def.group.as_deref(),
                def.description.as_deref(),
                def.auto_startup,
                def.startup_order
            )
        );
        assert_eq!(Some("toA"), def.steps[0].id.as_deref());
        assert_eq!(Some("send"), def.steps[0].description.as_deref());
        assert!(def.steps[def.steps.len() - 2].disabled);
        let names: Vec<&str> = def.steps.iter().map(|s| s.short_name()).collect();
        assert_eq!(
            vec![
                "to",
                "to",
                "log",
                "log",
                "setProperty",
                "setVariable",
                "transform",
                "removeHeader",
                "removeHeaders",
                "removeHeaders",
                "pipeline",
                "process",
                "process",
                "stop",
                "throwException"
            ],
            names
        );
        assert_eq!(1, def.steps[10].outputs().len());
        let text = def.to_string();
        assert!(text.starts_with("Route(r)[From[direct:a] -> [to[mock:a], to[mock:b], Log[Got ${body}], Log[x], SetProperty[p, simple{${body}}], SetVariable[v, 1], Transform[append(body, !)], RemoveHeader[h], RemoveHeaders[Camel*], RemoveHeaders[x*], Pipeline[[to[mock:p]]], process[Processor@"), "{text}");
        assert!(text.ends_with("process[ref:bean], Stop, ThrowException[org.apache.camel.IllegalArgumentException]]]"), "{text}");
    }

    #[test]
    fn closures_build_routes() {
        let builder = |r: &mut Routes| {
            r.add(from("direct:a").filter(header("x").is_equal_to(1), |f| f.to("mock:x")));
            r.add(from("direct:b").choice(|c| c.when(language("constant", "true"), |w| w.stop())));
        };
        let defs = builder.build();
        assert_eq!(
            "Route[From[direct:a] -> [Filter[header(x) == 1 -> [to[mock:x]]]]]",
            defs[0].to_string()
        );
        assert_eq!(
            "Route[From[direct:b] -> [Choice[[When[constant{true} -> [Stop]]]]]]",
            defs[1].to_string()
        );
        assert_eq!(Some("constant"), language("constant", "x").language_name());
        let mut routes = Routes::default();
        routes.add(from("direct:c"));
        assert_eq!(1, routes.definitions().len());
        assert!(StepsDsl::default().into_steps().is_empty());
    }
}
