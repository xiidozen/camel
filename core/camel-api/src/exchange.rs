//! The message exchange. Port of `org.apache.camel.Exchange` and its default implementation
//! (`org.apache.camel.support.AbstractExchange` / `DefaultExchange`).

use crate::context::CamelContext;
use crate::convert::FromValue;
use crate::enums::ExchangePattern;
use crate::error::{CamelError, CamelResult, ExchangeRef};
use crate::exchange_constants::ExchangePropertyKey;
use crate::message::{header_matches, Message};
use crate::spi::synchronization::SharedSynchronization;
use camel_util::value::{Value, ValueMap};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// An exchange: the container of a message (and its reply) while it is routed.
///
/// The exchange holds the *in* message (the message being routed), an optional *out* message
/// (the reply, for the legacy in/out model), the error if processing failed, the exchange
/// properties (for the duration of the exchange), the variables and some routing state.
///
/// [`Exchange::message`] returns the current message: the out message if there is one,
/// otherwise the in message — this is the message processors should work with.
///
/// # Examples
///
/// ```
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
///
/// let mut exchange = Exchange::new(StandaloneContext::shared());
/// exchange.message_mut().set_body("Hello");
/// exchange.set_property("foo", 123);
/// assert_eq!(Some("Hello".to_string()), exchange.message().body_as::<String>().unwrap());
/// assert_eq!(Some(123), exchange.property_as::<i32>("foo").unwrap());
/// assert!(exchange.to_string().starts_with("Exchange["));
/// ```
#[derive(Clone)]
pub struct Exchange {
    context: Arc<dyn CamelContext>,
    exchange_id: String,
    created: i64,
    pattern: ExchangePattern,
    in_message: Message,
    out_message: Option<Message>,
    exception: Option<CamelError>,
    properties: ValueMap,
    internal_properties: HashMap<ExchangePropertyKey, Value>,
    variables: ValueMap,
    from_endpoint: Option<String>,
    from_route_id: Option<String>,
    route_stack: Vec<String>,
    from_route_group: Option<String>,
    route_stop: bool,
    rollback_only: bool,
    rollback_only_last: bool,
    transacted: bool,
    external_redelivered: bool,
    interrupted: bool,
    redelivery_exhausted: bool,
    failure_handled: bool,
    redelivery_counter: i32,
    redelivery_max_counter: i32,
    error_handler_handled: Option<bool>,
    stream_cache_disabled: bool,
    notify_event: bool,
    /// Whether a unit of work is active (a route created it).
    unit_of_work: bool,
    /// The in message when the unit of work started (Java `UnitOfWork.getOriginalInMessage`).
    original_message: Option<Box<Message>>,
    /// The callbacks run when the unit of work is done (Java `addOnCompletion`).
    on_completions: Vec<SharedSynchronization>,
}

impl fmt::Debug for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exchange")
            .field("exchange_id", &self.exchange_id)
            .field("pattern", &self.pattern)
            .field("in", &self.in_message)
            .field("out", &self.out_message)
            .field("exception", &self.exception)
            .field("properties", &self.properties)
            .finish()
    }
}

impl fmt::Display for Exchange {
    /// `Exchange[ID]` (the content is not shown, as it may contain sensitive information).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Exchange[{}]", self.exchange_id)
    }
}

impl ExchangeRef for Exchange {
    fn exchange_id(&self) -> String {
        self.exchange_id.clone()
    }

    fn display(&self) -> String {
        self.to_string()
    }
}

impl Exchange {
    /// Creates an `InOnly` exchange for the context.
    pub fn new(context: Arc<dyn CamelContext>) -> Self {
        Self::with_pattern(context, ExchangePattern::InOnly)
    }

