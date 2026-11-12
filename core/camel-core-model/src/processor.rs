//! The processor definitions of the model (Java `ProcessorDefinition` and its subclasses such
//! as `ToDefinition`, `FilterDefinition` and `ChoiceDefinition`).

use crate::expression::ExpressionDefinition;
use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::{CamelError, ErrorType};
use camel_api::processor::{SharedAggregationStrategy, SharedProcessor};
use camel_api::spi::aggregation_repository::AggregationRepository;
use camel_util::uri_support::sanitize_uri;
use camel_util::value::Value;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

/// A comparator of the sort EIP.
pub type SharedComparator = Arc<dyn Fn(&Value, &Value) -> Ordering + Send + Sync>;

/// The comparator of a sort step.
#[derive(Clone)]
pub struct SortComparator(pub SharedComparator);

impl fmt::Debug for SortComparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SortComparator")
    }
}

/// A step of a route: what it does ([`Eip`]) and its common options.
#[derive(Clone, Debug)]
pub struct ProcessorDefinition {
    /// The id of the node (generated, such as `to1`, when not set).
    pub id: Option<String>,
    /// The description of the node.
    pub description: Option<String>,
    /// Whether the node is disabled (then it is left out of the route).
    pub disabled: bool,
    /// What the node does.
    pub eip: Eip,
}

impl ProcessorDefinition {
    /// A node doing the EIP.
    pub fn new(eip: Eip) -> Self {
        ProcessorDefinition {
            id: None,
            description: None,
            disabled: false,
            eip,
        }
    }

    /// The short name of the node type, used as the prefix of generated ids (Java
    /// `getShortName()`).
    pub fn short_name(&self) -> &'static str {
        self.eip.short_name()
    }

    /// The nested steps of the node (for filter, pipeline, ...), if any.
    pub fn outputs(&self) -> Vec<&ProcessorDefinition> {
        match &self.eip {
            Eip::Filter { steps, .. }
            | Eip::Pipeline { steps }
            | Eip::Loop { steps, .. }
            | Eip::Step { steps, .. }
            | Eip::Split { steps, .. }
            | Eip::Multicast { steps, .. } => steps.iter().collect(),
            Eip::Aggregate(a) => a.steps.iter().collect(),
            Eip::OnCompletion(c) => c.steps.iter().collect(),
            Eip::DoTry {
                steps,
                catches,
                finally,
            } => {
                let mut v: Vec<&ProcessorDefinition> = steps.iter().collect();
                v.extend(catches.iter().flat_map(|c| c.steps.iter()));
                if let Some(f) = finally {
                    v.extend(f.iter());
                }
                v
            }
            Eip::Choice { whens, otherwise } => {
                let mut v: Vec<&ProcessorDefinition> =
                    whens.iter().flat_map(|w| w.steps.iter()).collect();
                if let Some(o) = otherwise {
                    v.extend(o.iter());
                }
                v
            }
            _ => vec![],
        }
    }
}

/// How a `process` step gets its processor.
#[derive(Clone)]
pub enum ProcessRef {
    /// A processor bean looked up in the registry by name.
    Ref(String),
    /// A processor given in Rust code.
    Processor(SharedProcessor),
}

impl fmt::Debug for ProcessRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessRef::Ref(r) => write!(f, "Ref({r})"),
            ProcessRef::Processor(_) => f.write_str("Processor"),
        }
    }
}

/// How an EIP gets its aggregation strategy.
#[derive(Clone)]
pub enum AggregationRef {
    /// A strategy bean looked up in the registry by name.
    Ref(String),
    /// A strategy given in Rust code.
    Strategy(SharedAggregationStrategy),
}

impl fmt::Debug for AggregationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AggregationRef::Ref(r) => write!(f, "Ref({r})"),
            AggregationRef::Strategy(_) => f.write_str("Strategy"),
        }
    }
}

/// An aggregation repository given in Rust code.
#[derive(Clone)]
pub struct RepositoryRef(pub Arc<dyn AggregationRepository>);

impl fmt::Debug for RepositoryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AggregationRepository")
    }
}

