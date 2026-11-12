//! A route policy acting after a number of messages or a duration (Java `DurationRoutePolicy`
//! and `DurationRoutePolicyFactory`).

use crate::context::DefaultCamelContext;
use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::route::SharedRoute;
use camel_api::spi::route_policy::{RoutePolicy, RoutePolicyFactory, SharedRoutePolicy};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// What a [`DurationRoutePolicy`] does (Java `DurationRoutePolicy.Action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurationAction {
    /// Stops the context.
    StopCamelContext,
    /// Stops the route (the default).
    #[default]
    StopRoute,
    /// Suspends the route.
    SuspendRoute,
    /// Suspends all the routes (the context).
    SuspendAllRoutes,
}

/// Stops (or suspends) its route, or the context, once the route processed `max_messages`
/// messages or `max_seconds` after the route was added (Java `DurationRoutePolicy`).
pub struct DurationRoutePolicy {
    context: Weak<DefaultCamelContext>,
    max_seconds: u64,
    max_messages: usize,
    action: DurationAction,
    route_id: Mutex<Option<String>>,
    done_messages: AtomicUsize,
    action_done: Arc<AtomicBool>,
    timer: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl DurationRoutePolicy {
    /// The policy of the context, with the limits (`0`: none; at least one is required).
    pub fn new(context: &DefaultCamelContext, max_messages: usize, max_seconds: u64) -> Self {
        DurationRoutePolicy {
            context: context.downgrade(),
            max_seconds,
            max_messages,
            action: DurationAction::StopRoute,
            route_id: Mutex::new(None),
            done_messages: AtomicUsize::new(0),
            action_done: Arc::new(AtomicBool::new(false)),
            timer: Mutex::new(None),
        }
    }

    /// Sets the action.
    pub fn with_action(mut self, action: DurationAction) -> Self {
        self.action = action;
        self
    }

    /// Performs the action on another task.
    fn perform(&self, tail: String) {
        let Some(route_id) = self.route_id.lock().unwrap().clone() else {
            return;
        };
        let (context, action) = (self.context.clone(), self.action);
        let task = async move {
            let Some(context) = context.upgrade() else {
                return;
            };
            let r = run_action(&context, action, &route_id, &tail).await;
            if let Err(e) = r {
                log::warn!("Error performing action: {action:?}: {e}");
            }
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(task);
        }
    }

    fn check(&self) -> CamelResult<()> {
        if self.max_messages == 0 && self.max_seconds == 0 {
            return Err(CamelError::illegal_argument(
                "The options maxMessages or maxSeconds must be configured",
            ));
        }
        Ok(())
    }
}

async fn run_action(
    context: &DefaultCamelContext,
    action: DurationAction,
    route_id: &str,
    tail: &str,
) -> CamelResult<()> {
    match action {
        DurationAction::StopCamelContext => {
            log::info!("Stopping CamelContext {tail}");
            context.stop().await
        }
        DurationAction::StopRoute => {
            log::info!("Stopping route: {route_id}{tail}");
            context.stop_route(route_id).await
        }
        DurationAction::SuspendRoute => {
            log::info!("Suspending route: {route_id}{tail}");
            context.suspend_route(route_id).await
        }
        DurationAction::SuspendAllRoutes => {
            log::info!("Suspending all routes {tail}");
            context.suspend().await
        }
    }
}

impl RoutePolicy for DurationRoutePolicy {
    fn on_init(&self, route: &SharedRoute) {
        if let Err(e) = self.check() {
            log::warn!("{e}");
            return;
        }
        self.route_id
            .lock()
            .unwrap()
            .get_or_insert_with(|| route.id().to_string());
        if self.max_seconds > 0 {
            let Ok(handle) = tokio::runtime::Handle::try_current() else {
                return;
            };
            let (seconds, done) = (self.max_seconds, self.action_done.clone());
            let (context, action) = (self.context.clone(), self.action);
            let route_id = route.id().to_string();
            *self.timer.lock().unwrap() = Some(handle.spawn(async move {
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                if done.swap(true, Ordering::SeqCst) {
                    return;
                }
                if let Some(context) = context.upgrade() {
                    let tail = format!(" due max seconds {seconds}");
                    if let Err(e) = run_action(&context, action, &route_id, &tail).await {
                        log::warn!("Error performing action: {action:?}: {e}");
                    }
                }
            }));
        }
    }

    fn on_exchange_done(&self, _: &SharedRoute, _: &Exchange) {
        let done = self.done_messages.fetch_add(1, Ordering::SeqCst) + 1;
        if self.max_messages > 0
            && done >= self.max_messages
            && !self.action_done.swap(true, Ordering::SeqCst)
        {
            if let Some(timer) = self.timer.lock().unwrap().take() {
                timer.abort();
            }
            self.perform(format!(" due max messages {} processed", self.max_messages));
        }
    }

    fn stop(&self) {
        if let Some(timer) = self.timer.lock().unwrap().take() {
            timer.abort();
        }
    }
}

/// Creates a [`DurationRoutePolicy`] for the routes (all, or those with the ids) (Java
/// `DurationRoutePolicyFactory`).
#[derive(Debug, Clone, Default)]
pub struct DurationRoutePolicyFactory {
    /// The ids of the routes (all when empty).
    pub from_route_ids: Vec<String>,
    /// The maximum number of messages.
    pub max_messages: usize,
    /// The maximum number of seconds.
    pub max_seconds: u64,
    /// The action.
    pub action: DurationAction,
}

impl RoutePolicyFactory for DurationRoutePolicyFactory {
    fn create_route_policy(
        &self,
        context: &Arc<dyn CamelContext>,
        route_id: &str,
    ) -> Option<SharedRoutePolicy> {
        if !self.from_route_ids.is_empty() && !self.from_route_ids.iter().any(|id| id == route_id) {
            return None;
        }
        let context = context.as_any().downcast_ref::<DefaultCamelContext>()?;
        Some(Arc::new(
            DurationRoutePolicy::new(context, self.max_messages, self.max_seconds)
                .with_action(self.action),
        ))
    }
}
