//! Throttling route policies (the Java `org.apache.camel.throttling` package).

use camel_api::endpoint::SharedConsumer;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::route::{Route, SharedRoute};
use camel_api::spi::inflight::InflightRepository;
use camel_api::spi::route_policy::RoutePolicy;
use std::sync::atomic::Ordering as AtomicOrdering;
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

    fn size(&self, route: &SharedRoute, exchange: &Exchange) -> usize {
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
    pub fn throttle(&self, route: &SharedRoute, exchange: &Exchange) {
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
    fn route_controller_took_over(&self, route: &SharedRoute) {
        if let Some(consumer) = route.consumer() {
            self.forget(&consumer);
        }
    }
}

impl RoutePolicy for ThrottlingInflightRoutePolicy {
    fn on_start(&self, route: &SharedRoute) {
        self.route_controller_took_over(route);
    }

    fn on_stop(&self, route: &SharedRoute) {
        self.route_controller_took_over(route);
    }

    fn on_suspend(&self, route: &SharedRoute) {
        self.route_controller_took_over(route);
    }

    fn on_resume(&self, route: &SharedRoute) {
        self.route_controller_took_over(route);
    }

    fn on_exchange_done(&self, route: &SharedRoute, exchange: &Exchange) {
        self.throttle(route, exchange);
    }
}

/// Decides whether an open circuit of a [`ThrottlingExceptionRoutePolicy`] can be closed when
/// it would be half opened (Java `ThrottlingExceptionHalfOpenHandler`), for example by checking
/// a remote service.
pub trait ThrottlingExceptionHalfOpenHandler: Send + Sync {
    /// Whether the circuit can be closed (else it opens again).
    fn is_ready_to_be_closed(&self) -> bool;
}

const STATE_CLOSED: u8 = 0;
const STATE_HALF_OPEN: u8 = 1;
const STATE_OPEN: u8 = 2;

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A circuit breaker of a route (Java `ThrottlingExceptionRoutePolicy`): when
/// `failure_threshold` exchanges failed within the last `failure_window` millis, the circuit
/// opens (the consumer of the route stops); after `half_open_after` millis it is half opened
/// (the consumer starts again, or the half open handler decides to close or reopen it), and
/// the next exchange closes it, or opens it again when the failures are still over the
/// threshold. With `keep_open` the circuit stays open.
///
/// The consumer is stopped and started on another task (see
/// [`route_policy_support`](crate::route_policy_support)), so a few exchanges may still be
/// consumed right after the circuit opens.
pub struct ThrottlingExceptionRoutePolicy {
    inner: Arc<ExceptionPolicyState>,
}

struct ExceptionPolicyState {
    failure_threshold: usize,
    failure_window: u64,
    half_open_after: u64,
    exceptions: Vec<camel_api::error::ErrorKind>,
    keep_open: std::sync::atomic::AtomicBool,
    half_open_handler: Option<Arc<dyn ThrottlingExceptionHalfOpenHandler>>,
    state_level: Option<log::Level>,
    failures: std::sync::atomic::AtomicUsize,
    success: std::sync::atomic::AtomicUsize,
    state: std::sync::atomic::AtomicU8,
    last_failure: std::sync::atomic::AtomicU64,
    opened_at: std::sync::atomic::AtomicU64,
    lock: Mutex<()>,
    timer: Mutex<Option<tokio::task::JoinHandle<()>>>,
    route: Mutex<Option<std::sync::Weak<dyn Route>>>,
}

impl ThrottlingExceptionRoutePolicy {
    /// The policy with the threshold of failures within the window (millis), half opening
    /// after the millis, counting the failures of the kinds (all when empty).
    pub fn new(
        failure_threshold: usize,
        failure_window: u64,
        half_open_after: u64,
        exceptions: Vec<camel_api::error::ErrorKind>,
    ) -> Self {
        Self::build(
            failure_threshold,
            failure_window,
            half_open_after,
            exceptions,
            false,
            None,
        )
    }

    fn build(
        failure_threshold: usize,
        failure_window: u64,
        half_open_after: u64,
        exceptions: Vec<camel_api::error::ErrorKind>,
        keep_open: bool,
        half_open_handler: Option<Arc<dyn ThrottlingExceptionHalfOpenHandler>>,
    ) -> Self {
        ThrottlingExceptionRoutePolicy {
            inner: Arc::new(ExceptionPolicyState {
                failure_threshold,
                failure_window,
                half_open_after,
                exceptions,
                keep_open: std::sync::atomic::AtomicBool::new(keep_open),
                half_open_handler,
                state_level: Some(log::Level::Debug),
                failures: Default::default(),
                success: Default::default(),
                state: std::sync::atomic::AtomicU8::new(STATE_CLOSED),
                last_failure: Default::default(),
                opened_at: Default::default(),
                lock: Mutex::new(()),
                timer: Mutex::new(None),
                route: Mutex::new(None),
            }),
        }
    }

    /// With the circuit kept open (Java `keepOpen`, for development and tests).
    pub fn with_keep_open(self, keep_open: bool) -> Self {
        self.inner
            .keep_open
            .store(keep_open, AtomicOrdering::SeqCst);
        self
    }

    /// With the half open handler (Java `halfOpenHandler`).
    pub fn with_half_open_handler(
        self,
        handler: Arc<dyn ThrottlingExceptionHalfOpenHandler>,
    ) -> Self {
        let i = &self.inner;
        Self::build(
            i.failure_threshold,
            i.failure_window,
            i.half_open_after,
            i.exceptions.clone(),
            i.keep_open.load(AtomicOrdering::SeqCst),
            Some(handler),
        )
    }

    /// Sets whether the circuit is kept open (Java `setKeepOpen`): keeping it open opens it.
    pub fn set_keep_open(&self, keep_open: bool) {
        self.inner
            .keep_open
            .store(keep_open, AtomicOrdering::SeqCst);
        if keep_open {
            if let Some(route) = self.inner.route() {
                self.inner.open_circuit(&route);
            }
        }
    }

    /// The state: `closed`, `half opened` or `opened`.
    pub fn state_as_string(&self) -> &'static str {
        self.inner.state_as_string()
    }

    /// The number of failures counted.
    pub fn failures(&self) -> usize {
        self.inner.failures.load(AtomicOrdering::SeqCst)
    }

    /// The number of successes counted.
    pub fn success(&self) -> usize {
        self.inner.success.load(AtomicOrdering::SeqCst)
    }
}