/// The aggregate EIP (Java `AggregateDefinition`); the options left `None` use the defaults
/// of the aggregator.
#[derive(Debug, Clone)]
pub struct AggregateDefinition {
    /// The correlation key of the exchanges.
    pub correlation: ExpressionDefinition,
    /// The aggregation strategy (required).
    pub strategy: Option<AggregationRef>,
    /// The steps of the completed exchanges.
    pub steps: Vec<ProcessorDefinition>,
    /// The number of exchanges of a complete group.
    pub completion_size: Option<i32>,
    /// The number of exchanges of a complete group, per group.
    pub completion_size_expression: Option<ExpressionDefinition>,
    /// The inactivity timeout of a group in millis.
    pub completion_timeout: Option<i64>,
    /// The inactivity timeout in millis, per group.
    pub completion_timeout_expression: Option<ExpressionDefinition>,
    /// How often the timeouts are checked in millis.
    pub completion_timeout_checker_interval: Option<u64>,
    /// Completes all groups every interval millis.
    pub completion_interval: Option<i64>,
    /// Completes a group when it matches (a predicate).
    pub completion_predicate: Option<ExpressionDefinition>,
    /// Checks the completion on the incoming exchange.
    pub eager_check_completion: bool,
    /// Ignores exchanges without correlation key.
    pub ignore_invalid_correlation_keys: bool,
    /// Closes the keys of completed groups (the number of keys remembered, 0 for no limit).
    pub close_correlation_key_on_completion: Option<usize>,
    /// Discards groups completed by timeout.
    pub discard_on_completion_timeout: bool,
    /// Discards the group when the strategy fails.
    pub discard_on_aggregation_failure: bool,
    /// Completes all groups when a new group starts.
    pub completion_on_new_correlation_group: bool,
    /// Completes all groups when the route stops.
    pub force_completion_on_stop: bool,
    /// Completes all groups when the route stops.
    pub complete_all_on_stop: bool,
    /// The repository of the groups (default in memory).
    pub repository: Option<RepositoryRef>,
}

impl AggregateDefinition {
    /// A definition with the correlation, strategy and steps, and the default options.
    pub fn new(
        correlation: ExpressionDefinition,
        strategy: Option<AggregationRef>,
        steps: Vec<ProcessorDefinition>,
    ) -> Self {
        AggregateDefinition {
            correlation,
            strategy,
            steps,
            completion_size: None,
            completion_size_expression: None,
            completion_timeout: None,
            completion_timeout_expression: None,
            completion_timeout_checker_interval: None,
            completion_interval: None,
            completion_predicate: None,
            eager_check_completion: false,
            ignore_invalid_correlation_keys: false,
            close_correlation_key_on_completion: None,
            discard_on_completion_timeout: false,
            discard_on_aggregation_failure: false,
            completion_on_new_correlation_group: false,
            force_completion_on_stop: false,
            complete_all_on_stop: false,
            repository: None,
        }
    }
}

/// The options of the processing of the copies of multicast, split and recipient list (Java
/// `MulticastDefinition`, `SplitDefinition` and `RecipientListDefinition` options).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MulticastOptions {
    /// Whether to stop on the first failed copy (`stopOnException`).
    pub stop_on_exception: bool,
    /// Whether the copies are processed concurrently (`parallelProcessing`).
    pub parallel_processing: bool,
    /// Whether the results are aggregated in the order they complete instead of the order
    /// of the copies (`streaming`).
    pub streaming: bool,
    /// Whether the aggregation strategy may be called concurrently (`parallelAggregate`; the
    /// Rust port always aggregates from one task, so this only documents the strategy).
    pub parallel_aggregate: bool,
    /// The total timeout in millis of the parallel processing (`timeout`).
    pub timeout: Option<u64>,
}

/// The onCompletion EIP (Java `OnCompletionDefinition`): steps routing a copy of the exchange
/// when it is done.
#[derive(Debug, Clone, Default)]
pub struct OnCompletionDefinition {
    /// The steps.
    pub steps: Vec<ProcessorDefinition>,
    /// Only when the exchange completed successfully.
    pub on_complete_only: bool,
    /// Only when the exchange failed.
    pub on_failure_only: bool,
    /// Only when the exchange matches (a predicate).
    pub on_when: Option<ExpressionDefinition>,
    /// Routes the original in message.
    pub use_original_message: bool,
    /// Runs when the route is done, before its consumer continues (Java mode
    /// `BeforeConsumer`; default `AfterConsumer`).
    pub before_consumer: bool,
    /// Routes the copy in the background.
    pub parallel_processing: bool,
}

