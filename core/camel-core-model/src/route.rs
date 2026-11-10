//! Routes of the model (Java `RouteDefinition` and `FromDefinition`).

use crate::errorhandler::{ErrorHandlerDefinition, OnExceptionDefinition};
use crate::processor::{ProcessorDefinition, Steps};
use camel_util::uri_support::sanitize_uri;
use camel_util::value::ValueMap;
use std::fmt;

/// A route: the endpoint to consume from and the steps.
#[derive(Clone, Debug)]
pub struct RouteDefinition {
    /// The route id (generated when not set).
    pub id: Option<String>,
    /// The uri of the endpoint to consume from.
    pub from: String,
    /// The steps.
    pub steps: Vec<ProcessorDefinition>,
    /// Whether the route starts with the context (default `true`).
    pub auto_startup: bool,
    /// The startup order.
    pub startup_order: Option<i32>,
    /// The route group.
    pub group: Option<String>,
    /// The route description.
    pub description: Option<String>,
    /// The source location (file and line) of the route.
    pub source_location: Option<String>,
    /// The route properties.
    pub properties: ValueMap,
    /// The error handler of the route (`None`: the one of the route builder, else the default
    /// error handler).
    pub error_handler: Option<ErrorHandlerDefinition>,
    /// The `onException` clauses of the route.
    pub on_exceptions: Vec<OnExceptionDefinition>,
    /// The `onException` clauses of the route builder (they apply to all its routes).
    pub builder_on_exceptions: Vec<OnExceptionDefinition>,
}

impl RouteDefinition {
    /// A route from the endpoint, without steps.
    pub fn new(from: impl Into<String>) -> Self {
        RouteDefinition {
            id: None,
            from: from.into(),
            steps: Vec::new(),
            auto_startup: true,
            startup_order: None,
            group: None,
            description: None,
            source_location: None,
            properties: ValueMap::new(),
            error_handler: None,
            on_exceptions: Vec::new(),
            builder_on_exceptions: Vec::new(),
        }
    }
}

impl fmt::Display for RouteDefinition {
    /// Java `toString()`: `Route(id)[From[uri] -> [steps]]`, or `Route[From[uri] -> [steps]]`
    /// without an id.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let from = sanitize_uri(&self.from);
        match &self.id {
            Some(id) => write!(f, "Route({id})[From[{from}] -> {}]", Steps(&self.steps)),
            None => write!(f, "Route[From[{from}] -> {}]", Steps(&self.steps)),
        }
    }
}