    /// Creates an exchange with the pattern.
    pub fn with_pattern(context: Arc<dyn CamelContext>, pattern: ExchangePattern) -> Self {
        let exchange_id = context.uuid_generator().generate_exchange_uuid();
        Exchange {
            context,
            exchange_id,
            created: now_millis(),
            pattern,
            in_message: Message::new(),
            out_message: None,
            exception: None,
            properties: ValueMap::new(),
            internal_properties: HashMap::new(),
            variables: ValueMap::new(),
            from_endpoint: None,
            from_route_id: None,
            route_stack: Vec::new(),
            from_route_group: None,
            route_stop: false,
            rollback_only: false,
            rollback_only_last: false,
            transacted: false,
            external_redelivered: false,
            interrupted: false,
            redelivery_exhausted: false,
            failure_handled: false,
            redelivery_counter: 0,
            redelivery_max_counter: 0,
            error_handler_handled: None,
            stream_cache_disabled: false,
            notify_event: false,
            unit_of_work: false,
            original_message: None,
            on_completions: Vec::new(),
        }
    }

    /// Creates an exchange with the body.
    pub fn with_body(context: Arc<dyn CamelContext>, body: impl Into<Value>) -> Self {
        let mut e = Self::new(context);
        e.in_message.set_body(body);
        e
    }

    /// Creates a copy of the exchange, so it can be forwarded to another destination. The copy
    /// has its own exchange id, and copies the messages, error, properties, variables and
    /// routing state; not the unit of work and its on completions (Java: a copy has no unit
    /// of work, see [`handover_completions`](Exchange::handover_completions)).
    pub fn copy(&self) -> Exchange {
        let mut copy = self.clone();
        copy.exchange_id = self.context.uuid_generator().generate_exchange_uuid();
        copy.unit_of_work = false;
        copy.original_message = None;
        copy.on_completions = Vec::new();
        copy
    }

    /// Whether a unit of work is active: the exchange is routed by a route, which runs the
    /// on completions when it is done with the exchange.
    pub fn has_unit_of_work(&self) -> bool {
        self.unit_of_work
    }

    /// Starts the unit of work, remembering the in message as the original message (done by
    /// the first route the exchange enters).
    pub fn begin_unit_of_work(&mut self) {
        self.unit_of_work = true;
        self.original_message = Some(Box::new(self.in_message.clone()));
    }

    /// Takes the on completions to run when the unit of work is done (see
    /// [`done_synchronizations`](crate::spi::synchronization::done_synchronizations)); the
    /// unit of work stays active, so they still see the
    /// [`original_message`](Exchange::original_message).
    pub fn take_on_completions(&mut self) -> Vec<SharedSynchronization> {
        std::mem::take(&mut self.on_completions)
    }

    /// Ends the unit of work, once its on completions ran (Java `UnitOfWork.done`).
    pub fn end_unit_of_work(&mut self) {
        self.unit_of_work = false;
        self.original_message = None;
    }

    /// The in message as it was when the unit of work started (Java
    /// `ExchangeHelper.getOriginalInMessage`), if any.
    pub fn original_message(&self) -> Option<&Message> {
        self.original_message.as_deref()
    }

    /// Adds a callback run when the unit of work is done (Java `addOnCompletion`).
    pub fn add_on_completion(&mut self, synchronization: SharedSynchronization) {
        self.on_completions.push(synchronization);
    }

    /// Whether the callback (this very instance) was added (Java `containsOnCompletion`).
    pub fn contains_on_completion(&self, synchronization: &SharedSynchronization) -> bool {
        self.on_completions
            .iter()
            .any(|s| Arc::ptr_eq(s, synchronization))
    }

    /// The callbacks run when the unit of work is done.
    pub fn on_completions(&self) -> &[SharedSynchronization] {
        &self.on_completions
    }

    /// Hands the on completions that allow it over to the target exchange (Java
    /// `UnitOfWork.handoverSynchronization`), for example when the target continues the
    /// routing asynchronously.
    pub fn handover_completions(&mut self, target: &mut Exchange) {
        let all = std::mem::take(&mut self.on_completions);
        for s in all {
            if s.allow_handover() {
                s.before_handover(target);
                target.on_completions.push(s);
            } else {
                self.on_completions.push(s);
            }
        }
    }