/// A `when` clause of a choice.
#[derive(Clone, Debug)]
pub struct WhenDefinition {
    /// The predicate.
    pub predicate: ExpressionDefinition,
    /// The steps when the predicate matches.
    pub steps: Vec<ProcessorDefinition>,
}

/// A `doCatch` clause of a `doTry`.
#[derive(Clone, Debug)]
pub struct CatchDefinition {
    /// The types of the errors to catch.
    pub exceptions: Vec<ErrorType>,
    /// An extra predicate the exchange must match.
    pub on_when: Option<ExpressionDefinition>,
    /// The steps handling the error.
    pub steps: Vec<ProcessorDefinition>,
}

impl fmt::Display for CatchDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.exceptions.iter().map(|e| e.to_string()).collect();
        write!(f, "DoCatch[{} -> {}]", names.join(","), Steps(&self.steps))
    }
}

/// What a step does: the Enterprise Integration Patterns of the model.
#[derive(Clone, Debug)]
pub enum Eip {
    /// Sends to an endpoint (`to`).
    To {
        /// The endpoint uri.
        uri: String,
        /// The exchange pattern to send with.
        pattern: Option<ExchangePattern>,
    },
    /// Continues only with the exchanges matching the predicate (`filter`).
    Filter {
        /// The predicate.
        predicate: ExpressionDefinition,
        /// The steps for the matching exchanges.
        steps: Vec<ProcessorDefinition>,
        /// The name of an exchange property storing whether the predicate matched.
        status_property_name: Option<String>,
    },
    /// The content based router (`choice`).
    Choice {
        /// The `when` clauses, tried in order.
        whens: Vec<WhenDefinition>,
        /// The steps when no `when` matches.
        otherwise: Option<Vec<ProcessorDefinition>>,
    },
    /// Steps in sequence (`pipeline`).
    Pipeline {
        /// The steps.
        steps: Vec<ProcessorDefinition>,
    },
    /// Sets the message body (`setBody`).
    SetBody(ExpressionDefinition),
    /// Transforms the message body (`transform`).
    Transform(ExpressionDefinition),
    /// Sets a header (`setHeader`).
    SetHeader {
        /// The header name.
        name: String,
        /// The value.
        expression: ExpressionDefinition,
    },
    /// Sets an exchange property (`setProperty`).
    SetProperty {
        /// The property name.
        name: String,
        /// The value.
        expression: ExpressionDefinition,
    },
    /// Sets a variable (`setVariable`).
    SetVariable {
        /// The variable name.
        name: String,
        /// The value.
        expression: ExpressionDefinition,
    },
    /// Removes a header (`removeHeader`).
    RemoveHeader(String),
    /// Removes the headers matching a pattern (`removeHeaders`).
    RemoveHeaders {
        /// The pattern (name, wildcard or regular expression).
        pattern: String,
        /// The patterns of headers to keep.
        exclude_patterns: Vec<String>,
    },
    /// Logs a message (`log`), in the simple language.
    Log {
        /// The message.
        message: String,
        /// The logging level (default `INFO`).
        level: LoggingLevel,
        /// The logger name (default the route id).
        log_name: Option<String>,
    },
    /// Stops routing the exchange (`stop`).
    Stop,
    /// Runs a processor (`process`).
    Process(ProcessRef),
    /// Fails the exchange with an error (`throwException`).
    ThrowException(CamelError),
    /// Splits the message into parts processed by the steps (`split`).
    Split {
        /// The expression of the parts.
        expression: ExpressionDefinition,
        /// The steps of each part.
        steps: Vec<ProcessorDefinition>,
        /// The aggregation of the parts (default: the original exchange).
        strategy: Option<AggregationRef>,
        /// How the parts are processed.
        options: MulticastOptions,
        /// The delimiter of texts (default `,`).
        delimiter: Option<String>,
    },
    /// Sends a copy to each step (`multicast`).
    Multicast {
        /// The outputs (each step gets a copy).
        steps: Vec<ProcessorDefinition>,
        /// The aggregation of the replies (default: the last reply).
        strategy: Option<AggregationRef>,
        /// How the copies are processed.
        options: MulticastOptions,
    },
    /// Routes a copy of the exchange when it is done (`onCompletion`; only directly in a
    /// route, where it applies to that route wherever it is).
    OnCompletion(Box<OnCompletionDefinition>),
    /// Aggregates exchanges by correlation key (`aggregate`).
    Aggregate(Box<AggregateDefinition>),
    /// Delays the exchange by the millis of the expression (`delay`).
    Delay(ExpressionDefinition),
    /// Sends to the endpoint of a uri in the simple language (`toD`).
    ToD {
        /// The uri (a simple expression).
        uri: String,
        /// The exchange pattern to send with.
        pattern: Option<ExchangePattern>,
        /// Whether an invalid endpoint uri is ignored.
        ignore_invalid_endpoint: bool,
    },
    /// Sends a copy to each endpoint of the expression (`recipientList`).
    RecipientList {
        /// The endpoint uris.
        expression: ExpressionDefinition,
        /// The delimiter of the uris in a text (default `,`).
        delimiter: Option<String>,
        /// The aggregation of the replies (default: the last reply).
        strategy: Option<AggregationRef>,
        /// How the copies are processed.
        options: MulticastOptions,
        /// Whether invalid endpoint uris are ignored.
        ignore_invalid_endpoints: bool,
    },
    /// Sends a copy to the endpoint in the background (`wireTap`).
    WireTap {
        /// The uri (a simple expression).
        uri: String,
        /// Whether an invalid endpoint uri is ignored.
        ignore_invalid_endpoint: bool,
    },
    /// Sets the exchange pattern (`setExchangePattern`).
    SetExchangePattern(ExchangePattern),
    /// Converts the body to the Java type (`convertBodyTo`).
    ConvertBodyTo {
        /// The Java type name, such as `String` or `java.lang.Integer`.
        type_name: String,
        /// Whether a body that cannot be converted fails (default `true`).
        mandatory: bool,
    },
    /// Calls a method of a bean (`bean`): by name in the registry or the bean itself; the
    /// result becomes the message body.
    Bean {
        /// The bean.
        bean: camel_api::bean_binding::BeanReference,
        /// The method (a method call such as `hello('World')`), `None` for the only method of
        /// the bean.
        method: Option<String>,
    },
    /// Sets several headers, in order (`setHeaders`).
    SetHeaders(Vec<(String, ExpressionDefinition)>),
    /// Sets several variables, in order (`setVariables`).
    SetVariables(Vec<(String, ExpressionDefinition)>),
    /// Removes an exchange property (`removeProperty`).
    RemoveProperty(String),
    /// Removes the exchange properties matching the pattern (`removeProperties`).
    RemoveProperties {
        /// The pattern of the names.
        pattern: String,
        /// The patterns of the names to keep.
        exclude_patterns: Vec<String>,
    },
    /// Removes a variable, all of them with `*` (`removeVariable`).
    RemoveVariable(String),
    /// Evaluates an expression for its side effects (`script`).
    Script(ExpressionDefinition),
    /// Processes the steps a number of times or while a predicate matches (`loop`).
    Loop {
        /// The number of iterations.
        count: Option<ExpressionDefinition>,
        /// The predicate of `loopDoWhile`.
        while_predicate: Option<ExpressionDefinition>,
        /// Whether each iteration starts from a copy of the original exchange.
        copy: bool,
        /// The steps of each iteration.
        steps: Vec<ProcessorDefinition>,
    },
    /// Sends to the endpoints of the slip one after the other (`routingSlip`).
    RoutingSlip {
        /// The uris of the slip.
        expression: ExpressionDefinition,
        /// The delimiter of the uris (default `,`).
        delimiter: Option<String>,
        /// Whether invalid endpoint uris are skipped.
        ignore_invalid_endpoints: bool,
    },
    /// Sends to the endpoints the expression answers, evaluated again after each one
    /// (`dynamicRouter`).
    DynamicRouter {
        /// The uris of the next endpoints, nothing to end.
        expression: ExpressionDefinition,
        /// The delimiter of the uris (default `,`).
        delimiter: Option<String>,
        /// Whether invalid endpoint uris are skipped.
        ignore_invalid_endpoints: bool,
    },
    /// Sorts the list of the expression, by default the body, as the body (`sort`).
    Sort {
        /// The list to sort (`None`: the body).
        expression: Option<ExpressionDefinition>,
        /// The comparator (`None`: the natural order).
        comparator: Option<SortComparator>,
    },
    /// The steps as a step with an id (`step`).
    Step {
        /// The id of the step.
        id: String,
        /// The steps.
        steps: Vec<ProcessorDefinition>,
    },
    /// `doTry` with its `doCatch` and `doFinally` clauses.
    DoTry {
        /// The steps of the try block.
        steps: Vec<ProcessorDefinition>,
        /// The catch clauses.
        catches: Vec<CatchDefinition>,
        /// The steps of the finally clause.
        finally: Option<Vec<ProcessorDefinition>>,
    },
}

