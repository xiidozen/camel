//! Routes of the model (Java `RouteDefinition` and `FromDefinition`).

use crate::errorhandler::{ErrorHandlerDefinition, OnExceptionDefinition};
use crate::intercept::{
    InterceptDefinition, InterceptFromDefinition, InterceptSendToEndpointDefinition,
};
use crate::processor::{OnCompletionDefinition, ProcessorDefinition, Steps};
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
    /// The `onCompletion`s of the route builder (they apply to its routes without their own).
    pub builder_on_completions: Vec<OnCompletionDefinition>,
    /// The data formats of the route builder by id (Java `<dataFormats>`), for `custom` refs.
    pub builder_data_formats: Vec<(String, crate::processor::DataFormatDefinition)>,
    /// The `intercept`s of the route builder.
    pub builder_intercepts: Vec<InterceptDefinition>,
    /// The `interceptFrom`s of the route builder.
    pub builder_intercept_froms: Vec<InterceptFromDefinition>,
    /// The `interceptSendToEndpoint`s of the route builder.
    pub builder_intercept_send_to_endpoints: Vec<InterceptSendToEndpointDefinition>,
    /// The policies of the route (Java `routePolicy` and `routePolicyRef`).
    pub route_policies: Vec<RoutePolicyRef>,
}

/// A policy of a route: the policy, or the name of a policy bean of the registry.
#[derive(Clone)]
pub enum RoutePolicyRef {
    /// The policy.
    Policy(camel_api::spi::route_policy::SharedRoutePolicy),
    /// The name of the policy bean.
    Ref(String),
}

impl fmt::Debug for RoutePolicyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoutePolicyRef::Policy(_) => write!(f, "RoutePolicy"),
            RoutePolicyRef::Ref(name) => write!(f, "RoutePolicyRef[{name}]"),
        }
    }
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
            builder_on_completions: Vec::new(),
            builder_data_formats: Vec::new(),
            builder_intercepts: Vec::new(),
            builder_intercept_froms: Vec::new(),
            builder_intercept_send_to_endpoints: Vec::new(),
            route_policies: Vec::new(),
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