    /// The CamelContext.
    pub fn context(&self) -> &Arc<dyn CamelContext> {
        &self.context
    }

    /// The exchange id.
    pub fn exchange_id(&self) -> &str {
        &self.exchange_id
    }

    /// Sets the exchange id.
    pub fn set_exchange_id(&mut self, id: impl Into<String>) {
        self.exchange_id = id.into();
    }

    /// The time (millis since epoch) the exchange was created.
    pub fn created(&self) -> i64 {
        self.created
    }

    /// The exchange pattern.
    pub fn pattern(&self) -> ExchangePattern {
        self.pattern
    }

    /// Sets the exchange pattern.
    pub fn set_pattern(&mut self, pattern: ExchangePattern) {
        self.pattern = pattern;
    }

    // -------------------------------------------------------------------------------------
    // messages

    /// The in message.
    pub fn in_message(&self) -> &Message {
        &self.in_message
    }

    /// The mutable in message.
    pub fn in_message_mut(&mut self) -> &mut Message {
        &mut self.in_message
    }

    /// Replaces the in message.
    pub fn set_in(&mut self, message: Message) {
        self.in_message = message;
    }

    /// Whether there is an out message.
    pub fn has_out(&self) -> bool {
        self.out_message.is_some()
    }

    /// The out message, created lazily (as an empty message) when first accessed.
    pub fn out_message_mut(&mut self) -> &mut Message {
        self.out_message.get_or_insert_with(Message::new)
    }

    /// The out message, if any.
    pub fn out_message(&self) -> Option<&Message> {
        self.out_message.as_ref()
    }

    /// Sets the out message.
    pub fn set_out(&mut self, message: Option<Message>) {
        self.out_message = message;
    }

    /// The current message: the out message if there is one, otherwise the in message.
    pub fn message(&self) -> &Message {
        self.out_message.as_ref().unwrap_or(&self.in_message)
    }

    /// The current message (mutable).
    pub fn message_mut(&mut self) -> &mut Message {
        match &mut self.out_message {
            Some(out) => out,
            None => &mut self.in_message,
        }
    }

    /// Replaces the current message (the out message if there is one, otherwise the in
    /// message).
    pub fn set_message(&mut self, message: Message) {
        match &mut self.out_message {
            Some(out) => *out = message,
            None => self.in_message = message,
        }
    }

    // -------------------------------------------------------------------------------------
    // error

    /// The error, if the exchange failed.
    pub fn exception(&self) -> Option<&CamelError> {
        self.exception.as_ref()
    }

    /// Sets (or clears) the error.
    pub fn set_exception(&mut self, error: Option<CamelError>) {
        self.exception = error;
    }

    /// Takes the error, clearing it.
    pub fn take_exception(&mut self) -> Option<CamelError> {
        self.exception.take()
    }

    /// Whether the exchange failed (has an error).
    pub fn is_failed(&self) -> bool {
        self.exception.is_some()
    }

    // -------------------------------------------------------------------------------------
    // properties

    /// The internal property.
    pub fn internal_property(&self, key: ExchangePropertyKey) -> Option<&Value> {
        self.internal_properties.get(&key)
    }

    /// Sets the internal property (`Null` removes it).
    pub fn set_internal_property(&mut self, key: ExchangePropertyKey, value: impl Into<Value>) {
        let value = value.into();
        if value.is_null() {
            self.internal_properties.remove(&key);
        } else {
            self.internal_properties.insert(key, value);
        }
    }

    /// Removes the internal property.
    pub fn remove_internal_property(&mut self, key: ExchangePropertyKey) -> Option<Value> {
        self.internal_properties.remove(&key)
    }

    /// The property: a well-known internal property (see [`ExchangePropertyKey`]) or a custom
    /// one.
    pub fn property(&self, name: &str) -> Option<&Value> {
        if let Some(key) = ExchangePropertyKey::as_exchange_property_key(name) {
            if let Some(v) = self.internal_properties.get(&key) {
                return Some(v);
            }
        }
        self.properties.get(name)
    }

