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

mod errorhandler;

pub use errorhandler::*;

use crate::errorhandler::{ErrorHandlerDefinition, OnExceptionDefinition};
use crate::expression::ExpressionDefinition;
use crate::intercept::{
    InterceptDefinition, InterceptFromDefinition, InterceptSendToEndpointDefinition,
};
use crate::processor::{
    AggregateDefinition, AggregationRef, CatchDefinition, Eip, MulticastOptions,
    OnCompletionDefinition, ProcessRef, ProcessorDefinition, RepositoryRef, SortComparator,
    WhenDefinition,
};
use crate::processor::{
    IdempotentConsumerDefinition, ResequenceDefinition, ResequenceMode, ThreadsDefinition,
};
use crate::route::RouteDefinition;
use camel_api::bean_binding::BeanReference;
use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::{CamelError, CamelResult, ErrorType};
use camel_api::exchange::Exchange;
use camel_api::processor::{processor_fn, SharedAggregationStrategy, SharedProcessor};
use camel_api::spi::aggregation_repository::AggregationRepository;
use std::sync::Arc;

pub use crate::processor::{IdempotentRepositoryRef, LoadBalancerDefinition};
pub use camel_support::builder::{
    and, body, constant, exchange_pattern, exchange_property, header, not, or, variable,
    ValueBuilder,
};

pub use camel_support::tokenizer::TokenizeExpression;

/// Splits the body by the token (Java `tokenize(",")`); see [`TokenizeExpression`] for the
/// options (`group`, `skip_first`, `source`, ...).
pub fn tokenize(token: &str) -> TokenizeExpression {
    TokenizeExpression::token(token)
}

/// Splits the body by the regular expression (Java `tokenize(regex, true)`).
pub fn tokenize_regex(regex: &str) -> TokenizeExpression {
    TokenizeExpression::regex(regex)
}

/// The texts of the body between the start and end tokens (Java `tokenizePair`).
pub fn tokenize_pair(start: &str, end: &str, include_tokens: bool) -> TokenizeExpression {
    TokenizeExpression::pair(start, end, include_tokens)
}

/// The XML elements of the tag in the body (Java `tokenizeXML(tag)` and
/// `tokenizeXML(tag, inheritNamespaceTag)`).
pub fn tokenize_xml(tag: &str, inherit_namespace_tag: Option<&str>) -> TokenizeExpression {
    TokenizeExpression::xml(tag, inherit_namespace_tag)
}

/// An expression in the simple language (Java `simple("${body}")`).
pub fn simple(expression: &str) -> ExpressionDefinition {
    ExpressionDefinition::language("simple", expression)
}

/// An expression in the language (Java `language("groovy", "...")`).
pub fn language(language: &str, expression: &str) -> ExpressionDefinition {
    ExpressionDefinition::language(language, expression)
}

/// An expression calling the method of the bean in the registry (Java `method(ref,
/// method)`), with the `bean` language.
pub fn method(bean: &str, method: &str) -> ExpressionDefinition {
    let bean = strip_bean_prefix(bean);
    ExpressionDefinition::language("bean", &format!("{bean}::{method}"))
}

/// The bean name without its `ref:` or `bean:` prefix (Java `MethodCallExpression`).
fn strip_bean_prefix(bean: &str) -> &str {
    bean.strip_prefix("ref:")
        .or_else(|| bean.strip_prefix("bean:"))
        .unwrap_or(bean)
}