impl ExceptionPolicyState {
    fn route(&self) -> Option<SharedRoute> {
        self.route
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade())
    }

    fn state_as_string(&self) -> &'static str {
        match self.state.load(AtomicOrdering::SeqCst) {
            STATE_CLOSED => "closed",
            STATE_HALF_OPEN => "half opened",
            _ => "opened",
        }
    }

    fn log_state(&self) {
        if let Some(level) = self.state_level {
            let failures = self.failures.load(AtomicOrdering::SeqCst);
            if failures > 0 {
                let ago =
                    now_millis().saturating_sub(self.last_failure.load(AtomicOrdering::SeqCst));
                log::log!(
                    level,
                    "State {}, failures {failures}, last failure {ago} ms ago",
                    self.state_as_string()
                );
            } else {
                log::log!(
                    level,
                    "State {}, failures {failures}",
                    self.state_as_string()
                );
            }
        }
    }

    fn has_failed(&self, exchange: &Exchange) -> bool {
        match exchange.exception() {
            None => false,
            Some(_) if self.exceptions.is_empty() => true,
            Some(e) => self.exceptions.iter().any(|k| e.find_cause(*k).is_some()),
        }
    }

    fn threshold_exceeded(&self) -> bool {
        self.log_state();
        self.failures.load(AtomicOrdering::SeqCst) >= self.failure_threshold
            && self.last_failure.load(AtomicOrdering::SeqCst)
                >= now_millis().saturating_sub(self.failure_window)
    }

    fn calculate_state(self: &Arc<Self>, route: &SharedRoute) {
        let exceeded = self.threshold_exceeded();
        match self.state.load(AtomicOrdering::SeqCst) {
            STATE_CLOSED => {
                if exceeded {
                    self.open_circuit(route);
                }
            }
            STATE_HALF_OPEN => {
                if exceeded {
                    self.open_circuit(route);
                } else {
                    self.close_circuit(route);
                }
            }
            _ => {
                if self.keep_open.load(AtomicOrdering::SeqCst) {
                    self.add_half_open_timer(route);
                } else {
                    let elapsed =
                        now_millis().saturating_sub(self.opened_at.load(AtomicOrdering::SeqCst));
                    if self.half_open_after <= elapsed {
                        match &self.half_open_handler {
                            Some(h) if h.is_ready_to_be_closed() => self.close_circuit(route),
                            Some(_) => self.reopen_circuit(route),
                            None => self.half_open_circuit(route),
                        }
                    }
                }
            }
        }
    }

    fn stop_consumer(route: &SharedRoute) {
        if let Some(c) = route.consumer() {
            crate::route_policy_support::stop_consumer(c);
        }
    }

    fn start_consumer(route: &SharedRoute) {
        // not a route the route controller stopped
        if route.status().is_started() {
            if let Some(c) = route.consumer() {
                crate::route_policy_support::start_consumer(c);
            }
        }
    }

    fn open_circuit(self: &Arc<Self>, route: &SharedRoute) {
        {
            let _lock = self.lock.lock().unwrap();
            if self.state.load(AtomicOrdering::SeqCst) == STATE_OPEN {
                return;
            }
            Self::stop_consumer(route);
            self.state.store(STATE_OPEN, AtomicOrdering::SeqCst);
            self.opened_at.store(now_millis(), AtomicOrdering::SeqCst);
        }
        self.add_half_open_timer(route);
        self.log_state();
    }

    fn reopen_circuit(self: &Arc<Self>, route: &SharedRoute) {
        {
            let _lock = self.lock.lock().unwrap();
            Self::stop_consumer(route);
            self.state.store(STATE_OPEN, AtomicOrdering::SeqCst);
            self.opened_at.store(now_millis(), AtomicOrdering::SeqCst);
        }
        self.add_half_open_timer(route);
        self.log_state();
    }

    fn half_open_circuit(&self, route: &SharedRoute) {
        let _lock = self.lock.lock().unwrap();
        Self::start_consumer(route);
        self.state.store(STATE_HALF_OPEN, AtomicOrdering::SeqCst);
        self.log_state();
    }

    fn close_circuit(&self, route: &SharedRoute) {
        let _lock = self.lock.lock().unwrap();
        if self.state.load(AtomicOrdering::SeqCst) == STATE_OPEN {
            Self::start_consumer(route);
        }
        self.failures.store(0, AtomicOrdering::SeqCst);
        self.success.store(0, AtomicOrdering::SeqCst);
        self.last_failure.store(0, AtomicOrdering::SeqCst);
        self.opened_at.store(0, AtomicOrdering::SeqCst);
        self.state.store(STATE_CLOSED, AtomicOrdering::SeqCst);
        self.log_state();
    }

    /// Checks the state again after `half_open_after` millis (Java `HalfOpenTask`).
    fn add_half_open_timer(self: &Arc<Self>, route: &SharedRoute) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (this, route) = (Arc::downgrade(self), Arc::downgrade(route));
        let delay = std::time::Duration::from_millis(self.half_open_after);
        let task = handle.spawn(async move {
            tokio::time::sleep(delay).await;
            if let (Some(this), Some(route)) = (this.upgrade(), route.upgrade()) {
                this.calculate_state(&route);
            }
        });
        if let Some(previous) = self.timer.lock().unwrap().replace(task) {
            previous.abort();
        }
    }
}

impl RoutePolicy for ThrottlingExceptionRoutePolicy {
    fn on_init(&self, route: &SharedRoute) {
        *self.inner.route.lock().unwrap() = Some(Arc::downgrade(route));
        self.inner.log_state();
    }

    fn on_start(&self, route: &SharedRoute) {
        if self.inner.keep_open.load(AtomicOrdering::SeqCst) {
            self.inner.reopen_circuit(route);
        }
    }

    fn on_exchange_done(&self, route: &SharedRoute, exchange: &Exchange) {
        let s = &self.inner;
        if s.keep_open.load(AtomicOrdering::SeqCst) {
            if s.state.load(AtomicOrdering::SeqCst) != STATE_OPEN {
                s.open_circuit(route);
            }
        } else {
            if s.has_failed(exchange) {
                s.failures.fetch_add(1, AtomicOrdering::SeqCst);
                s.last_failure.store(now_millis(), AtomicOrdering::SeqCst);
            } else {
                s.success.fetch_add(1, AtomicOrdering::SeqCst);
            }
            s.calculate_state(route);
        }
    }

    fn stop(&self) {
        if let Some(t) = self.inner.timer.lock().unwrap().take() {
            t.abort();
        }
    }
}
