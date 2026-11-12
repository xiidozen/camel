//! The interceptors of the routes of a route builder (Java `InterceptDefinition`,
//! `InterceptFromDefinition` and `InterceptSendToEndpointDefinition`): steps run before each
//! step of the routes, when a route consumes, or before sending to matching endpoints.

use crate::expression::ExpressionDefinition;
use crate::processor::{ProcessorDefinition, Steps};
use std::fmt;

/// `intercept()`: steps run before each step of the routes (a `stop()` in them stops the
/// routing).
#[derive(Debug, Clone, Default)]
pub struct InterceptDefinition {
    /// The predicate of the exchanges to intercept (`None`: all).
    pub on_when: Option<ExpressionDefinition>,
    /// The steps of the interception.
    pub steps: Vec<ProcessorDefinition>,
}

impl fmt::Display for InterceptDefinition {
    /// Java `toString()`: `Intercept[[steps]]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Intercept[{}]", Steps(&self.steps))
    }
}

/// `interceptFrom(uri)`: steps run when a route consuming from a matching endpoint receives an
/// exchange.
#[derive(Debug, Clone, Default)]
pub struct InterceptFromDefinition {
    /// The pattern of the endpoint uris (exact, wildcard `*` or regex; `None`: all).
    pub uri: Option<String>,
    /// The predicate of the exchanges to intercept (`None`: all).
    pub on_when: Option<ExpressionDefinition>,
    /// The steps of the interception.
    pub steps: Vec<ProcessorDefinition>,
}

impl fmt::Display for InterceptFromDefinition {
    /// Java `toString()`: `InterceptFrom[[steps]]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InterceptFrom[{}]", Steps(&self.steps))
    }
}

/// `interceptSendToEndpoint(uri)`: steps run before sending to a matching endpoint, which can
/// skip the sending or send to another endpoint after it.
#[derive(Debug, Clone, Default)]
pub struct InterceptSendToEndpointDefinition {
    /// The pattern of the endpoint uris (exact, wildcard `*` or regex).
    pub uri: String,
    /// The predicate of the exchanges to intercept (`None`: all).
    pub on_when: Option<ExpressionDefinition>,
    /// Whether the sending to the original endpoint is skipped.
    pub skip_send_to_original_endpoint: bool,
    /// An endpoint the exchange is sent to after the original one.
    pub after_uri: Option<String>,
    /// The steps of the interception.
    pub steps: Vec<ProcessorDefinition>,
}

impl fmt::Display for InterceptSendToEndpointDefinition {
    /// Java `toString()`: `interceptSendToEndpoint[uri -> [steps]]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "interceptSendToEndpoint[{} -> {}]",
            self.uri,
            Steps(&self.steps)
        )
    }
}
