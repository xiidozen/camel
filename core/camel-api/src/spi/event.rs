//! The events of the CamelContext, its routes and the exchanges (Java `CamelEvent` and its
//! sub interfaces), and the notifiers receiving them (Java `EventNotifier`, with the ignore
//! options of `EventNotifierSupport`).
//!
//! The notifiers are added to the context (`DefaultCamelContext::add_event_notifier`, Java
//! `getManagementStrategy().addEventNotifier`); the engine sends the events with [`notify`].
//!
//! # Examples
//!
//! ```
//! use camel_api::spi::event::{CamelEvent, EventNotifierSupport, EventType};
//! use std::sync::{Arc, Mutex};
//!
//! let events = Arc::new(Mutex::new(Vec::new()));
//! let recorded = events.clone();
//! let notifier = EventNotifierSupport::new(move |e: &CamelEvent<'_>| {
//!     recorded.lock().unwrap().push(e.event_type());
//! })
//! .ignore_exchange_events(true);
//! # use camel_api::spi::event::EventNotifier;
//! notifier.notify(&CamelEvent::CamelContextStarted);
//! assert!(notifier.is_enabled(&CamelEvent::RouteAdded("r")));
//! assert_eq!(vec![EventType::CamelContextStarted], *events.lock().unwrap());
//! ```

use crate::context::CamelContext;
use crate::error::CamelError;
use crate::exchange::Exchange;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// The type of an event (Java `CamelEvent.Type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum EventType {
    CamelContextInitializing,
    CamelContextInitialized,
    CamelContextStarting,
    CamelContextStarted,
    CamelContextStartupFailure,
    CamelContextStopping,
    CamelContextStopped,
    CamelContextStopFailure,
    CamelContextRoutesStarting,
    CamelContextRoutesStarted,
    CamelContextRoutesStopping,
    CamelContextRoutesStopped,
    RouteAdded,
    RouteRemoved,
    RouteStarting,
    RouteStarted,
    RouteStopping,
    RouteStopped,
    ExchangeCreated,
    ExchangeCompleted,
    ExchangeFailed,
    ExchangeFailureHandling,
    ExchangeFailureHandled,
    ExchangeRedelivery,
    ExchangeSending,
    ExchangeSent,
}

