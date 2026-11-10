//! The processor definitions of the model (Java `ProcessorDefinition` and its subclasses such
//! as `ToDefinition`, `FilterDefinition` and `ChoiceDefinition`).

use crate::expression::ExpressionDefinition;
use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::CamelError;
use camel_api::processor::SharedProcessor;
use camel_util::uri_support::sanitize_uri;
use std::fmt;

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
            Eip::Filter { steps, .. } | Eip::Pipeline { steps } => steps.iter().collect(),
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

/// A `when` clause of a choice.
#[derive(Clone, Debug)]
pub struct WhenDefinition {
    /// The predicate.
    pub predicate: ExpressionDefinition,
    /// The steps when the predicate matches.
    pub steps: Vec<ProcessorDefinition>,
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
        }
    }
}

/// Formats the steps like a Java list: `[a, b]`.
pub(crate) struct Steps<'a>(pub &'a [ProcessorDefinition]);

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
        }
    }
}