impl Eip {
    /// The short name (Java `getShortName()`).
    pub fn short_name(&self) -> &'static str {
        match self {
            Eip::To { .. } => "to",
            Eip::Filter { .. } => "filter",
            Eip::Choice { .. } => "choice",
            Eip::Pipeline { .. } => "pipeline",
            Eip::SetBody(_) => "setBody",
            Eip::Transform(_) => "transform",
            Eip::SetHeader { .. } => "setHeader",
            Eip::SetProperty { .. } => "setProperty",
            Eip::SetVariable { .. } => "setVariable",
            Eip::RemoveHeader(_) => "removeHeader",
            Eip::RemoveHeaders { .. } => "removeHeaders",
            Eip::Log { .. } => "log",
            Eip::Stop => "stop",
            Eip::Process(_) => "process",
            Eip::ThrowException(_) => "throwException",
            Eip::DoTry { .. } => "doTry",
            Eip::Split { .. } => "split",
            Eip::Multicast { .. } => "multicast",
            Eip::Delay(_) => "delay",
            Eip::Aggregate(_) => "aggregate",
            Eip::OnCompletion(_) => "onCompletion",
            Eip::ToD { .. } => "toD",
            Eip::RecipientList { .. } => "recipientList",
            Eip::WireTap { .. } => "wireTap",
            Eip::SetExchangePattern(_) => "setExchangePattern",
            Eip::ConvertBodyTo { .. } => "convertBodyTo",
            Eip::Bean { .. } => "bean",
            Eip::SetHeaders(_) => "setHeaders",
            Eip::SetVariables(_) => "setVariables",
            Eip::RemoveProperty(_) => "removeProperty",
            Eip::RemoveProperties { .. } => "removeProperties",
            Eip::RemoveVariable(_) => "removeVariable",
            Eip::Script(_) => "script",
            Eip::Loop { .. } => "loop",
            Eip::RoutingSlip { .. } => "routingSlip",
            Eip::DynamicRouter { .. } => "dynamicRouter",
            Eip::Sort { .. } => "sort",
            Eip::Step { .. } => "step",
        }
    }
}