    /// The property converted to the type.
    ///
    /// # Errors
    ///
    /// Returns an error if the property cannot be converted.
    pub fn property_as<T: FromValue>(&self, name: &str) -> CamelResult<Option<T>> {
        match self.property(name) {
            None => Ok(None),
            Some(v) => T::from_value(v),
        }
    }

    /// Sets the property (`Null` removes it). Well-known names are stored as internal
    /// properties.
    pub fn set_property(&mut self, name: &str, value: impl Into<Value>) {
        let value = value.into();
        if let Some(key) = ExchangePropertyKey::as_exchange_property_key(name) {
            self.set_internal_property(key, value);
        } else if value.is_null() {
            self.properties.shift_remove(name);
        } else {
            self.properties.insert(name.to_string(), value);
        }
    }

    /// Removes the property.
    pub fn remove_property(&mut self, name: &str) -> Option<Value> {
        if let Some(key) = ExchangePropertyKey::as_exchange_property_key(name) {
            if let Some(v) = self.internal_properties.remove(&key) {
                return Some(v);
            }
        }
        self.properties.shift_remove(name)
    }

    /// Removes the custom properties matching the pattern (except those matching any exclude
    /// pattern); see [`header_matches`] for the pattern syntax.
    ///
    /// # Returns
    ///
    /// `true` if any property was removed.
    pub fn remove_properties(&mut self, pattern: &str, exclude_patterns: &[&str]) -> bool {
        let before = self.properties.len() + self.internal_properties.len();
        let keep = |k: &str| {
            let matched = header_matches(pattern, k);
            !matched || exclude_patterns.iter().any(|p| header_matches(p, k))
        };
        self.properties.retain(|k, _| keep(k));
        self.internal_properties.retain(|k, _| keep(k.name()));
        before != self.properties.len() + self.internal_properties.len()
    }

    /// The custom (non internal) properties.
    pub fn properties(&self) -> &ValueMap {
        &self.properties
    }

    /// The custom properties (mutable).
    pub fn properties_mut(&mut self) -> &mut ValueMap {
        &mut self.properties
    }

    /// All the properties: the internal ones (by name) and the custom ones.
    pub fn all_properties(&self) -> ValueMap {
        let mut all = ValueMap::new();
        for key in ExchangePropertyKey::VALUES {
            if let Some(v) = self.internal_properties.get(key) {
                all.insert(key.name().to_string(), v.clone());
            }
        }
        for (k, v) in &self.properties {
            all.insert(k.clone(), v.clone());
        }
        all
    }

    /// Whether the exchange has any custom properties.
    pub fn has_properties(&self) -> bool {
        !self.properties.is_empty()
    }

    // -------------------------------------------------------------------------------------
    // variables

    /// The variable.
    pub fn variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// The variable converted to the type.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable cannot be converted.
    pub fn variable_as<T: FromValue>(&self, name: &str) -> CamelResult<Option<T>> {
        match self.variables.get(name) {
            None => Ok(None),
            Some(v) => T::from_value(v),
        }
    }

    /// Sets the variable (`Null` removes it).
    pub fn set_variable(&mut self, name: &str, value: impl Into<Value>) {
        let value = value.into();
        if value.is_null() {
            self.variables.shift_remove(name);
        } else {
            self.variables.insert(name.to_string(), value);
        }
    }

    /// Removes the variable (`*` removes all variables).
    pub fn remove_variable(&mut self, name: &str) -> Option<Value> {
        if name == "*" {
            self.variables.clear();
            return None;
        }
        self.variables.shift_remove(name)
    }

    /// The variables.
    pub fn variables(&self) -> &ValueMap {
        &self.variables
    }

    /// Whether the exchange has any variables.
    pub fn has_variables(&self) -> bool {
        !self.variables.is_empty()
    }

    // -------------------------------------------------------------------------------------
    // routing state