/// An expression calling the only method of the bean in the registry (Java `method(ref)`),
/// with the `bean` language; the name may have a `ref:` or `bean:` prefix.
pub fn method_ref(bean: &str) -> ExpressionDefinition {
    // the query form keeps a bean name with dots whole
    let bean = strip_bean_prefix(bean);
    ExpressionDefinition::language("bean", &format!("{bean}?scope=Singleton"))
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

    /// Sets whether the last step is disabled (Java `.disabled(boolean)`).
    fn set_disabled(mut self, disabled: bool) -> Self {
        if let Some(s) = self.last_mut() {
            s.disabled = disabled;
        }
        self
    }

    /// Sends to the endpoint.
    fn to(self, uri: &str) -> Self {
        self.eip(Eip::To {
            uri: uri.to_string(),
            pattern: None,
            variable_send: None,
            variable_receive: None,
        })
    }

    /// Sends to the endpoint the value of the variable `send` as body (if any), and stores the
    /// reply in the variable `receive` (its body, and its headers as `header:receive.name`)
    /// instead of the message (Java `.toV(uri, variableSend, variableReceive)`).
    fn to_v(self, uri: &str, send: Option<&str>, receive: Option<&str>) -> Self {
        self.eip(Eip::To {
            uri: uri.to_string(),
            pattern: None,
            variable_send: send.map(str::to_string),
            variable_receive: receive.map(str::to_string),
        })
    }

    /// Sends to the endpoint with the exchange pattern (Java `.to(ExchangePattern, uri)`).
    fn to_with_pattern(self, pattern: ExchangePattern, uri: &str) -> Self {
        self.eip(Eip::To {
            uri: uri.to_string(),
            pattern: Some(pattern),
            variable_send: None,
            variable_receive: None,
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

    /// The last filter stores whether its predicate matched in the exchange property (Java
    /// `.statusPropertyName(name)`).
    fn status_property_name(mut self, name: &str) -> Self {
        if let Some(Eip::Filter {
            status_property_name,
            ..
        }) = self.last_mut().map(|l| &mut l.eip)
        {
            *status_property_name = Some(name.to_string());
        }
        self
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
    ///     "Route[From[direct:start] -> [DoTry[[to[direct:risky], DoCatch[java.lang.IllegalArgumentException -> [to[mock:catch]]], DoFinally[[to[mock:finally]]]]]]]",
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

    /// Splits the message into parts (by the expression) processed by the steps (Java
    /// `.split(..)`); see [`aggregation_strategy`](Dsl::aggregation_strategy),
    /// [`stop_on_exception`](Dsl::stop_on_exception) and [`delimiter`](Dsl::delimiter).
    fn split<F>(self, expression: impl Into<ExpressionDefinition>, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Split {
            expression: expression.into(),
            steps,
            strategy: None,
            options: MulticastOptions::default(),
            delimiter: None,
        })
    }

    /// Sends a copy of the exchange to each step of the block (Java `.multicast().to(a).to(b)`).
    fn multicast<F>(self, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Multicast {
            steps,
            strategy: None,
            options: MulticastOptions::default(),
        })
    }

    /// Sends to the endpoint of the uri, a simple expression such as `mock:${header.name}`
    /// (Java `.toD(..)`).
    fn to_d(self, uri: &str) -> Self {
        self.eip(Eip::ToD {
            uri: uri.to_string(),
            pattern: None,
            ignore_invalid_endpoint: false,
        })
    }

    /// Sends a copy to each endpoint of the expression (Java `.recipientList(..)`); see
    /// [`aggregation_strategy`](Dsl::aggregation_strategy),
    /// [`stop_on_exception`](Dsl::stop_on_exception), [`delimiter`](Dsl::delimiter) and
    /// [`ignore_invalid_endpoints`](Dsl::ignore_invalid_endpoints).
    fn recipient_list(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::RecipientList {
            expression: expression.into(),
            delimiter: None,
            strategy: None,
            options: MulticastOptions::default(),
            ignore_invalid_endpoints: false,
        })
    }

    /// Sends a copy to the endpoint of the uri (a simple expression) in the background (Java
    /// `.wireTap(..)`).
    fn wire_tap(self, uri: &str) -> Self {
        self.eip(Eip::WireTap {
            uri: uri.to_string(),
            ignore_invalid_endpoint: false,
            on_prepare: None,
            variable_send: None,
        })
    }

    /// Sends a copy with the value of the variable as body to the endpoint in the background
    /// (Java `.wireTap(uri, variableSend)`).
    fn wire_tap_variable(self, uri: &str, variable: &str) -> Self {
        self.eip(Eip::WireTap {
            uri: uri.to_string(),
            ignore_invalid_endpoint: false,
            on_prepare: None,
            variable_send: Some(variable.to_string()),
        })
    }

    /// Sets the exchange pattern (Java `.setExchangePattern(..)`).
    fn set_exchange_pattern(self, pattern: ExchangePattern) -> Self {
        self.eip(Eip::SetExchangePattern(pattern))
    }

    /// Sets several headers, in order (Java `.setHeaders(..)`); an expression sees the
    /// headers set before it.
    fn set_headers<I, N, E>(self, headers: I) -> Self
    where
        I: IntoIterator<Item = (N, E)>,
        N: Into<String>,
        E: Into<ExpressionDefinition>,
    {
        let headers = headers
            .into_iter()
            .map(|(n, e)| (n.into(), e.into()))
            .collect();
        self.eip(Eip::SetHeaders(headers))
    }

    /// Sets several variables, in order (Java `.setVariables(..)`).
    fn set_variables<I, N, E>(self, variables: I) -> Self
    where
        I: IntoIterator<Item = (N, E)>,
        N: Into<String>,
        E: Into<ExpressionDefinition>,
    {
        let variables = variables
            .into_iter()
            .map(|(n, e)| (n.into(), e.into()))
            .collect();
        self.eip(Eip::SetVariables(variables))
    }

    /// Removes the exchange property (Java `.removeProperty(..)`).
    fn remove_property(self, name: &str) -> Self {
        self.eip(Eip::RemoveProperty(name.to_string()))
    }

    /// Removes the exchange properties matching the pattern (Java `.removeProperties(..)`).
    fn remove_properties(self, pattern: &str) -> Self {
        self.remove_properties_excluding(pattern, &[])
    }

    /// Removes the exchange properties matching the pattern, except those matching an exclude
    /// pattern.
    fn remove_properties_excluding(self, pattern: &str, exclude_patterns: &[&str]) -> Self {
        self.eip(Eip::RemoveProperties {
            pattern: pattern.to_string(),
            exclude_patterns: exclude_patterns.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Removes the variable, all of them with `*` (Java `.removeVariable(..)`).
    fn remove_variable(self, name: &str) -> Self {
        self.eip(Eip::RemoveVariable(name.to_string()))
    }

    /// Evaluates the expression for its side effects, keeping the message (Java
    /// `.script(..)`).
    fn script(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::Script(expression.into()))
    }

    /// Processes the steps the number of times (Java `.loop(count)`); see [`copy`](Self::copy).
    fn loop_count<F>(self, count: impl Into<ExpressionDefinition>, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Loop {
            count: Some(count.into()),
            while_predicate: None,
            copy: false,
            steps,
        })
    }

    /// Processes the steps while the predicate matches (Java `.loopDoWhile(..)`).
    fn loop_do_while<F>(self, predicate: impl Into<ExpressionDefinition>, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Loop {
            count: None,
            while_predicate: Some(predicate.into()),
            copy: false,
            steps,
        })
    }

    /// Each iteration of the last loop starts from a copy of the original exchange (Java
    /// `.loop(..).copy()`).
    fn copy(mut self) -> Self {
        if let Some(Eip::Loop { copy, .. }) = self.last_mut().map(|l| &mut l.eip) {
            *copy = true;
        }
        self
    }

    /// Sends to the endpoints of the slip (uris separated by `,`) one after the other (Java
    /// `.routingSlip(..)`); see [`uri_delimiter`](Self::uri_delimiter).
    fn routing_slip(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::RoutingSlip {
            expression: expression.into(),
            delimiter: None,
            ignore_invalid_endpoints: false,
        })
    }

    /// Sends to the endpoints the expression answers, evaluated again after each endpoint
    /// until it answers nothing (Java `.dynamicRouter(..)`).
    fn dynamic_router(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::DynamicRouter {
            expression: expression.into(),
            delimiter: None,
            ignore_invalid_endpoints: false,
        })
    }

    /// The delimiter of the uris of the last routing slip, dynamic router or recipient list
    /// (Java `uriDelimiter` / `delimiter`).
    fn uri_delimiter(mut self, delimiter: &str) -> Self {
        if let Some(
            Eip::RoutingSlip { delimiter: d, .. }
            | Eip::DynamicRouter { delimiter: d, .. }
            | Eip::RecipientList { delimiter: d, .. },
        ) = self.last_mut().map(|l| &mut l.eip)
        {
            *d = Some(delimiter.to_string());
        }
        self
    }

    /// Sorts the list of the body, by the natural order (Java `.sort(body())`).
    fn sort_body(self) -> Self {
        self.eip(Eip::Sort {
            expression: None,
            comparator: None,
        })
    }

    /// Sorts the list of the expression, as the body (Java `.sort(expression)`).
    fn sort(self, expression: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::Sort {
            expression: Some(expression.into()),
            comparator: None,
        })
    }

    /// Sorts the list of the expression with the comparator (Java `.sort(expression,
    /// comparator)`).
    fn sort_with(
        self,
        expression: impl Into<ExpressionDefinition>,
        comparator: impl Fn(&camel_util::value::Value, &camel_util::value::Value) -> std::cmp::Ordering
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.eip(Eip::Sort {
            expression: Some(expression.into()),
            comparator: Some(SortComparator(Arc::new(comparator))),
        })
    }

    /// The steps of the block as a step with the id (Java `.step(id)`).
    fn step<F>(self, id: &str, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Step {
            id: id.to_string(),
            steps,
        })
    }

    /// Sends a copy (`InOut`) of the exchange to the endpoint of the uri (a simple expression)
    /// and merges the reply, by default the reply replaces the message (Java `.enrich(..)`);
    /// see [`aggregation_strategy`](Self::aggregation_strategy) and
    /// [`aggregate_on_exception`](Self::aggregate_on_exception).
    fn enrich(self, uri: &str) -> Self {
        self.eip(Eip::Enrich {
            uri: ExpressionDefinition::language("simple", uri),
            strategy: None,
            aggregate_on_exception: false,
            ignore_invalid_endpoint: false,
            variable_send: None,
            variable_receive: None,
        })
    }

    /// Polls a message from the endpoint of the uri (a simple expression), waiting up to the
    /// timeout in millis (negative: without limit, `0`: not at all), and merges it (Java
    /// `.pollEnrich(uri, timeout)`).
    fn poll_enrich(self, uri: &str, timeout: i64) -> Self {
        self.eip(Eip::PollEnrich {
            uri: ExpressionDefinition::language("simple", uri),
            timeout,
            strategy: None,
            aggregate_on_exception: false,
            ignore_invalid_endpoint: false,
            variable_receive: None,
        })
    }

    /// Polls a message from the endpoint of the uri (a simple expression without the file
    /// functions) as the new message, waiting up to 20 seconds (Java `.poll(uri)`).
    fn poll(self, uri: &str) -> Self {
        self.poll_timeout(uri, 20000)
    }

    /// Polls a message from the endpoint of the uri as the new message, waiting up to the
    /// timeout in millis (negative: without limit, `0`: not at all); the body is empty when
    /// nothing was polled (Java `.poll(uri, timeout)`).
    fn poll_timeout(self, uri: &str, timeout: i64) -> Self {
        self.eip(Eip::Poll {
            uri: ExpressionDefinition::language("simple", uri),
            timeout,
        })
    }

    /// The last enrich sends the variable as the body instead of the message body (Java
    /// `variableSend`).
    fn variable_send(mut self, name: &str) -> Self {
        if let Some(Eip::Enrich { variable_send, .. }) = self.last_mut().map(|l| &mut l.eip) {
            *variable_send = Some(name.to_string());
        }
        self
    }

    /// The last enrich or pollEnrich stores the reply (its body, and its headers as the
    /// variables `header:name.key`) in the variable, keeping the message (Java
    /// `variableReceive`).
    fn variable_receive(mut self, name: &str) -> Self {
        if let Some(
            Eip::Enrich {
                variable_receive, ..
            }
            | Eip::PollEnrich {
                variable_receive, ..
            },
        ) = self.last_mut().map(|l| &mut l.eip)
        {
            *variable_receive = Some(name.to_string());
        }
        self
    }

    /// Whether the last enrich or pollEnrich merges a failed reply too (Java
    /// `aggregateOnException`).
    fn aggregate_on_exception(mut self, aggregate: bool) -> Self {
        if let Some(
            Eip::Enrich {
                aggregate_on_exception,
                ..
            }
            | Eip::PollEnrich {
                aggregate_on_exception,
                ..
            },
        ) = self.last_mut().map(|l| &mut l.eip)
        {
            *aggregate_on_exception = aggregate;
        }
        self
    }

    /// Continues the routing on a pool of tasks (Java `.threads()`): the following steps run
    /// on another tokio task, at most `maxPoolSize` exchanges at a time, the others waiting up
    /// to `maxQueueSize` before being rejected by the rejected policy.
    fn threads(self) -> Self {
        self.eip(Eip::Threads(ThreadsDefinition::default()))
    }

    /// `threads` with the pool size (Java `.threads(poolSize)`).
    fn threads_pool(self, pool_size: i32) -> Self {
        self.threads().pool_size(pool_size)
    }

    /// `threads` with the pool and max pool sizes (Java `.threads(poolSize, maxPoolSize)`).
    fn threads_pools(self, pool_size: i32, max_pool_size: i32) -> Self {
        self.threads()
            .pool_size(pool_size)
            .max_pool_size(max_pool_size)
    }

    /// Sets an option of the last `threads`.
    #[doc(hidden)]
    fn threads_option(mut self, f: impl FnOnce(&mut ThreadsDefinition)) -> Self {
        if let Some(Eip::Threads(t)) = self.last_mut().map(|l| &mut l.eip) {
            f(t);
        }
        self
    }

    /// The pool size of the last `threads`.
    fn pool_size(self, size: i32) -> Self {
        self.threads_option(|t| t.pool_size = Some(size))
    }

    /// The max pool size of the last `threads`.
    fn max_pool_size(self, size: i32) -> Self {
        self.threads_option(|t| t.max_pool_size = Some(size))
    }

    /// The max queue size of the last `threads`.
    fn max_queue_size(self, size: i32) -> Self {
        self.threads_option(|t| t.max_queue_size = Some(size))
    }

    /// The keep alive time (seconds) of the last `threads`.
    fn keep_alive_time(self, seconds: u64) -> Self {
        self.threads_option(|t| t.keep_alive_time = Some(seconds))
    }

    /// The pool name of the last `threads`.
    fn thread_name(self, name: &str) -> Self {
        self.threads_option(|t| t.thread_name = Some(name.to_string()))
    }

    /// The rejected policy of the last `threads`.
    fn rejected_policy(self, policy: camel_api::enums::ThreadPoolRejectedPolicy) -> Self {
        self.threads_option(|t| t.rejected_policy = Some(policy))
    }

    /// Whether the caller runs a rejected exchange of the last `threads`.
    fn caller_runs_when_rejected(self, caller_runs: bool) -> Self {
        self.threads_option(|t| t.caller_runs_when_rejected = Some(caller_runs))
    }

    /// Lets one exchange through per second, stopping the others (Java `.sample()`).
    fn sample(self) -> Self {
        self.sample_period(1000)
    }

    /// Lets one exchange through per period in millis (Java `.sample(Duration)`).
    fn sample_period(self, millis: u64) -> Self {
        self.eip(Eip::Sample {
            message_frequency: None,
            period_millis: millis,
        })
    }

    /// Lets one exchange through per number of messages (Java `.sample(long)`).
    fn sample_messages(self, message_frequency: u64) -> Self {
        self.eip(Eip::Sample {
            message_frequency: Some(message_frequency),
            period_millis: 1000,
        })
    }

    /// Limits the exchanges to the maximum number of requests per time period (default 1000
    /// millis), waiting for a permit (Java `.throttle(..)`); see
    /// [`time_period_millis`](Self::time_period_millis),
    /// [`concurrent_requests_mode`](Self::concurrent_requests_mode),
    /// [`correlation_expression`](Self::correlation_expression) and
    /// [`reject_execution`](Self::reject_execution).
    fn throttle(self, max_requests: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::Throttle {
            max_requests: max_requests.into(),
            time_period_millis: 1000,
            concurrent: false,
            correlation: None,
            reject_execution: false,
        })
    }

    /// The time period of the last throttle (total requests mode).
    fn time_period_millis(mut self, millis: u64) -> Self {
        if let Some(Eip::Throttle {
            time_period_millis, ..
        }) = self.last_mut().map(|l| &mut l.eip)
        {
            *time_period_millis = millis;
        }
        self
    }

    /// The last throttle limits the exchanges processed at the same time (Java
    /// `concurrentRequestsMode()`).
    fn concurrent_requests_mode(mut self) -> Self {
        if let Some(Eip::Throttle { concurrent, .. }) = self.last_mut().map(|l| &mut l.eip) {
            *concurrent = true;
        }
        self
    }

    /// The correlation key of separate limits of the last throttle.
    fn correlation_expression(mut self, expression: impl Into<ExpressionDefinition>) -> Self {
        if let Some(Eip::Throttle { correlation, .. }) = self.last_mut().map(|l| &mut l.eip) {
            *correlation = Some(expression.into());
        }
        self
    }

    /// Whether the exchanges over the limit of the last throttle fail with a
    /// `ThrottlerRejectedExecutionException` instead of waiting.
    fn reject_execution(mut self, reject: bool) -> Self {
        if let Some(Eip::Throttle {
            reject_execution, ..
        }) = self.last_mut().map(|l| &mut l.eip)
        {
            *reject_execution = reject;
        }
        self
    }

    /// The claim check EIP (Java `.claimCheck(operation, key, filter)`): the operation is
    /// `Set`, `Get`, `GetAndRemove`, `Push` or `Pop`; the key is a simple expression.
    fn claim_check(self, operation: &str, key: Option<&str>, filter: Option<&str>) -> Self {
        self.eip(Eip::ClaimCheck {
            operation: operation.to_string(),
            key: key.map(str::to_string),
            filter: filter.map(str::to_string),
            strategy: None,
        })
    }

    /// Fails the exchange with a `RollbackExchangeException` and marks it for rollback (Java
    /// `.rollback()`).
    fn rollback(self) -> Self {
        self.eip(Eip::Rollback {
            message: None,
            mark_rollback_only: false,
            mark_rollback_only_last: false,
        })
    }

    /// [`rollback`](Self::rollback) with the message of the exception.
    fn rollback_message(self, message: &str) -> Self {
        self.eip(Eip::Rollback {
            message: Some(message.to_string()),
            mark_rollback_only: false,
            mark_rollback_only_last: false,
        })
    }

    /// Only marks the exchange for rollback (Java `.markRollbackOnly()`).
    fn mark_rollback_only(self) -> Self {
        self.eip(Eip::Rollback {
            message: None,
            mark_rollback_only: true,
            mark_rollback_only_last: false,
        })
    }

    /// Only marks the last transaction of the exchange for rollback (Java
    /// `.markRollbackOnlyLast()`).
    fn mark_rollback_only_last(self) -> Self {
        self.eip(Eip::Rollback {
            message: None,
            mark_rollback_only: false,
            mark_rollback_only_last: true,
        })
    }

    /// Converts the header to the Java type (Java `.convertHeaderTo(name, type)`).
    fn convert_header_to(self, name: &str, type_name: &str) -> Self {
        self.eip(Eip::ConvertValueTo {
            variable: false,
            name: name.to_string(),
            to_name: None,
            type_name: type_name.to_string(),
            mandatory: true,
        })
    }

    /// Converts the variable to the Java type (Java `.convertVariableTo(name, type)`).
    fn convert_variable_to(self, name: &str, type_name: &str) -> Self {
        self.eip(Eip::ConvertValueTo {
            variable: true,
            name: name.to_string(),
            to_name: None,
            type_name: type_name.to_string(),
            mandatory: true,
        })
    }

    /// Sends the exchanges to the steps of the block in the order of the expression (Java
    /// `.resequence(expression)`), in batch mode by default: see [`stream`](Self::stream),
    /// [`timeout`](Self::timeout), [`batch_size`](Self::batch_size),
    /// [`capacity`](Self::capacity), [`allow_duplicates`](Self::allow_duplicates),
    /// [`reverse`](Self::reverse), [`reject_old`](Self::reject_old) and
    /// [`ignore_invalid_exchanges`](Self::ignore_invalid_exchanges).
    fn resequence<F>(self, expression: impl Into<ExpressionDefinition>, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Resequence(Box::new(ResequenceDefinition {
            expression: expression.into(),
            mode: ResequenceMode::Batch {
                size: 100,
                allow_duplicates: false,
                reverse: false,
            },
            timeout: 1000,
            ignore_invalid_exchanges: false,
            steps,
        })))
    }

    /// The last resequencer uses the stream mode (Java `.stream()`).
    fn stream(mut self) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            r.mode = ResequenceMode::Stream {
                capacity: 1000,
                reject_old: false,
            };
        }
        self
    }

    /// The batch size of the last resequencer (batch mode).
    fn batch_size(mut self, size: usize) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            if let ResequenceMode::Batch { size: s, .. } = &mut r.mode {
                // a size of 0 disables the limit, as Java
                *s = if size == 0 { usize::MAX } else { size };
            }
        }
        self
    }

    /// The capacity of the last resequencer (stream mode).
    fn capacity(mut self, capacity: usize) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            if let ResequenceMode::Stream { capacity: c, .. } = &mut r.mode {
                *c = capacity;
            }
        }
        self
    }

    /// The last resequencer keeps the exchanges with the same expression value (batch mode).
    fn allow_duplicates(mut self) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            if let ResequenceMode::Batch {
                allow_duplicates, ..
            } = &mut r.mode
            {
                *allow_duplicates = true;
            }
        }
        self
    }

    /// The last resequencer sends in the reverse order (batch mode).
    fn reverse(mut self) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            if let ResequenceMode::Batch { reverse, .. } = &mut r.mode {
                *reverse = true;
            }
        }
        self
    }

    /// The last resequencer fails the exchanges older than the last sent one (stream mode).
    fn reject_old(mut self) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            if let ResequenceMode::Stream { reject_old, .. } = &mut r.mode {
                *reject_old = true;
            }
        }
        self
    }

    /// The last resequencer ignores the exchanges without an expression value instead of
    /// failing them.
    fn ignore_invalid_exchanges(mut self) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            r.ignore_invalid_exchanges = true;
        }
        self
    }

    /// Fails the exchange with a `PredicateValidationException` when the predicate does not
    /// match (Java `.validate(..)`).
    fn validate(self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::Validate(predicate.into()))
    }

    /// Processes the steps once per message id, skipping the duplicates (Java
    /// `.idempotentConsumer(..)`); the repository is a bean name or an
    /// `Arc<dyn IdempotentRepository>`, see [`eager`](Self::eager),
    /// [`skip_duplicate`](Self::skip_duplicate), [`remove_on_failure`](Self::remove_on_failure),
    /// [`completion_eager`](Self::completion_eager).
    fn idempotent_consumer<F>(
        self,
        message_id: impl Into<ExpressionDefinition>,
        repository: impl Into<IdempotentRepositoryRef>,
        steps: F,
    ) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::IdempotentConsumer(Box::new(
            IdempotentConsumerDefinition {
                message_id: message_id.into(),
                repository: repository.into(),
                eager: true,
                completion_eager: false,
                skip_duplicate: true,
                remove_on_failure: true,
                steps,
            },
        )))
    }

    /// Whether the last idempotent consumer adds the id before processing (default `true`).
    fn eager(mut self, eager: bool) -> Self {
        if let Some(Eip::IdempotentConsumer(i)) = self.last_mut().map(|l| &mut l.eip) {
            i.eager = eager;
        }
        self
    }

    /// Whether the last idempotent consumer skips the duplicates (default `true`), else routes
    /// them with the `CamelDuplicateMessage` property.
    fn skip_duplicate(mut self, skip: bool) -> Self {
        if let Some(Eip::IdempotentConsumer(i)) = self.last_mut().map(|l| &mut l.eip) {
            i.skip_duplicate = skip;
        }
        self
    }

    /// Whether the last idempotent consumer removes the id when the exchange fails (default
    /// `true`).
    fn remove_on_failure(mut self, remove: bool) -> Self {
        if let Some(Eip::IdempotentConsumer(i)) = self.last_mut().map(|l| &mut l.eip) {
            i.remove_on_failure = remove;
        }
        self
    }

    /// Whether the last idempotent consumer completes when its block ends (default `false`).
    fn completion_eager(mut self, eager: bool) -> Self {
        if let Some(Eip::IdempotentConsumer(i)) = self.last_mut().map(|l| &mut l.eip) {
            i.completion_eager = eager;
        }
        self
    }

    /// Sends each message to one of the steps of the block, chosen by the balancer (Java
    /// `.loadBalance()`); see [`LoadBalancerDefinition`].
    fn load_balance<F>(self, balancer: LoadBalancerDefinition, steps: F) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::LoadBalance { balancer, steps })
    }

    /// Calls the only method of the bean (Java `.bean(..)`): the name of a bean in the
    /// registry, or the bean itself (such as a `camel_bean::MethodBean`); the result becomes
    /// the message body.
    fn bean(self, bean: impl Into<BeanReference>) -> Self {
        self.eip(Eip::Bean {
            bean: bean.into(),
            method: None,
        })
    }

    /// Calls the method of the bean (Java `.bean(bean, method)`); the method may have
    /// parameters, such as `hello('World', ${header.name})`.
    fn bean_method(self, bean: impl Into<BeanReference>, method: &str) -> Self {
        self.eip(Eip::Bean {
            bean: bean.into(),
            method: Some(method.to_string()),
        })
    }

    /// Converts the body to the Java type, such as `String` or `Integer` (Java
    /// `.convertBodyTo(..)`).
    fn convert_body_to(self, type_name: &str) -> Self {
        self.eip(Eip::ConvertBodyTo {
            type_name: type_name.to_string(),
            mandatory: true,
            charset: None,
        })
    }

    /// Converts the body to the Java type using the charset between text and bytes, such as
    /// `byte[]` in `iso-8859-1` (Java `.convertBodyTo(type, charset)`).
    fn convert_body_to_charset(self, type_name: &str, charset: &str) -> Self {
        self.eip(Eip::ConvertBodyTo {
            type_name: type_name.to_string(),
            mandatory: true,
            charset: Some(charset.to_string()),
        })
    }

    /// Converts the body to the Java type, a body that cannot be converted becomes `Null`
    /// (Java `.convertBodyTo(type, false)`).
    fn convert_body_to_optional(self, type_name: &str) -> Self {
        self.eip(Eip::ConvertBodyTo {
            type_name: type_name.to_string(),
            mandatory: false,
            charset: None,
        })
    }

    /// Invalid endpoint uris of the last step (toD, recipient list or wire tap) are ignored.
    fn ignore_invalid_endpoints(mut self) -> Self {
        match self.last_mut().map(|l| &mut l.eip) {
            Some(Eip::ToD {
                ignore_invalid_endpoint,
                ..
            }) => *ignore_invalid_endpoint = true,
            Some(Eip::RecipientList {
                ignore_invalid_endpoints,
                ..
            }) => *ignore_invalid_endpoints = true,
            Some(Eip::WireTap {
                ignore_invalid_endpoint,
                ..
            }) => *ignore_invalid_endpoint = true,
            Some(
                Eip::Enrich {
                    ignore_invalid_endpoint,
                    ..
                }
                | Eip::PollEnrich {
                    ignore_invalid_endpoint,
                    ..
                },
            ) => *ignore_invalid_endpoint = true,
            Some(
                Eip::RoutingSlip {
                    ignore_invalid_endpoints,
                    ..
                }
                | Eip::DynamicRouter {
                    ignore_invalid_endpoints,
                    ..
                },
            ) => *ignore_invalid_endpoints = true,
            _ => {}
        }
        self
    }

    /// Aggregates the exchanges with the same correlation key with the strategy; the steps
    /// process the completed groups (Java `.aggregate(correlation, strategy)`). At least one
    /// completion option is required, such as [`completion_size`](Dsl::completion_size) or
    /// [`completion_timeout`](Dsl::completion_timeout).
    fn aggregate<F>(
        self,
        correlation: impl Into<ExpressionDefinition>,
        strategy: SharedAggregationStrategy,
        steps: F,
    ) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Aggregate(Box::new(AggregateDefinition::new(
            correlation.into(),
            Some(AggregationRef::Strategy(strategy)),
            steps,
        ))))
    }

    /// As [`aggregate`](Dsl::aggregate), with the strategy bean of the registry
    /// (Java `aggregationStrategy = "#bean:name"`).
    fn aggregate_ref<F>(
        self,
        correlation: impl Into<ExpressionDefinition>,
        strategy: &str,
        steps: F,
    ) -> Self
    where
        F: FnOnce(StepsDsl) -> StepsDsl,
    {
        let steps = steps(StepsDsl::default()).steps;
        self.eip(Eip::Aggregate(Box::new(AggregateDefinition::new(
            correlation.into(),
            Some(AggregationRef::Ref(strategy.to_string())),
            steps,
        ))))
    }

    /// Completes a group of the last aggregate after that many exchanges (Java `.completionSize(n)`).
    fn completion_size(mut self, completion_size: i32) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_size = Some(completion_size);
        }
        self
    }

    /// Completes a group of the last aggregate after the number of exchanges of the expression, per group (Java `.completionSize(expression)`).
    fn completion_size_expression(mut self, size: impl Into<ExpressionDefinition>) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_size_expression = Some(size.into());
        }
        self
    }

    /// Completes a group of the last aggregate after it was inactive for the millis (Java `.completionTimeout(ms)`).
    fn completion_timeout(mut self, millis: i64) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_timeout = Some(millis);
        }
        self
    }

    /// Completes a group of the last aggregate after it was inactive for the millis of the expression (Java `.completionTimeout(expression)`).
    fn completion_timeout_expression(mut self, millis: impl Into<ExpressionDefinition>) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_timeout_expression = Some(millis.into());
        }
        self
    }

    /// How often the last aggregate checks the timeouts, in millis (default 1000).
    fn completion_timeout_checker_interval(mut self, millis: u64) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_timeout_checker_interval = Some(millis);
        }
        self
    }

    /// Completes all groups of the last aggregate every interval millis (Java `.completionInterval(ms)`).
    fn completion_interval(mut self, millis: i64) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_interval = Some(millis);
        }
        self
    }

    /// Completes a group of the last aggregate when its aggregated exchange matches (Java `.completionPredicate(..)`).
    fn completion_predicate(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_predicate = Some(predicate.into());
        }
        self
    }

    /// The last aggregate checks the completion on the incoming exchange, before aggregating it.
    fn eager_check_completion(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.eager_check_completion = true;
        }
        self
    }

    /// The last aggregate ignores exchanges without correlation key (instead of failing them).
    fn ignore_invalid_correlation_keys(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.ignore_invalid_correlation_keys = true;
        }
        self
    }

    /// The last aggregate closes the correlation keys of completed groups, remembering at most `capacity` keys (0 for no limit); later exchanges of a closed key fail with a `ClosedCorrelationKeyException`.
    fn close_correlation_key_on_completion(mut self, capacity: usize) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.close_correlation_key_on_completion = Some(capacity);
        }
        self
    }

    /// The last aggregate discards the groups completed by timeout.
    fn discard_on_completion_timeout(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.discard_on_completion_timeout = true;
        }
        self
    }

    /// The last aggregate discards a group when its aggregation strategy fails.
    fn discard_on_aggregation_failure(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.discard_on_aggregation_failure = true;
        }
        self
    }

    /// The last aggregate completes all its groups when the route stops (Java `.forceCompletionOnStop()`).
    fn force_completion_on_stop(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.force_completion_on_stop = true;
        }
        self
    }

    /// The last aggregate completes all its groups when the route stops (Java `.completeAllOnStop()`).
    fn complete_all_on_stop(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.complete_all_on_stop = true;
        }
        self
    }

    /// The repository of the groups of the last aggregate (default in memory; Java
    /// `.aggregationRepository(..)`).
    fn aggregation_repository(mut self, repository: Arc<dyn AggregationRepository>) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.repository = Some(RepositoryRef(repository));
        }
        self
    }

    /// The last aggregate completes all groups when a new correlation group starts.
    fn completion_on_new_correlation_group(mut self) -> Self {
        if let Some(Eip::Aggregate(a)) = self.last_mut().map(|l| &mut l.eip) {
            a.completion_on_new_correlation_group = true;
        }
        self
    }

    /// Routes a copy of the exchange through the steps when the exchange is done (Java
    /// `.onCompletion()` in a route; it applies to the whole route wherever it is). The block
    /// sets the options, such as [`OnCompletionDsl::on_failure_only`], and the steps.
    fn on_completion<F>(self, block: F) -> Self
    where
        F: FnOnce(OnCompletionDsl) -> OnCompletionDsl,
    {
        let def = block(OnCompletionDsl::default()).def;
        self.eip(Eip::OnCompletion(Box::new(def)))
    }

    /// Delays by the millis of the expression (Java `.delay(..)`).
    fn delay(self, millis: impl Into<ExpressionDefinition>) -> Self {
        self.eip(Eip::Delay(millis.into()))
    }

    /// Sets the aggregation strategy of the last step (split or multicast).
    fn aggregation_strategy(mut self, strategy: SharedAggregationStrategy) -> Self {
        if let Some(
            Eip::Split { strategy: s, .. }
            | Eip::Multicast { strategy: s, .. }
            | Eip::RecipientList { strategy: s, .. }
            | Eip::Enrich { strategy: s, .. }
            | Eip::PollEnrich { strategy: s, .. }
            | Eip::ClaimCheck { strategy: s, .. },
        ) = self.last_mut().map(|l| &mut l.eip)
        {
            *s = Some(AggregationRef::Strategy(strategy));
        }
        self
    }

    /// Sets the aggregation strategy bean (by name) of the last step (split or multicast).
    fn aggregation_strategy_ref(mut self, name: &str) -> Self {
        if let Some(
            Eip::Split { strategy: s, .. }
            | Eip::Multicast { strategy: s, .. }
            | Eip::RecipientList { strategy: s, .. }
            | Eip::Enrich { strategy: s, .. }
            | Eip::PollEnrich { strategy: s, .. }
            | Eip::ClaimCheck { strategy: s, .. },
        ) = self.last_mut().map(|l| &mut l.eip)
        {
            *s = Some(AggregationRef::Ref(name.to_string()));
        }
        self
    }

    /// The options of the last step, if it is a split, multicast or recipient list.
    #[doc(hidden)]
    fn last_multicast_options(&mut self) -> Option<&mut MulticastOptions> {
        match self.last_mut().map(|l| &mut l.eip) {
            Some(
                Eip::Split { options, .. }
                | Eip::Multicast { options, .. }
                | Eip::RecipientList { options, .. },
            ) => Some(options),
            _ => None,
        }
    }

    /// The last step (split, multicast, recipient list) prepares each copy with the processor
    /// before it is sent (Java `.onPrepare(processor)`); a failure fails the copy.
    fn on_prepare(self, processor: SharedProcessor) -> Self {
        self.set_on_prepare(ProcessRef::Processor(processor))
    }

    /// Sets the onPrepare of the last split, multicast, recipient list or wire tap.
    #[doc(hidden)]
    fn set_on_prepare(mut self, prepare: ProcessRef) -> Self {
        if let Some(Eip::WireTap { on_prepare, .. }) = self.last_mut().map(|l| &mut l.eip) {
            *on_prepare = Some(prepare);
            return self;
        }
        if let Some(o) = self.last_multicast_options() {
            o.on_prepare = Some(prepare);
        }
        self
    }

    /// The last step (split, multicast, recipient list) prepares each copy with the processor
    /// bean of the registry (Java `.onPrepare("#name")`).
    fn on_prepare_ref(self, name: &str) -> Self {
        self.set_on_prepare(ProcessRef::Ref(name.to_string()))
    }

    /// The last step (split, multicast or recipient list) stops on the first failed copy
    /// (Java `.stopOnException()`).
    fn stop_on_exception(mut self) -> Self {
        if let Some(o) = self.last_multicast_options() {
            o.stop_on_exception = true;
        }
        self
    }

    /// The last step (split, multicast or recipient list) processes its copies concurrently
    /// (Java `.parallelProcessing()`).
    fn parallel_processing(mut self) -> Self {
        if let Some(o) = self.last_multicast_options() {
            o.parallel_processing = true;
        }
        self
    }

    /// The last step (split, multicast or recipient list) aggregates the results in the order
    /// they complete instead of the order of the copies (Java `.streaming()`).
    fn streaming(mut self) -> Self {
        if let Some(o) = self.last_multicast_options() {
            o.streaming = true;
        }
        self
    }

    /// The aggregation strategy of the last step (split, multicast or recipient list) may be
    /// called concurrently (Java `.parallelAggregate()`).
    fn parallel_aggregate(mut self) -> Self {
        if let Some(o) = self.last_multicast_options() {
            o.parallel_aggregate = true;
        }
        self
    }

    /// The total timeout in millis of the parallel processing of the last step (split,
    /// multicast or recipient list; Java `.timeout(millis)`): the copies not done in time are
    /// given to the `timeout` method of the aggregation strategy, and the exchange continues
    /// with what was aggregated.
    fn timeout(mut self, millis: u64) -> Self {
        if let Some(Eip::Resequence(r)) = self.last_mut().map(|l| &mut l.eip) {
            // the batch timeout or the time to wait for a missing exchange of a resequencer
            r.timeout = millis;
            return self;
        }
        if let Some(o) = self.last_multicast_options() {
            o.timeout = Some(millis);
        }
        self
    }

    /// The delimiter of the texts split by the last step (a split).
    fn delimiter(mut self, delimiter: &str) -> Self {
        if let Some(Eip::Split { delimiter: d, .. } | Eip::RecipientList { delimiter: d, .. }) =
            self.last_mut().map(|l| &mut l.eip)
        {
            *d = Some(delimiter.to_string());
        }
        self
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
        self.log_level(LoggingLevel::INFO, message)
    }

    /// Logs the message at the level (Java `.log(LoggingLevel, message)`).
    fn log_level(self, level: LoggingLevel, message: &str) -> Self {
        self.eip(Eip::Log {
            message: message.to_string(),
            level,
            log_name: None,
            log_language: None,
        })
    }

    /// Logs the message at the level with the logger name.
    fn log_with(self, level: LoggingLevel, log_name: &str, message: &str) -> Self {
        self.eip(Eip::Log {
            message: message.to_string(),
            level,
            log_name: Some(log_name.to_string()),
            log_language: None,
        })
    }

    /// Logs the message, an expression of the language, at `INFO` (Java
    /// `.log(logLanguage, message)`).
    fn log_language(self, language: &str, message: &str) -> Self {
        self.eip(Eip::Log {
            message: message.to_string(),
            level: LoggingLevel::INFO,
            log_name: None,
            log_language: Some(language.to_string()),
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

    /// Fails the exchange with an error of the kind, its message a simple expression evaluated
    /// with the exchange (Java `.throwException(type, message)`; `None`: without message).
    fn throw_exception_of(self, kind: camel_api::error::ErrorKind, message: Option<&str>) -> Self {
        self.eip(Eip::ThrowExceptionOf {
            kind,
            message: message.map(str::to_string),
        })
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

    /// Adds a policy to the route (Java `.routePolicy(policy)`).
    pub fn route_policy(mut self, policy: camel_api::spi::route_policy::SharedRoutePolicy) -> Self {
        self.def
            .route_policies
            .push(crate::route::RoutePolicyRef::Policy(policy));
        self
    }

    /// Adds the policy beans of the registry to the route, by their names separated by commas
    /// (Java `.routePolicyRef("a, b")`).
    pub fn route_policy_ref(mut self, names: &str) -> Self {
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            self.def
                .route_policies
                .push(crate::route::RoutePolicyRef::Ref(name.to_string()));
        }
        self
    }

    /// Sets the error handler of the route (Java `.errorHandler(..)` on the route).
    pub fn error_handler(mut self, error_handler: impl Into<ErrorHandlerDefinition>) -> Self {
        self.def.error_handler = Some(error_handler.into());
        self
    }

    /// Adds an `onException` clause scoped to the route.
    pub fn on_exception(mut self, clause: impl Into<OnExceptionDefinition>) -> Self {
        self.def.on_exceptions.push(clause.into());
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

/// The options and steps of an onCompletion (see [`Dsl::on_completion`] and
/// [`Routes::on_completion`]).
#[derive(Default)]
pub struct OnCompletionDsl {
    def: OnCompletionDefinition,
}

impl OnCompletionDsl {
    /// Only when the exchange completed successfully (Java `.onCompleteOnly()`).
    pub fn on_complete_only(mut self) -> Self {
        self.def.on_complete_only = true;
        self
    }

    /// Only when the exchange failed (Java `.onFailureOnly()`).
    pub fn on_failure_only(mut self) -> Self {
        self.def.on_failure_only = true;
        self
    }

    /// Only when the exchange matches (Java `.onWhen(..)`).
    pub fn on_when(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.def.on_when = Some(predicate.into());
        self
    }

    /// Routes the original in message (Java `.useOriginalMessage()`).
    pub fn use_original_message(mut self) -> Self {
        self.def.use_original_message = true;
        self
    }

    /// Runs when the route is done, before its consumer continues, for example before an
    /// HTTP reply is sent (Java `.modeBeforeConsumer()`).
    pub fn mode_before_consumer(mut self) -> Self {
        self.def.before_consumer = true;
        self
    }

    /// Runs when the unit of work is done, after the consumer (the default; Java
    /// `.modeAfterConsumer()`).
    pub fn mode_after_consumer(mut self) -> Self {
        self.def.before_consumer = false;
        self
    }

    /// Routes the copy in the background (Java `.parallelProcessing()`).
    pub fn parallel_processing(mut self) -> Self {
        self.def.parallel_processing = true;
        self
    }

    /// The definition.
    pub fn build(self) -> OnCompletionDefinition {
        self.def
    }
}

impl Dsl for OnCompletionDsl {
    fn push(mut self, step: ProcessorDefinition) -> Self {
        self.def.steps.push(step);
        self
    }

    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition> {
        self.def.steps.last_mut()
    }
}

/// The steps and options of an interceptor (see [`Routes::intercept`],
/// [`Routes::intercept_from`] and [`Routes::intercept_send_to_endpoint`]).
#[derive(Default)]
pub struct InterceptDsl {
    steps: Vec<ProcessorDefinition>,
    on_when: Option<ExpressionDefinition>,
    skip: bool,
    after_uri: Option<String>,
}

impl InterceptDsl {
    /// Only intercepts the exchanges matching the predicate (Java `.when(..)` of the
    /// interceptors).
    pub fn on_when(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.on_when = Some(predicate.into());
        self
    }

    /// The intercepted exchange is not sent to the original endpoint (interceptSendToEndpoint
    /// only, Java `skipSendToOriginalEndpoint()`).
    pub fn skip_send_to_original_endpoint(mut self) -> Self {
        self.skip = true;
        self
    }

    /// The intercepted exchange is sent to the uri after the original endpoint
    /// (interceptSendToEndpoint only, Java `afterUri(..)`).
    pub fn after_uri(mut self, uri: &str) -> Self {
        self.after_uri = Some(uri.to_string());
        self
    }
}

impl Dsl for InterceptDsl {
    fn push(mut self, step: ProcessorDefinition) -> Self {
        self.steps.push(step);
        self
    }

    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition> {
        self.steps.last_mut()
    }
}

/// The routes collected by a [`RouteBuilder`], with the error handler, `onException` and
/// `onCompletion` clauses that apply to all of them.
#[derive(Default)]
pub struct Routes {
    definitions: Vec<RouteDefinition>,
    error_handler: Option<ErrorHandlerDefinition>,
    on_exceptions: Vec<OnExceptionDefinition>,
    on_completions: Vec<OnCompletionDefinition>,
    intercepts: Vec<InterceptDefinition>,
    intercept_froms: Vec<InterceptFromDefinition>,
    intercept_send_to_endpoints: Vec<InterceptSendToEndpointDefinition>,
}

impl Routes {
    /// Adds the route.
    pub fn add(&mut self, route: impl Into<RouteDefinition>) -> &mut Self {
        self.definitions.push(route.into());
        self
    }

    /// Sets the error handler of the routes of this builder (Java `errorHandler(..)` in
    /// `configure()`).
    pub fn error_handler(&mut self, error_handler: impl Into<ErrorHandlerDefinition>) -> &mut Self {
        self.error_handler = Some(error_handler.into());
        self
    }

    /// Adds an `onException` clause for the routes of this builder.
    pub fn on_exception(&mut self, clause: impl Into<OnExceptionDefinition>) -> &mut Self {
        self.on_exceptions.push(clause.into());
        self
    }

    /// Adds an `onCompletion` for the routes of this builder (Java `onCompletion()` in
    /// `configure()`); a route with its own `onCompletion` does not use these.
    pub fn on_completion<F>(&mut self, block: F) -> &mut Self
    where
        F: FnOnce(OnCompletionDsl) -> OnCompletionDsl,
    {
        self.on_completions
            .push(block(OnCompletionDsl::default()).def);
        self
    }

    /// Intercepts each step of the routes of this builder with the steps of the block (Java
    /// `intercept()`): a `stop()` in the block stops the routing; see [`InterceptDsl::on_when`].
    pub fn intercept<F>(&mut self, block: F) -> &mut Self
    where
        F: FnOnce(InterceptDsl) -> InterceptDsl,
    {
        let d = block(InterceptDsl::default());
        self.intercepts.push(InterceptDefinition {
            on_when: d.on_when,
            steps: d.steps,
        });
        self
    }

    /// Intercepts the exchanges the routes of this builder consume, from the endpoints matching
    /// the pattern (`None`: all, else an uri, a wildcard `*` or a regex), with the steps of the
    /// block (Java `interceptFrom(uri)`).
    pub fn intercept_from<F>(&mut self, uri: Option<&str>, block: F) -> &mut Self
    where
        F: FnOnce(InterceptDsl) -> InterceptDsl,
    {
        let d = block(InterceptDsl::default());
        self.intercept_froms.push(InterceptFromDefinition {
            uri: uri.map(str::to_string),
            on_when: d.on_when,
            steps: d.steps,
        });
        self
    }

    /// Intercepts the sending of the routes of this builder to the endpoints matching the
    /// pattern (an uri, a wildcard `*` or a regex) with the steps of the block (Java
    /// `interceptSendToEndpoint(uri)`); see [`InterceptDsl::skip_send_to_original_endpoint`]
    /// and [`InterceptDsl::after_uri`].
    pub fn intercept_send_to_endpoint<F>(&mut self, uri: &str, block: F) -> &mut Self
    where
        F: FnOnce(InterceptDsl) -> InterceptDsl,
    {
        let d = block(InterceptDsl::default());
        self.intercept_send_to_endpoints
            .push(InterceptSendToEndpointDefinition {
                uri: uri.to_string(),
                on_when: d.on_when,
                skip_send_to_original_endpoint: d.skip,
                after_uri: d.after_uri,
                steps: d.steps,
            });
        self
    }

    /// The route definitions.
    pub fn definitions(&self) -> &[RouteDefinition] {
        &self.definitions
    }

    /// Takes the route definitions, with the error handler and `onException` clauses of the
    /// builder applied.
    pub fn into_definitions(self) -> Vec<RouteDefinition> {
        let (error_handler, on_exceptions, on_completions) =
            (self.error_handler, self.on_exceptions, self.on_completions);
        let (intercepts, intercept_froms, intercept_send_to_endpoints) = (
            self.intercepts,
            self.intercept_froms,
            self.intercept_send_to_endpoints,
        );
        self.definitions
            .into_iter()
            .map(|mut d| {
                if d.error_handler.is_none() {
                    d.error_handler = error_handler.clone();
                }
                d.builder_on_exceptions = on_exceptions.clone();
                d.builder_on_completions = on_completions.clone();
                d.builder_intercepts = intercepts.clone();
                d.builder_intercept_froms = intercept_froms.clone();
                d.builder_intercept_send_to_endpoints = intercept_send_to_endpoints.clone();
                d
            })
            .collect()
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
        assert!(
            text.ends_with(
                "process[ref:bean], Stop, ThrowException[java.lang.IllegalArgumentException]]]"
            ),
            "{text}"
        );
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
