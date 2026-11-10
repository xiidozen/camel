//! A route (Java `org.apache.camel.Route`): a consumer endpoint and the processors the
//! consumed exchanges are routed through.

use crate::endpoint::{SharedConsumer, SharedEndpoint};
use crate::processor::SharedProcessor;
use crate::service::StatefulService;
use camel_util::value::ValueMap;
use std::sync::Arc;

/// A running route.
pub trait Route: StatefulService {
    /// The route id.
    fn id(&self) -> &str;

    /// Whether the id was set by the user (not generated).
    fn is_custom_id(&self) -> bool;

    /// The route group, if any.
    fn group(&self) -> Option<&str> {
        None
    }

    /// The route description, if any.
    fn description(&self) -> Option<&str> {
        None
    }

    /// The source location (file and line) the route was defined in, if known.
    fn source_location(&self) -> Option<&str> {
        None
    }

    /// The endpoint the route consumes from.
    fn endpoint(&self) -> SharedEndpoint;

    /// The consumer of the route (once started).
    fn consumer(&self) -> Option<SharedConsumer>;

    /// The processor (the route pipeline) the consumed exchanges are passed to.
    fn processor(&self) -> SharedProcessor;

    /// The route properties.
    fn properties(&self) -> ValueMap {
        ValueMap::new()
    }

    /// Whether the route is started when the context starts.
    fn is_auto_startup(&self) -> bool {
        true
    }

    /// The startup order of the route, if set (routes with a lower order start first).
    fn startup_order(&self) -> Option<i32> {
        None
    }

    /// The uptime of the route in milliseconds (0 when not started).
    fn uptime_millis(&self) -> u64 {
        0
    }
}

/// A shared route.
pub type SharedRoute = Arc<dyn Route>;