    /// The uri of the endpoint the exchange was created by (the route consumer), if any.
    pub fn from_endpoint(&self) -> Option<&str> {
        self.from_endpoint.as_deref()
    }

    /// Sets the uri of the endpoint the exchange was created by.
    pub fn set_from_endpoint(&mut self, uri: impl Into<String>) {
        self.from_endpoint = Some(uri.into());
    }

    /// The id of the route the exchange was created by, if any.
    pub fn from_route_id(&self) -> Option<&str> {
        self.from_route_id.as_deref()
    }

    /// Sets the id of the route the exchange was created by.
    pub fn set_from_route_id(&mut self, route_id: impl Into<String>) {
        self.from_route_id = Some(route_id.into());
    }

    /// The id of the route the exchange is in (Java `ExchangeHelper.getRouteId`: the route of
    /// the unit of work), else the route it was created by.
    pub fn route_id(&self) -> Option<&str> {
        self.route_stack
            .last()
            .map(String::as_str)
            .or(self.from_route_id.as_deref())
    }

    /// The exchange enters the route (Java `UnitOfWork.pushRoute`).
    pub fn push_route(&mut self, route_id: impl Into<String>) {
        self.route_stack.push(route_id.into());
    }

    /// The exchange leaves the current route (Java `UnitOfWork.popRoute`).
    pub fn pop_route(&mut self) {
        self.route_stack.pop();
    }

    /// Clears the id of the route the exchange was created by, so the next route that routes
    /// it sets its own (used by consumers that take over an exchange created elsewhere).
    pub fn clear_from_route_id(&mut self) {
        self.from_route_id = None;
    }

    /// The group of the route the exchange was created by, if any.
    pub fn from_route_group(&self) -> Option<&str> {
        self.from_route_group.as_deref()
    }

    /// Sets the group of the route the exchange was created by.
    pub fn set_from_route_group(&mut self, group: impl Into<String>) {
        self.from_route_group = Some(group.into());
    }

    /// Whether routing of the exchange should stop (the Stop EIP).
    pub fn is_route_stop(&self) -> bool {
        self.route_stop
    }

    /// Sets whether routing of the exchange should stop.
    pub fn set_route_stop(&mut self, route_stop: bool) {
        self.route_stop = route_stop;
    }

    /// Whether the exchange is marked for rollback only.
    pub fn is_rollback_only(&self) -> bool {
        self.rollback_only
    }

    /// Marks the exchange for rollback only.
    pub fn set_rollback_only(&mut self, rollback_only: bool) {
        self.rollback_only = rollback_only;
    }

    /// Whether the exchange is marked for rollback only of the last transaction.
    pub fn is_rollback_only_last(&self) -> bool {
        self.rollback_only_last
    }

    /// Marks the exchange for rollback only of the last transaction.
    pub fn set_rollback_only_last(&mut self, rollback_only_last: bool) {
        self.rollback_only_last = rollback_only_last;
    }

    /// Whether the exchange is transacted.
    pub fn is_transacted(&self) -> bool {
        self.transacted
    }

    /// Sets whether the exchange is transacted.
    pub fn set_transacted(&mut self, transacted: bool) {
        self.transacted = transacted;
    }

    /// Whether the exchange is redelivered by an external system (such as a JMS broker).
    pub fn is_external_redelivered(&self) -> bool {
        self.external_redelivered
    }

    /// Sets whether the exchange is redelivered by an external system.
    pub fn set_external_redelivered(&mut self, redelivered: bool) {
        self.external_redelivered = redelivered;
    }

    /// Whether the exchange was interrupted (the routing engine was asked to stop it).
    pub fn is_interrupted(&self) -> bool {
        self.interrupted
    }

    /// Sets whether the exchange was interrupted.
    pub fn set_interrupted(&mut self, interrupted: bool) {
        self.interrupted = interrupted;
    }

