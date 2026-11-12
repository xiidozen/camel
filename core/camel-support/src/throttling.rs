//! Throttling route policies (the Java `org.apache.camel.throttling` package).

use camel_api::endpoint::SharedConsumer;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::route::Route;
use camel_api::spi::inflight::InflightRepository;
use camel_api::spi::route_policy::RoutePolicy;
use std::sync::{Arc, Mutex};

/// The scope of the inflight exchanges a [`ThrottlingInflightRoutePolicy`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThrottlingScope {
    /// All the exchanges of the context.
    Context,
    /// The exchanges of the route (the default).
    #[default]
    Route,
}

/// Suspends the consumer of a route while it has too many inflight exchanges, and resumes it
/// once they are under a percentage of the maximum (Java `ThrottlingInflightRoutePolicy`).
///
/// The consumer is stopped and started again (Java suspends a suspendable consumer); a
/// consumer the policy did not stop is never started by it. With the context scope the total
/// number of inflight exchanges is compared when an exchange of a route of the policy is done
/// (Java listens to all the completed exchanges of the context).
pub struct ThrottlingInflightRoutePolicy {
    scope: ThrottlingScope,
    max_inflight_exchanges: usize,
    resume_percent_of_max: usize,
    logging_level: Option<log::Level>,
    suspended: Mutex<Vec<SharedConsumer>>,
}

impl Default for ThrottlingInflightRoutePolicy {
    fn default() -> Self {
        ThrottlingInflightRoutePolicy {
            scope: ThrottlingScope::Route,
            max_inflight_exchanges: 1000,
            resume_percent_of_max: 70,
            logging_level: Some(log::Level::Info),
            suspended: Mutex::new(Vec::new()),
        }
    }
}

impl std::fmt::Display for ThrottlingInflightRoutePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ThrottlingInflightRoutePolicy[{} / {}% using scope {:?}]",
            self.max_inflight_exchanges, self.resume_percent_of_max, self.scope
        )
    }
}

impl ThrottlingInflightRoutePolicy {
    /// The policy with the defaults: at most 1000 inflight exchanges of the route, resuming at
    /// 70%.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of inflight exchanges (Java `maxInflightExchanges`).
    pub fn with_max_inflight_exchanges(mut self, max: usize) -> Self {
        self.max_inflight_exchanges = max;
        self
    }

    /// Sets the percentage of the maximum at which the consumer is resumed (Java
    /// `resumePercentOfMax`).
    ///
    /// # Errors
    ///
    /// A percentage over 100.
    pub fn with_resume_percent_of_max(mut self, percent: usize) -> CamelResult<Self> {
        if percent > 100 {
            return Err(CamelError::illegal_argument(format!(
                "Must be a percentage between 0 and 100, was: {percent}"
            )));
        }
        self.resume_percent_of_max = percent;
        Ok(self)
    }

    /// Sets the scope (Java `scope`).
    pub fn with_scope(mut self, scope: ThrottlingScope) -> Self {
        self.scope = scope;
        self
    }

    /// Sets the level of the throttling logs (`None`: off).
    pub fn with_logging_level(mut self, level: Option<log::Level>) -> Self {
        self.logging_level = level;
        self
    }

    /// The number of inflight exchanges at which the consumer is resumed (at least 1).
    pub fn resume_inflight_exchanges(&self) -> usize {
        (self.resume_percent_of_max * self.max_inflight_exchanges / 100).max(1)
    }

    fn size(&self, route: &dyn Route, exchange: &Exchange) -> usize {
        let Some(repo) =
            camel_api::context::context_plugin::<InflightRepository>(exchange.context().as_ref())
        else {
            return 0;
        };
        match self.scope {
            ThrottlingScope::Context => repo.size(),
            ThrottlingScope::Route => repo.route_size(route.id()),
        }
    }

    fn is_suspended(&self, consumer: &SharedConsumer) -> bool {
        self.suspended
            .lock()
            .unwrap()
            .iter()
            .any(|c| Arc::ptr_eq(c, consumer))
    }

    fn forget(&self, consumer: &SharedConsumer) {
        self.suspended
            .lock()
            .unwrap()
            .retain(|c| !Arc::ptr_eq(c, consumer));
    }

    fn log(&self, message: String) {
        if let Some(level) = self.logging_level {
            log::log!(level, "{message}");
        }
    }

    /// Throttles the consumer of the route (Java `throttle`).
    pub fn throttle(&self, route: &dyn Route, exchange: &Exchange) {
        let Some(consumer) = route.consumer() else {
            return;
        };
        let max = self.max_inflight_exchanges;
        let size = self.size(route, exchange);
        if max > 0 && size > max && !self.is_suspended(&consumer) {
            self.suspended.lock().unwrap().push(consumer.clone());
            self.log(format!(
                "Throttling consumer: {size} > {max} inflight exchange by suspending consumer: {}",
                consumer.endpoint().endpoint_uri()
            ));
            crate::route_policy_support::stop_consumer(consumer.clone());
        }
        if self.suspended.lock().unwrap().is_empty() {
            return;
        }
        let size = self.size(route, exchange);
        let resume = self.resume_inflight_exchanges();
        // only a consumer it suspended, of a route the route controller did not stop
        if size <= resume && self.is_suspended(&consumer) && route.status().is_started() {
            self.forget(&consumer);
            self.log(format!(
                "Throttling consumer: {size} <= {resume} inflight exchange by resuming consumer: {}",
                consumer.endpoint().endpoint_uri()
            ));
            crate::route_policy_support::start_consumer(consumer);
        }
    }

    /// The route controller took over the consumer.
    fn route_controller_took_over(&self, route: &dyn Route) {
        if let Some(consumer) = route.consumer() {
            self.forget(&consumer);
        }
    }
}

impl RoutePolicy for ThrottlingInflightRoutePolicy {
    fn on_start(&self, route: &dyn Route) {
        self.route_controller_took_over(route);
    }

    fn on_stop(&self, route: &dyn Route) {
        self.route_controller_took_over(route);
    }

    fn on_suspend(&self, route: &dyn Route) {
        self.route_controller_took_over(route);
    }

    fn on_resume(&self, route: &dyn Route) {
        self.route_controller_took_over(route);
    }

    fn on_exchange_done(&self, route: &dyn Route, exchange: &Exchange) {
        self.throttle(route, exchange);
    }
}