/// An event; the exchange events refer to the exchange (the notifiers run synchronously,
/// while the exchange is routed).
#[derive(Debug, Clone, Copy)]
pub enum CamelEvent<'a> {
    /// The context is initializing (the first start).
    CamelContextInitializing,
    /// The context is initialized.
    CamelContextInitialized,
    /// The context is starting.
    CamelContextStarting,
    /// The context started.
    CamelContextStarted,
    /// The context failed to start.
    CamelContextStartupFailure(&'a CamelError),
    /// The context is stopping.
    CamelContextStopping,
    /// The context stopped.
    CamelContextStopped,
    /// The context failed to stop.
    CamelContextStopFailure(&'a CamelError),
    /// The routes are starting (with the context).
    CamelContextRoutesStarting,
    /// The routes started (with the context).
    CamelContextRoutesStarted,
    /// The routes are stopping (with the context).
    CamelContextRoutesStopping,
    /// The routes stopped (with the context).
    CamelContextRoutesStopped,
    /// The route of the id was added.
    RouteAdded(&'a str),
    /// The route of the id was removed.
    RouteRemoved(&'a str),
    /// The route of the id is starting.
    RouteStarting(&'a str),
    /// The route of the id started.
    RouteStarted(&'a str),
    /// The route of the id is stopping.
    RouteStopping(&'a str),
    /// The route of the id stopped.
    RouteStopped(&'a str),
    /// An exchange entered a route (its unit of work began).
    ExchangeCreated(&'a Exchange),
    /// The exchange completed (its unit of work is done, without error).
    ExchangeCompleted(&'a Exchange),
    /// The exchange failed (its unit of work is done, with an error).
    ExchangeFailed(&'a Exchange),
    /// A failure processor (a dead letter channel, an onException or a doCatch) is handling
    /// the failed exchange.
    ExchangeFailureHandling {
        /// The exchange.
        exchange: &'a Exchange,
        /// Whether the failure processor is a dead letter channel.
        dead_letter: bool,
        /// The uri of the dead letter channel.
        dead_letter_uri: Option<&'a str>,
    },
    /// The failure processor handled the failed exchange.
    ExchangeFailureHandled {
        /// The exchange.
        exchange: &'a Exchange,
        /// Whether the failure processor is a dead letter channel.
        dead_letter: bool,
        /// The uri of the dead letter channel.
        dead_letter_uri: Option<&'a str>,
        /// Whether the error handler marked the failure as handled; otherwise it is continued
        /// (Java `isContinued` is `!isHandled`).
        handled: bool,
    },
    /// The exchange is redelivered (the attempt number, from 1).
    ExchangeRedelivery {
        /// The exchange.
        exchange: &'a Exchange,
        /// The attempt number, from 1.
        attempt: i32,
    },
    /// The exchange is being sent to the endpoint.
    ExchangeSending {
        /// The exchange.
        exchange: &'a Exchange,
        /// The uri of the endpoint.
        endpoint: &'a str,
    },
    /// The exchange was sent to the endpoint.
    ExchangeSent {
        /// The exchange.
        exchange: &'a Exchange,
        /// The uri of the endpoint.
        endpoint: &'a str,
        /// The time the sending took.
        time_taken: Duration,
    },
}

impl CamelEvent<'_> {
    /// The type of the event.
    pub fn event_type(&self) -> EventType {
        use CamelEvent as E;
        match self {
            E::CamelContextInitializing => EventType::CamelContextInitializing,
            E::CamelContextInitialized => EventType::CamelContextInitialized,
            E::CamelContextStarting => EventType::CamelContextStarting,
            E::CamelContextStarted => EventType::CamelContextStarted,
            E::CamelContextStartupFailure(_) => EventType::CamelContextStartupFailure,
            E::CamelContextStopping => EventType::CamelContextStopping,
            E::CamelContextStopped => EventType::CamelContextStopped,
            E::CamelContextStopFailure(_) => EventType::CamelContextStopFailure,
            E::CamelContextRoutesStarting => EventType::CamelContextRoutesStarting,
            E::CamelContextRoutesStarted => EventType::CamelContextRoutesStarted,
            E::CamelContextRoutesStopping => EventType::CamelContextRoutesStopping,
            E::CamelContextRoutesStopped => EventType::CamelContextRoutesStopped,
            E::RouteAdded(_) => EventType::RouteAdded,
            E::RouteRemoved(_) => EventType::RouteRemoved,
            E::RouteStarting(_) => EventType::RouteStarting,
            E::RouteStarted(_) => EventType::RouteStarted,
            E::RouteStopping(_) => EventType::RouteStopping,
            E::RouteStopped(_) => EventType::RouteStopped,
            E::ExchangeCreated(_) => EventType::ExchangeCreated,
            E::ExchangeCompleted(_) => EventType::ExchangeCompleted,
            E::ExchangeFailed(_) => EventType::ExchangeFailed,
            E::ExchangeFailureHandling { .. } => EventType::ExchangeFailureHandling,
            E::ExchangeFailureHandled { .. } => EventType::ExchangeFailureHandled,
            E::ExchangeRedelivery { .. } => EventType::ExchangeRedelivery,
            E::ExchangeSending { .. } => EventType::ExchangeSending,
            E::ExchangeSent { .. } => EventType::ExchangeSent,
        }
    }

    /// The exchange of an exchange event.
    pub fn exchange(&self) -> Option<&Exchange> {
        use CamelEvent as E;
        match self {
            E::ExchangeCreated(e) | E::ExchangeCompleted(e) | E::ExchangeFailed(e) => Some(e),
            E::ExchangeFailureHandling { exchange, .. }
            | E::ExchangeFailureHandled { exchange, .. }
            | E::ExchangeRedelivery { exchange, .. }
            | E::ExchangeSending { exchange, .. }
            | E::ExchangeSent { exchange, .. } => Some(exchange),
            _ => None,
        }
    }

    /// The route id of a route event.
    pub fn route_id(&self) -> Option<&str> {
        use CamelEvent as E;
        match self {
            E::RouteAdded(id)
            | E::RouteRemoved(id)
            | E::RouteStarting(id)
            | E::RouteStarted(id)
            | E::RouteStopping(id)
            | E::RouteStopped(id) => Some(id),
            _ => None,
        }
    }

    /// Whether it is an event of the context (Java `CamelContextEvent`).
    pub fn is_context_event(&self) -> bool {
        format!("{:?}", self.event_type()).starts_with("CamelContext")
    }

    /// Whether it is a route event (Java `RouteEvent`).
    pub fn is_route_event(&self) -> bool {
        self.route_id().is_some()
    }

    /// Whether it is an exchange event (Java `ExchangeEvent`).
    pub fn is_exchange_event(&self) -> bool {
        self.exchange().is_some()
    }
}

impl fmt::Display for CamelEvent<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CamelEvent as E;
        match self {
            E::RouteAdded(id) => write!(f, "Route added: {id}"),
            E::RouteRemoved(id) => write!(f, "Route removed: {id}"),
            E::RouteStarting(id) => write!(f, "Route starting: {id}"),
            E::RouteStarted(id) => write!(f, "Route started: {id}"),
            E::RouteStopping(id) => write!(f, "Route stopping: {id}"),
            E::RouteStopped(id) => write!(f, "Route stopped: {id}"),
            E::ExchangeCreated(e) => write!(f, "{} exchange created: {e}", e.exchange_id()),
            E::ExchangeCompleted(e) => write!(f, "{} exchange completed: {e}", e.exchange_id()),
            E::ExchangeFailed(e) => write!(f, "{} exchange failed: {e}", e.exchange_id()),
            E::ExchangeSending { exchange, endpoint } => {
                write!(
                    f,
                    "{} exchange {exchange} is being sent to: {endpoint}",
                    exchange.exchange_id()
                )
            }
            E::ExchangeSent {
                exchange,
                endpoint,
                time_taken,
            } => write!(
                f,
                "{} exchange {exchange} sent to: {endpoint} took: {} ms.",
                exchange.exchange_id(),
                time_taken.as_millis()
            ),
            other => write!(f, "{:?}", other.event_type()),
        }
    }
}

/// A notifier of the events (Java `EventNotifier`).
pub trait EventNotifier: Send + Sync {
    /// Receives the event (when enabled).
    fn notify(&self, event: &CamelEvent<'_>);

    /// Whether the notifier wants the event (default: all).
    fn is_enabled(&self, _event: &CamelEvent<'_>) -> bool {
        true
    }
}

/// A shared event notifier.
pub type SharedEventNotifier = Arc<dyn EventNotifier>;

/// Sends the event to the notifiers of the context that want it.
pub fn notify(context: &dyn CamelContext, event: CamelEvent<'_>) {
    for n in context.event_notifiers() {
        if n.is_enabled(&event) {
            n.notify(&event);
        }
    }
}

/// A notifier calling a function, with the ignore options of Java `EventNotifierSupport`.
pub struct EventNotifierSupport<F> {
    function: F,
    ignore_context_events: bool,
    ignore_context_init_events: bool,
    ignore_route_events: bool,
    ignore_exchange_events: bool,
    ignore_exchange_created: bool,
    ignore_exchange_completed: bool,
    ignore_exchange_failed: bool,
    ignore_exchange_redelivery: bool,
    ignore_exchange_sending: bool,
    ignore_exchange_sent: bool,
}

impl<F: Fn(&CamelEvent<'_>) + Send + Sync> EventNotifierSupport<F> {
    /// The notifier of the function (all the events enabled).
    pub fn new(function: F) -> Self {
        EventNotifierSupport {
            function,
            ignore_context_events: false,
            ignore_context_init_events: false,
            ignore_route_events: false,
            ignore_exchange_events: false,
            ignore_exchange_created: false,
            ignore_exchange_completed: false,
            ignore_exchange_failed: false,
            ignore_exchange_redelivery: false,
            ignore_exchange_sending: false,
            ignore_exchange_sent: false,
        }
    }

    /// Ignores the events of the context.
    pub fn ignore_camel_context_events(mut self, ignore: bool) -> Self {
        self.ignore_context_events = ignore;
        self
    }

    /// Ignores the initializing and initialized events of the context.
    pub fn ignore_camel_context_init_events(mut self, ignore: bool) -> Self {
        self.ignore_context_init_events = ignore;
        self
    }

    /// Ignores the route events.
    pub fn ignore_route_events(mut self, ignore: bool) -> Self {
        self.ignore_route_events = ignore;
        self
    }

    /// Ignores the exchange events.
    pub fn ignore_exchange_events(mut self, ignore: bool) -> Self {
        self.ignore_exchange_events = ignore;
        self
    }

    /// Ignores the exchange created events.
    pub fn ignore_exchange_created_event(mut self, ignore: bool) -> Self {
        self.ignore_exchange_created = ignore;
        self
    }

    /// Ignores the exchange completed events.
    pub fn ignore_exchange_completed_event(mut self, ignore: bool) -> Self {
        self.ignore_exchange_completed = ignore;
        self
    }

    /// Ignores the exchange failed and failure handling/handled events.
    pub fn ignore_exchange_failed_events(mut self, ignore: bool) -> Self {
        self.ignore_exchange_failed = ignore;
        self
    }

    /// Ignores the exchange redelivery events.
    pub fn ignore_exchange_redelivery_events(mut self, ignore: bool) -> Self {
        self.ignore_exchange_redelivery = ignore;
        self
    }

    /// Ignores the exchange sending events.
    pub fn ignore_exchange_sending_events(mut self, ignore: bool) -> Self {
        self.ignore_exchange_sending = ignore;
        self
    }

    /// Ignores the exchange sent events.
    pub fn ignore_exchange_sent_events(mut self, ignore: bool) -> Self {
        self.ignore_exchange_sent = ignore;
        self
    }
}

impl<F: Fn(&CamelEvent<'_>) + Send + Sync> EventNotifier for EventNotifierSupport<F> {
    fn notify(&self, event: &CamelEvent<'_>) {
        (self.function)(event)
    }

    fn is_enabled(&self, event: &CamelEvent<'_>) -> bool {
        use EventType as T;
        let t = event.event_type();
        if event.is_context_event() {
            if self.ignore_context_events {
                return false;
            }
            return !(self.ignore_context_init_events
                && matches!(t, T::CamelContextInitializing | T::CamelContextInitialized));
        }
        if event.is_route_event() {
            return !self.ignore_route_events;
        }
        if self.ignore_exchange_events {
            return false;
        }
        !match t {
            T::ExchangeCreated => self.ignore_exchange_created,
            T::ExchangeCompleted => self.ignore_exchange_completed,
            T::ExchangeFailed | T::ExchangeFailureHandling | T::ExchangeFailureHandled => {
                self.ignore_exchange_failed
            }
            T::ExchangeRedelivery => self.ignore_exchange_redelivery,
            T::ExchangeSending => self.ignore_exchange_sending,
            T::ExchangeSent => self.ignore_exchange_sent,
            _ => false,
        }
    }
}