    /// Whether the error handler has exhausted all redelivery attempts.
    pub fn is_redelivery_exhausted(&self) -> bool {
        self.redelivery_exhausted
    }

    /// Sets whether the redelivery attempts are exhausted.
    pub fn set_redelivery_exhausted(&mut self, exhausted: bool) {
        self.redelivery_exhausted = exhausted;
    }

    /// Whether the failure was handled by a failure processor (Java
    /// `ExchangeExtension.isFailureHandled`).
    pub fn is_failure_handled(&self) -> bool {
        self.failure_handled
    }

    /// Sets whether the failure was handled by a failure processor.
    pub fn set_failure_handled(&mut self, handled: bool) {
        self.failure_handled = handled;
    }

    /// The redelivery attempt (0 when not redelivered, -1 after the error handler finished).
    pub fn redelivery_counter(&self) -> i32 {
        self.redelivery_counter
    }

    /// Sets the redelivery attempt.
    pub fn set_redelivery_counter(&mut self, counter: i32) {
        self.redelivery_counter = counter;
    }

    /// The maximum number of redelivery attempts (0 when unknown).
    pub fn redelivery_max_counter(&self) -> i32 {
        self.redelivery_max_counter
    }

    /// Sets the maximum number of redelivery attempts.
    pub fn set_redelivery_max_counter(&mut self, counter: i32) {
        self.redelivery_max_counter = counter;
    }

    /// Whether an error handler handled the error (`None` if no error handler was involved).
    pub fn error_handler_handled(&self) -> Option<bool> {
        self.error_handler_handled
    }

    /// Sets whether an error handler handled the error.
    pub fn set_error_handler_handled(&mut self, handled: Option<bool>) {
        self.error_handler_handled = handled;
    }

    /// Whether stream caching is disabled for this exchange.
    pub fn is_stream_cache_disabled(&self) -> bool {
        self.stream_cache_disabled
    }

    /// Sets whether stream caching is disabled for this exchange.
    pub fn set_stream_cache_disabled(&mut self, disabled: bool) {
        self.stream_cache_disabled = disabled;
    }

    /// Whether events are being notified for the exchange (to avoid notifying recursively).
    pub fn is_notify_event(&self) -> bool {
        self.notify_event
    }

