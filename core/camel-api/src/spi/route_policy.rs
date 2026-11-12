//! Route policies (Java `RoutePolicy` and `RoutePolicyFactory`): callbacks on the lifecycle of
//! a route and on the exchanges it routes, for example to stop the consumer of a route after
//! some exchanges or to throttle it.

use crate::context::CamelContext;
use crate::exchange::Exchange;
use crate::route::SharedRoute;
use camel_util::value::{ObjectRef, Value};
use std::sync::Arc;

/// A policy of a route (Java `RoutePolicy`); every callback does nothing by default (Java
/// `RoutePolicySupport`).
pub trait RoutePolicy: Send + Sync {
    /// The route is added to the context.
    fn on_init(&self, route: &SharedRoute) {
        let _ = route;
    }

    /// The route is removed from the context.
    fn on_remove(&self, route: &SharedRoute) {
        let _ = route;
    }

    /// The route is started.
    fn on_start(&self, route: &SharedRoute) {
        let _ = route;
    }

    /// The route is stopped.
    fn on_stop(&self, route: &SharedRoute) {
        let _ = route;
    }

    /// The route is suspended.
    fn on_suspend(&self, route: &SharedRoute) {
        let _ = route;
    }

    /// The route is resumed.
    fn on_resume(&self, route: &SharedRoute) {
        let _ = route;
    }

    /// An exchange enters the route.
    fn on_exchange_begin(&self, route: &SharedRoute, exchange: &mut Exchange) {
        let _ = (route, exchange);
    }

    /// An exchange is done with the route (completed or failed).
    fn on_exchange_done(&self, route: &SharedRoute, exchange: &Exchange) {
        let _ = (route, exchange);
    }

    /// The policy is started, with its route (Java `doStart` of the policy service).
    fn start(&self) {}

    /// The policy is stopped, with the context (Java `doStop` of the policy service).
    fn stop(&self) {}
}

/// A shared [`RoutePolicy`].
pub type SharedRoutePolicy = Arc<dyn RoutePolicy>;

/// Creates the policies of the routes of a context (Java `RoutePolicyFactory`).
pub trait RoutePolicyFactory: Send + Sync {
    /// The policy of the route, if any.
    fn create_route_policy(
        &self,
        context: &Arc<dyn CamelContext>,
        route_id: &str,
    ) -> Option<SharedRoutePolicy>;
}

/// A shared [`RoutePolicyFactory`].
pub type SharedRoutePolicyFactory = Arc<dyn RoutePolicyFactory>;

/// The policy as a value, to bind it in the registry (`routePolicyRef`).
pub fn route_policy_value(policy: SharedRoutePolicy) -> Value {
    Value::Object(ObjectRef::new(policy))
}

/// The policy held by a value of the registry (see [`route_policy_value`]).
pub fn route_policy_of(value: &Value) -> Option<SharedRoutePolicy> {
    match value {
        Value::Object(o) => o.downcast_ref::<SharedRoutePolicy>().cloned(),
        _ => None,
    }
}