/// Formats the steps like a Java list: `[a, b]`.
pub struct Steps<'a>(pub &'a [ProcessorDefinition]);

impl fmt::Display for Steps<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (i, s) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{s}")?;
        }
        f.write_str("]")
    }
}

impl fmt::Display for WhenDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "When[{} -> {}]", self.predicate, Steps(&self.steps))
    }
}

impl fmt::Display for ProcessorDefinition {
    /// Java `toString()` of the definitions, such as `to[mock:a]` and
    /// `Filter[header(foo) == bar -> [to[mock:a]]]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.eip {
            Eip::To { uri, .. } => write!(f, "to[{}]", sanitize_uri(uri)),
            Eip::Filter {
                predicate, steps, ..
            } => write!(f, "Filter[{predicate} -> {}]", Steps(steps)),
            Eip::Choice { whens, otherwise } => {
                f.write_str("Choice[[")?;
                for (i, w) in whens.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{w}")?;
                }
                f.write_str("]")?;
                if let Some(o) = otherwise {
                    write!(f, " Otherwise[{}]", Steps(o))?;
                }
                f.write_str("]")
            }
            Eip::Pipeline { steps } => write!(f, "Pipeline[{}]", Steps(steps)),
            Eip::SetBody(e) => write!(f, "SetBody[{e}]"),
            Eip::Transform(e) => write!(f, "Transform[{e}]"),
            Eip::SetHeader { name, expression } => write!(f, "SetHeader[{name}, {expression}]"),
            Eip::SetProperty { name, expression } => write!(f, "SetProperty[{name}, {expression}]"),
            Eip::SetVariable { name, expression } => write!(f, "SetVariable[{name}, {expression}]"),
            Eip::RemoveHeader(name) => write!(f, "RemoveHeader[{name}]"),
            Eip::RemoveHeaders { pattern, .. } => write!(f, "RemoveHeaders[{pattern}]"),
            Eip::Log { message, .. } => write!(f, "Log[{message}]"),
            Eip::Stop => f.write_str("Stop"),
            Eip::Process(ProcessRef::Ref(r)) => write!(f, "process[ref:{r}]"),
            Eip::Process(ProcessRef::Processor(p)) => {
                write!(f, "process[Processor@{:p}]", std::sync::Arc::as_ptr(p))
            }
            Eip::ThrowException(e) => write!(
                f,
                "ThrowException[{}]",
                camel_api::error::error_type_name(e)
            ),
            Eip::Split {
                expression, steps, ..
            } => write!(f, "Split[{expression} -> {}]", Steps(steps)),
            Eip::Multicast { steps, .. } => write!(f, "Multicast[{}]", Steps(steps)),
            Eip::Delay(e) => write!(f, "Delay[{e} -> []]"),
            Eip::Aggregate(a) => write!(f, "Aggregate[{} -> {}]", a.correlation, Steps(&a.steps)),
            Eip::OnCompletion(c) => write!(f, "onCompletion[{}]", Steps(&c.steps)),
            Eip::ToD { uri, .. } => write!(f, "DynamicTo[toD[{uri}]]"),
            Eip::RecipientList { expression, .. } => write!(f, "RecipientList[{expression}]"),
            Eip::WireTap { uri, .. } => write!(f, "WireTap[{uri}]"),
            Eip::SetExchangePattern(p) => write!(f, "setExchangePattern[pattern: {p}]"),
            Eip::ConvertBodyTo { type_name, .. } => write!(f, "ConvertBodyTo[{type_name}]"),
            Eip::SetHeaders(h) | Eip::SetVariables(h) => {
                let kind = if matches!(self.eip, Eip::SetHeaders(_)) {
                    "SetHeaders"
                } else {
                    "SetVariables"
                };
                let items: Vec<String> = h.iter().map(|(n, e)| format!("{n}={e}")).collect();
                write!(f, "{kind}[{}]", items.join(", "))
            }
            Eip::RemoveProperty(name) => write!(f, "RemoveProperty[{name}]"),
            Eip::RemoveProperties { pattern, .. } => write!(f, "RemoveProperties[{pattern}]"),
            Eip::RemoveVariable(name) => write!(f, "RemoveVariable[{name}]"),
            Eip::Script(e) => write!(f, "Script[{e}]"),
            Eip::Loop {
                count,
                while_predicate,
                steps,
                ..
            } => {
                let e = count
                    .as_ref()
                    .or(while_predicate.as_ref())
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                write!(f, "Loop[{e} -> {}]", Steps(steps))
            }
            Eip::RoutingSlip { expression, .. } => write!(f, "RoutingSlip[{expression}]"),
            Eip::DynamicRouter { expression, .. } => write!(f, "DynamicRouter[{expression}]"),
            Eip::Sort { expression, .. } => match expression {
                Some(e) => write!(f, "sort[{e}]"),
                None => f.write_str("sort[body]"),
            },
            Eip::Step { steps, .. } => write!(f, "Step[{}]", Steps(steps)),
            Eip::Bean { bean, method } => match bean {
                camel_api::bean_binding::BeanReference::Name(name) => match method {
                    Some(m) => write!(f, "Bean[ref:{name} method:{m}]"),
                    None => write!(f, "Bean[ref:{name}]"),
                },
                camel_api::bean_binding::BeanReference::Bean(b) => write!(f, "Bean[{b}]"),
            },
            Eip::DoTry {
                steps,
                catches,
                finally,
            } => {
                // Java: the clauses are outputs of the try definition
                let mut parts: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
                parts.extend(catches.iter().map(|c| c.to_string()));
                if let Some(fs) = finally {
                    parts.push(format!("DoFinally[{}]", Steps(fs)));
                }
                write!(f, "DoTry[[{}]]", parts.join(", "))
            }
        }
    }
}