    /// Sets whether events are being notified for the exchange.
    pub fn set_notify_event(&mut self, notify_event: bool) {
        self.notify_event = notify_event;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StandaloneContext;
    use crate::exchange_constants::{CHARSET_NAME, EXCEPTION_CAUGHT};

    fn exchange() -> Exchange {
        Exchange::new(StandaloneContext::shared())
    }

    #[test]
    fn messages() {
        let mut e = exchange();
        e.in_message_mut().set_body("in");
        assert!(!e.has_out());
        assert_eq!(&Value::from("in"), e.message().body());
        e.out_message_mut().set_body("out");
        assert!(e.has_out());
        assert_eq!(&Value::from("out"), e.message().body());
        e.message_mut().set_header("x", 1);
        assert_eq!(Some(&Value::from(1)), e.out_message().unwrap().header("x"));
        e.set_message(Message::with_body("replaced"));
        assert_eq!(&Value::from("replaced"), e.out_message().unwrap().body());
        assert_eq!(&Value::from("in"), e.in_message().body());
        e.set_out(None);
        e.set_message(Message::with_body("in2"));
        assert_eq!(&Value::from("in2"), e.in_message().body());
        e.set_in(Message::with_body("in3"));
        assert_eq!(&Value::from("in3"), e.message().body());
    }

    #[test]
    fn properties() {
        let mut e = exchange();
        e.set_property("foo", "bar");
        e.set_property(CHARSET_NAME, "UTF-8");
        assert_eq!(
            Some(&Value::from("UTF-8")),
            e.internal_property(ExchangePropertyKey::CharsetName)
        );
        assert_eq!(Some(&Value::from("UTF-8")), e.property(CHARSET_NAME));
        assert_eq!(1, e.properties().len());
        assert_eq!(2, e.all_properties().len());
        assert!(e.has_properties());
        e.set_property("foo", Value::Null);
        assert!(e.property("foo").is_none());
        e.set_property("a1", 1);
        e.set_property("a2", 2);
        e.set_property("b", 3);
        assert!(e.remove_properties("a*", &["a2"]));
        assert!(e.property("a1").is_none());
        assert!(e.property("a2").is_some());
        assert_eq!(Some(Value::from(3)), e.remove_property("b"));
        assert_eq!(Some(Value::from("UTF-8")), e.remove_property(CHARSET_NAME));
        e.set_internal_property(ExchangePropertyKey::ExceptionCaught, "x");
        assert_eq!(Some(&Value::from("x")), e.property(EXCEPTION_CAUGHT));
        assert!(e.remove_properties("Camel*", &[]));
        assert!(e
            .internal_property(ExchangePropertyKey::ExceptionCaught)
            .is_none());
        assert_eq!(Some(2), e.property_as::<i32>("a2").unwrap());
        e.properties_mut().clear();
        assert!(!e.has_properties());
        e.set_internal_property(ExchangePropertyKey::BatchSize, 4);
        assert_eq!(
            Some(Value::from(4)),
            e.remove_internal_property(ExchangePropertyKey::BatchSize)
        );
    }

    #[test]
    fn variables_and_state() {
        let mut e = exchange();
        e.set_variable("v", "1");
        assert_eq!(Some(1), e.variable_as::<i32>("v").unwrap());
        assert!(e.has_variables());
        e.set_variable("w", 2);
        assert_eq!(Some(Value::from(2)), e.remove_variable("w"));
        e.remove_variable("*");
        assert!(!e.has_variables());
        e.set_variable("x", 1);
        e.set_variable("x", Value::Null);
        assert!(e.variable("x").is_none());

        e.set_route_stop(true);
        e.set_rollback_only(true);
        e.set_rollback_only_last(true);
        e.set_transacted(true);
        e.set_external_redelivered(true);
        e.set_interrupted(true);
        e.set_redelivery_exhausted(true);
        e.set_error_handler_handled(Some(true));
        e.set_stream_cache_disabled(true);
        e.set_notify_event(true);
        e.set_from_endpoint("direct://start");
        e.set_from_route_id("route1");
        e.set_from_route_group("g");
        assert!(
            e.is_route_stop()
                && e.is_rollback_only()
                && e.is_rollback_only_last()
                && e.is_transacted()
        );
        assert!(e.is_external_redelivered() && e.is_interrupted() && e.is_redelivery_exhausted());
        assert!(e.is_stream_cache_disabled() && e.is_notify_event());
        assert_eq!(Some(true), e.error_handler_handled());
        assert_eq!(Some("direct://start"), e.from_endpoint());
        assert_eq!(Some("route1"), e.from_route_id());
        assert_eq!(Some("g"), e.from_route_group());
        e.set_pattern(ExchangePattern::InOut);
        assert_eq!(ExchangePattern::InOut, e.pattern());
        assert!(e.created() > 0);
    }

    #[test]
    fn errors_and_copy() {
        let mut e = Exchange::with_body(StandaloneContext::shared(), "body");
        e.set_exchange_id("ID-1");
        assert_eq!("Exchange[ID-1]", e.to_string());
        assert!(!e.is_failed());
        e.set_exception(Some(CamelError::rollback_exchange(None, &e)));
        assert!(e.is_failed());
        assert_eq!(
            "Intended rollback. Exchange[ID-1]",
            e.exception().unwrap().to_string()
        );
        let copy = e.copy();
        assert_ne!(e.exchange_id(), copy.exchange_id());
        assert!(copy.is_failed());
        assert_eq!(&Value::from("body"), copy.message().body());
        assert!(e.take_exception().is_some());
        assert!(!e.is_failed());
        assert_eq!("camel-standalone", e.context().name());
        assert!(format!("{e:?}").contains("ID-1"));
    }
}
