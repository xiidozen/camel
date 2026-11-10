//! The mock endpoint (Java `org.apache.camel.component.mock.MockEndpoint`).

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::convert::FromValue;
use camel_api::endpoint::{Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer};
use camel_api::enums::ExchangePattern;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{
    BoxFuture, Processor, SharedExpression, SharedPredicate, SharedProcessor,
};
use camel_api::service::Service;
use camel_util::value::Value;
use std::any::Any;
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

/// A failed mock expectation (Java `AssertionError`). The message starts with the endpoint uri,
/// such as `mock://result Received message count. Expected: <1> but was: <0>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockAssertionError {
    message: String,
}

impl MockAssertionError {
    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for MockAssertionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MockAssertionError {}

type TaskResult = Result<(), String>;
type Task = Arc<dyn Fn(&State) -> TaskResult + Send + Sync>;

/// The state of a mock endpoint (Java fields of `MockEndpoint`).
struct State {
    expected_count: i32,
    expected_minimum_count: i32,
    latch: Option<usize>,
    counter: usize,
    received: Vec<Exchange>,
    expected_body_values: Option<Vec<Value>>,
    actual_body_values: Vec<Value>,
    expected_header_values: Option<Vec<(String, Value)>>,
    expected_property_values: Option<Vec<(String, Value)>>,
    expected_variable_values: Option<Vec<(String, Value)>>,
    tests: Vec<Task>,
    failures: Vec<String>,
    processors: HashMap<usize, SharedProcessor>,
    default_processor: Option<SharedProcessor>,
    reporter: Option<SharedProcessor>,
    result_wait_time: u64,
    result_minimum_wait_time: u64,
    assert_period: u64,
    sleep_for_empty_test: u64,
    retain_first: i32,
    retain_last: i32,
    copy_on_exchange: bool,
    log: bool,
}

impl State {
    fn new() -> Self {
        State {
            expected_count: -1,
            expected_minimum_count: -1,
            latch: None,
            counter: 0,
            received: Vec::new(),
            expected_body_values: None,
            actual_body_values: Vec::new(),
            expected_header_values: None,
            expected_property_values: None,
            expected_variable_values: None,
            tests: Vec::new(),
            failures: Vec::new(),
            processors: HashMap::new(),
            default_processor: None,
            reporter: None,
            result_wait_time: 0,
            result_minimum_wait_time: 0,
            assert_period: 0,
            sleep_for_empty_test: 0,
            retain_first: -1,
            retain_last: -1,
            copy_on_exchange: true,
            log: false,
        }
    }

    fn received_exchange(&self, index: usize) -> Result<&Exchange, String> {
        self.received
            .get(index)
            .ok_or_else(|| format!("There is no exchange at index {index}"))
    }

    fn add_received(&mut self, copy: Exchange) {
        if self.retain_first == 0 && self.retain_last == 0 {
            return;
        }
        let retain_all = self.retain_first < 0 && self.retain_last < 0;
        if retain_all || (self.retain_first > 0 && (self.counter as i32) < self.retain_first) {
            self.received.push(copy);
        } else if self.retain_last > 0 {
            let index = self.received.len() as i32 - self.retain_last;
            if index >= 0 && (self.retain_first <= 0 || self.retain_first <= index) {
                self.received.remove(index as usize);
            }
            self.received.push(copy);
        }
    }
}

/// Converts the actual value to the type of the expected value, as Java converts with the type
/// converter before comparing (so `123` equals `"123"` when a text is expected).
fn convert_like(expected: &Value, actual: &Value) -> Option<Value> {
    fn conv<T: FromValue>(v: &Value) -> Option<T> {
        T::from_value(v).ok().flatten()
    }
    if actual.is_null() {
        return Some(Value::Null);
    }
    match expected {
        Value::Text(_) => conv::<String>(actual).map(Value::Text),
        Value::Int(_) => conv::<i64>(actual).map(Value::Int),
        Value::Float(_) => conv::<f64>(actual).map(Value::Float),
        Value::Bool(_) => conv::<bool>(actual).map(Value::Bool),
        Value::Bytes(_) => conv::<Vec<u8>>(actual).map(Value::Bytes),
        _ => Some(actual.clone()),
    }
}

/// Java `extractActualValue` + `assertEquals`.
fn assert_value_equals(message: &str, expected: &Value, actual: &Value) -> TaskResult {
    let actual = if expected.is_null() || actual.is_null() {
        actual.clone()
    } else {
        convert_like(expected, actual).ok_or_else(|| {
            format!(
                "There is no type conversion possible from {} to {}",
                actual.type_name(),
                expected.type_name()
            )
        })?
    };
    if *expected != actual {
        return Err(format!(
            "{message}. Expected: <{expected}> but was: <{actual}>"
        ));
    }
    Ok(())
}

/// Compares two values like Java `ObjectHelper.compare` (numbers numerically, otherwise by
/// text).
pub(crate) fn compare_values(a: &Value, b: &Value) -> CmpOrdering {
    match (a, b) {
        (Value::Null, Value::Null) => CmpOrdering::Equal,
        (Value::Null, _) => CmpOrdering::Less,
        (_, Value::Null) => CmpOrdering::Greater,
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => a
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&b.as_f64().unwrap_or(0.0))
            .unwrap_or(CmpOrdering::Equal),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => a.to_string().cmp(&b.to_string()),
    }
}

struct Inner {
    uri: String,
    name: String,
    context: Weak<dyn CamelContext>,
    state: Mutex<State>,
    changed: Notify,
}

/// A mock endpoint. Cloning gives another handle to the same endpoint.
///
/// Set the expectations, run the test, then call [`assert_is_satisfied`](Self::assert_is_satisfied)
/// (it panics with the Java assertion message on failure; use
/// [`try_assert_is_satisfied`](Self::try_assert_is_satisfied) to get the error instead).
#[derive(Clone)]
pub struct MockEndpoint {
    inner: Arc<Inner>,
}

impl fmt::Debug for MockEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MockEndpoint[{}]", self.inner.uri)
    }
}

impl MockEndpoint {
    /// Creates a mock endpoint (normally done by the [`MockComponent`](crate::MockComponent)).
    pub fn new(uri: &str, name: &str, context: Weak<dyn CamelContext>) -> Self {
        MockEndpoint {
            inner: Arc::new(Inner {
                uri: uri.to_string(),
                name: name.to_string(),
                context,
                state: Mutex::new(State::new()),
                changed: Notify::new(),
            }),
        }
    }

    /// Resolves the mock endpoint for the uri in the context (Java `MockEndpoint.resolve`).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be resolved or is not a mock endpoint.
    pub fn resolve(context: &dyn CamelContext, uri: &str) -> CamelResult<MockEndpoint> {
        let e = context.endpoint(uri)?;
        Self::from_endpoint(&e).ok_or_else(|| {
            CamelError::illegal_argument(format!(
                "The endpoint is not a mock endpoint: {}",
                e.endpoint_uri()
            ))
        })
    }

    /// The mock endpoint handle, if the endpoint is a mock endpoint.
    pub fn from_endpoint(endpoint: &SharedEndpoint) -> Option<MockEndpoint> {
        endpoint.as_any().downcast_ref::<MockEndpoint>().cloned()
    }

    /// Asserts that all the mock endpoints among the endpoints are satisfied (Java
    /// `MockEndpoint.assertIsSatisfied(CamelContext)`, passing `context.endpoints()`).
    ///
    /// # Panics
    ///
    /// Panics with the first failed expectation.
    pub async fn assert_is_satisfied_all(endpoints: impl IntoIterator<Item = SharedEndpoint>) {
        for e in endpoints {
            if let Some(m) = Self::from_endpoint(&e) {
                m.assert_is_satisfied().await;
            }
        }
    }

    /// Resets all the mock endpoints among the endpoints (Java `MockEndpoint.resetMocks`).
    pub fn reset_mocks(endpoints: impl IntoIterator<Item = SharedEndpoint>) {
        for e in endpoints {
            if let Some(m) = Self::from_endpoint(&e) {
                m.reset();
            }
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap()
    }

    /// The name of the endpoint (the part after `mock:`).
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Resets the endpoint: forgets the received exchanges and the expectations.
    pub fn reset(&self) {
        *self.state() = State::new();
        self.inner.changed.notify_waiters();
    }

    // ---------------------------------------------------------------------------------------
    // expectations
    // ---------------------------------------------------------------------------------------

    /// Expects the number of messages (Java `expectedMessageCount`); 0 expects no message
    /// (see [`set_sleep_for_empty_test`](Self::set_sleep_for_empty_test)).
    pub fn expected_message_count(&self, count: i32) {
        let mut s = self.state();
        s.expected_count = count;
        s.latch = if count <= 0 {
            None
        } else {
            Some(count as usize)
        };
    }

    /// Expects at least the number of messages.
    pub fn expected_minimum_message_count(&self, count: i32) {
        let mut s = self.state();
        s.expected_minimum_count = count;
        s.latch = if count <= 0 {
            None
        } else {
            Some(count as usize)
        };
    }

    /// The expected message count (-1 if not set).
    pub fn expected_count(&self) -> i32 {
        self.state().expected_count
    }

    /// The expected minimum message count (-1 if not set).
    pub fn expected_minimum_count(&self) -> i32 {
        self.state().expected_minimum_count
    }

    /// Adds a custom expectation, run when asserting (Java `expects(Runnable)`): it receives
    /// the received exchanges and returns the failure message.
    pub fn expects<F>(&self, test: F)
    where
        F: Fn(&[Exchange]) -> Result<(), String> + Send + Sync + 'static,
    {
        self.state()
            .tests
            .push(Arc::new(move |s: &State| test(&s.received)));
    }

    fn expects_task(&self, task: Task) {
        self.state().tests.push(task);
    }

    /// Expects the bodies, in this order (this also sets the expected message count).
    pub fn expected_bodies_received<I, V>(&self, bodies: I)
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        let bodies: Vec<Value> = bodies.into_iter().map(Into::into).collect();
        self.expected_message_count(bodies.len() as i32);
        {
            let mut s = self.state();
            s.expected_body_values = Some(bodies);
            s.actual_body_values = Vec::new();
        }
        self.expects_task(Arc::new(|s: &State| {
            let expected = s.expected_body_values.clone().unwrap_or_default();
            for (i, expected_body) in expected.iter().enumerate() {
                s.received_exchange(i)?;
                let actual = s.actual_body_values.get(i).cloned().unwrap_or(Value::Null);
                assert_value_equals(&format!("Body of message: {i}"), expected_body, &actual)?;
            }
            Ok(())
        }));
    }

    /// Expects the bodies in any order (this also sets the expected message count).
    pub fn expected_bodies_received_in_any_order<I, V>(&self, bodies: I)
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        let bodies: Vec<Value> = bodies.into_iter().map(Into::into).collect();
        self.expected_message_count(bodies.len() as i32);
        {
            let mut s = self.state();
            s.expected_body_values = Some(bodies);
            s.actual_body_values = Vec::new();
        }
        self.expects_task(Arc::new(|s: &State| {
            let mut actual = s.actual_body_values.clone();
            for (i, expected_body) in s
                .expected_body_values
                .clone()
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                s.received_exchange(i)?;
                match actual.iter().position(|a| a == expected_body) {
                    Some(p) => {
                        actual.remove(p);
                    }
                    None => {
                        let found: Vec<String> = actual.iter().map(|v| v.to_string()).collect();
                        return Err(format!(
                            "Message with body {expected_body} was expected but not found in [{}]",
                            found.join(", ")
                        ));
                    }
                }
            }
            Ok(())
        }));
    }

    fn ensure_minimum_one(&self) {
        let (min, count) = {
            let s = self.state();
            (s.expected_minimum_count, s.expected_count)
        };
        if min == -1 && count <= 0 {
            self.expected_minimum_message_count(1);
        }
    }

    /// Expects the header on the received messages. A `Null` value also accepts an absent
    /// header.
    pub fn expected_header_received(&self, name: &str, value: impl Into<Value>) {
        self.ensure_minimum_one();
        let first = {
            let mut s = self.state();
            let first = s.expected_header_values.is_none();
            let values = s.expected_header_values.get_or_insert_with(Vec::new);
            values.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
            values.push((name.to_string(), value.into()));
            first
        };
        if first {
            self.expects_task(Arc::new(|s: &State| {
                for i in 0..s.received.len() {
                    let exchange = s.received_exchange(i)?;
                    for (key, expected) in s.expected_header_values.clone().unwrap_or_default() {
                        let headers = exchange.in_message().headers();
                        if !expected.is_null() {
                            if headers.is_empty() {
                                return Err(format!("Exchange {i} has no headers"));
                            }
                            if exchange.in_message().header(&key).is_none() {
                                return Err(format!(
                                    "No header with name {key} found for message: {i}"
                                ));
                            }
                        }
                        let actual = exchange
                            .in_message()
                            .header(&key)
                            .cloned()
                            .unwrap_or(Value::Null);
                        assert_value_equals(
                            &format!("Header with name {key} for message: {i}"),
                            &expected,
                            &actual,
                        )?;
                    }
                }
                Ok(())
            }));
        }
    }

    /// Expects that the received messages have no headers.
    pub fn expected_no_header_received(&self) {
        self.ensure_minimum_one();
        self.expects_task(Arc::new(|s: &State| {
            for (i, e) in s.received.iter().enumerate() {
                if !e.in_message().headers().is_empty() {
                    return Err(format!("Exchange {i} has headers"));
                }
            }
            Ok(())
        }));
    }

    /// Expects the header values in any order (this also sets the expected message count).
    pub fn expected_header_values_received_in_any_order<I, V>(&self, name: &str, values: I)
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.values_in_any_order(false, name, values.into_iter().map(Into::into).collect());
    }

    /// Expects the exchange property values in any order (this also sets the expected message
    /// count).
    pub fn expected_property_values_received_in_any_order<I, V>(&self, name: &str, values: I)
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.values_in_any_order(true, name, values.into_iter().map(Into::into).collect());
    }

    fn values_in_any_order(&self, property: bool, name: &str, values: Vec<Value>) {
        self.expected_message_count(values.len() as i32);
        let name = name.to_string();
        self.expects_task(Arc::new(move |s: &State| {
            // the expected values still to find (a set, as in Java)
            let mut remaining: Vec<Value> = Vec::new();
            for v in &values {
                if !remaining.contains(v) {
                    remaining.push(v.clone());
                }
            }
            for e in &s.received {
                let actual = if property { e.property(&name) } else { e.in_message().header(&name) };
                let actual = actual.cloned().unwrap_or(Value::Null);
                for expected in remaining.clone() {
                    if convert_like(&expected, &actual).as_ref() == Some(&expected) {
                        remaining.retain(|r| *r != expected);
                    }
                }
            }
            if remaining.is_empty() {
                return Ok(());
            }
            let (plural, singular) = if property { ("properties", "property") } else { ("headers", "header") };
            let rest: Vec<String> = remaining.iter().map(|v| v.to_string()).collect();
            Err(format!(
                "Expected {} {plural} with key[{name}], received {} {plural}. Expected {singular} values: [{}]",
                values.len(),
                values.len() - remaining.len(),
                rest.join(", ")
            ))
        }));
    }

    fn expected_scoped_value(&self, kind: &'static str, name: &str, value: Value) {
        {
            let mut s = self.state();
            let map = match kind {
                "Property" => s.expected_property_values.get_or_insert_with(Vec::new),
                _ => s.expected_variable_values.get_or_insert_with(Vec::new),
            };
            map.retain(|(k, _)| k != name);
            map.push((name.to_string(), value));
        }
        self.expects_task(Arc::new(move |s: &State| {
            let lower = kind.to_lowercase();
            let values = match kind {
                "Property" => s.expected_property_values.clone(),
                _ => s.expected_variable_values.clone(),
            }
            .unwrap_or_default();
            for (i, e) in s.received.iter().enumerate() {
                for (key, expected) in &values {
                    let actual = match kind {
                        "Property" => e.property(key).cloned(),
                        _ => e.variable(key).cloned(),
                    }
                    .unwrap_or(Value::Null);
                    if !expected.is_null() && actual.is_null() {
                        return Err(format!("No {lower} with name {key} found for message: {i}"));
                    }
                    assert_value_equals(
                        &format!("{kind} with name {key} for message: {i}"),
                        expected,
                        &actual,
                    )?;
                }
            }
            Ok(())
        }));
    }

    /// Expects the exchange property on the received messages.
    pub fn expected_property_received(&self, name: &str, value: impl Into<Value>) {
        self.expected_scoped_value("Property", name, value.into());
    }

    /// Expects the exchange variable on the received messages.
    pub fn expected_variable_received(&self, name: &str, value: impl Into<Value>) {
        self.ensure_minimum_one();
        self.expected_scoped_value("Variable", name, value.into());
    }

    /// Expects that message `i` matches predicate `i`.
    pub fn expected_messages_matches(&self, predicates: Vec<SharedPredicate>) {
        for (index, predicate) in predicates.into_iter().enumerate() {
            self.message_matches(index, predicate);
        }
    }

    /// Expects that the message at the index (if received) matches the predicate (Java
    /// `message(index).predicate(...)`).
    pub fn message_matches(&self, index: usize, predicate: SharedPredicate) {
        self.expects_task(Arc::new(move |s: &State| {
            if let Some(e) = s.received.get(index) {
                if !predicate.matches(e).map_err(|err| err.to_string())? {
                    let desc = predicate.describe();
                    return Err(format!("Assertion error at index {index} on mock {} with predicate: {desc} evaluated as: false on {e}", e.from_endpoint().unwrap_or_default()));
                }
            }
            Ok(())
        }));
    }

    /// Expects that all the received messages match the predicate (Java
    /// `allMessages().predicate(...)`).
    pub fn all_messages_match(&self, predicate: SharedPredicate) {
        self.expects_task(Arc::new(move |s: &State| {
            for (index, e) in s.received.iter().enumerate() {
                if !predicate.matches(e).map_err(|err| err.to_string())? {
                    let desc = predicate.describe();
                    return Err(format!("Assertion error at index {index} with predicate: {desc} evaluated as: false on {e}"));
                }
            }
            Ok(())
        }));
    }

    /// Expects the exchange pattern on the received messages.
    pub fn expected_exchange_pattern(&self, pattern: ExchangePattern) {
        self.expects(move |received| {
            for (i, e) in received.iter().enumerate() {
                if e.pattern() != pattern {
                    return Err(format!(
                        "Exchange {i} has pattern {} but expected {pattern}",
                        e.pattern()
                    ));
                }
            }
            Ok(())
        });
    }

    fn sorted_task(expression: SharedExpression, ascending: bool) -> Task {
        Arc::new(move |s: &State| {
            let kind = if ascending { "ascending" } else { "descending" };
            let desc = expression.describe();
            for index in 0..s.received.len() {
                // as in Java, the first pair (0, 1) is not compared
                let prev = index as i64 - 1;
                if prev > 0 {
                    let e1 = &s.received[prev as usize];
                    let e2 = &s.received[index];
                    let v1 = expression.evaluate(e1).map_err(|e| e.to_string())?;
                    let v2 = expression.evaluate(e2).map_err(|e| e.to_string())?;
                    let mut result = compare_values(&v1, &v2);
                    if result == CmpOrdering::Equal {
                        return Err(format!(
                            "Messages not {kind}. Messages{prev} and {index} are equal with value: {v1} for expression: {desc}. Exchanges: {e1} and {e2}"
                        ));
                    }
                    if !ascending {
                        result = result.reverse();
                    }
                    if result == CmpOrdering::Greater {
                        return Err(format!(
                            "Messages not {kind}. Message {prev} has value: {v1} and message {index} has value: {v2} for expression: {desc}. Exchanges: {e1} and {e2}"
                        ));
                    }
                }
            }
            Ok(())
        })
    }

    /// Expects the values of the expression to be in ascending order over the messages.
    pub fn expects_ascending(&self, expression: SharedExpression) {
        self.expects_task(Self::sorted_task(expression, true));
    }

    /// Expects the values of the expression to be in descending order over the messages.
    pub fn expects_descending(&self, expression: SharedExpression) {
        self.expects_task(Self::sorted_task(expression, false));
    }

    /// Expects the values of the expression to be unique over the messages.
    pub fn expects_no_duplicates(&self, expression: SharedExpression) {
        self.expects_task(Arc::new(move |s: &State| {
            let mut seen: Vec<(Value, usize)> = Vec::new();
            let desc = expression.describe();
            for (i, e) in s.received.iter().enumerate() {
                let key = expression.evaluate(e).map_err(|err| err.to_string())?;
                if let Some((_, j)) = seen.iter().find(|(k, _)| *k == key) {
                    return Err(format!(
                        "Duplicate message found on message {i} has value: {key} for expression: {desc}. Exchanges: {} and {e}",
                        s.received[*j]
                    ));
                }
                seen.push((key, i));
            }
            Ok(())
        }));
    }

    // ---------------------------------------------------------------------------------------
    // behaviour
    // ---------------------------------------------------------------------------------------

    /// Runs the processor on every received exchange (for example to set a reply or fail).
    pub fn when_any_exchange_received(&self, processor: SharedProcessor) {
        self.state().default_processor = Some(processor);
    }

    /// Runs the processor on the received exchange with the index (1-based, as in Java's
    /// counter).
    pub fn when_exchange_received(&self, index: usize, processor: SharedProcessor) {
        self.state().processors.insert(index, processor);
    }

    /// Sets a processor that is called first with every received exchange (Java
    /// `setReporter`), for example to collect statistics.
    pub fn set_reporter(&self, reporter: SharedProcessor) {
        self.state().reporter = Some(reporter);
    }

    /// Whether a reporter is set.
    pub fn has_reporter(&self) -> bool {
        self.state().reporter.is_some()
    }

    /// Sets the reply body from the expression on every received exchange.
    pub fn return_reply_body(&self, expression: SharedExpression) {
        self.when_any_exchange_received(Arc::new(camel_api::processor::processor_fn(
            move |e: &mut Exchange| {
                let v = expression.evaluate(e)?;
                e.message_mut().set_body(v);
                Ok(())
            },
        )));
    }

    /// Sets the reply header from the expression on every received exchange.
    pub fn return_reply_header(&self, name: &str, expression: SharedExpression) {
        let name = name.to_string();
        self.when_any_exchange_received(Arc::new(camel_api::processor::processor_fn(
            move |e: &mut Exchange| {
                let v = expression.evaluate(e)?;
                e.message_mut().set_header(&name, v);
                Ok(())
            },
        )));
    }

    // ---------------------------------------------------------------------------------------
    // options
    // ---------------------------------------------------------------------------------------

    /// The maximum time (millis) to wait for the expected messages.
    pub fn result_wait_time(&self) -> u64 {
        self.state().result_wait_time
    }

    /// How long (millis) to wait before checking that no message arrived.
    pub fn sleep_for_empty_test(&self) -> u64 {
        self.state().sleep_for_empty_test
    }

    /// Re-asserts after this period (millis), to ensure the assertion is still valid.
    pub fn set_assert_period(&self, millis: u64) {
        self.state().assert_period = millis;
    }

    /// The maximum time (millis) to wait for the expected messages (0 means 10 seconds).
    pub fn set_result_wait_time(&self, millis: u64) {
        self.state().result_wait_time = millis;
    }

    /// The minimum time (millis) the expected messages must take to arrive.
    pub fn set_result_minimum_wait_time(&self, millis: u64) {
        self.state().result_minimum_wait_time = millis;
    }

    /// How long (millis) to wait before checking that no message arrived, when expecting 0.
    pub fn set_sleep_for_empty_test(&self, millis: u64) {
        self.state().sleep_for_empty_test = millis;
    }

    /// Only keep the first n received exchanges (-1 keeps all).
    pub fn set_retain_first(&self, n: i32) {
        self.state().retain_first = n;
    }

    /// Only keep the last n received exchanges (-1 keeps all).
    pub fn set_retain_last(&self, n: i32) {
        self.state().retain_last = n;
    }

    /// Whether to keep a copy of the received exchange (default `true`).
    pub fn set_copy_on_exchange(&self, copy: bool) {
        self.state().copy_on_exchange = copy;
    }

    /// Whether to log every received message.
    pub fn set_log(&self, log: bool) {
        self.state().log = log;
    }

    // ---------------------------------------------------------------------------------------
    // received exchanges
    // ---------------------------------------------------------------------------------------

    /// The number of received messages.
    pub fn received_counter(&self) -> usize {
        self.state().counter
    }

    /// The received (retained) exchanges.
    pub fn received_exchanges(&self) -> Vec<Exchange> {
        self.state().received.clone()
    }

    /// The received exchange at the index.
    ///
    /// # Panics
    ///
    /// Panics with `There is no exchange at index i` if there is none.
    pub fn assert_exchange_received(&self, index: usize) -> Exchange {
        let s = self.state();
        match s.received_exchange(index) {
            Ok(e) => e.clone(),
            Err(m) => panic!("{} {m}", self.inner.uri),
        }
    }

    /// The failures caught while receiving (such as a failing reply processor).
    pub fn failures(&self) -> Vec<String> {
        self.state().failures.clone()
    }

    async fn on_exchange(&self, exchange: &mut Exchange) {
        let reporter = self.state().reporter.clone();
        if let Some(r) = reporter {
            if let Err(e) = r.process(exchange).await {
                self.state().failures.push(e.to_string());
            }
        }
        let processor = {
            let mut s = self.state();
            if s.log {
                log::info!(
                    "mock:{} received #{} -> {exchange}",
                    self.inner.name,
                    s.counter + 1
                );
            }
            let mut copy = exchange.clone();
            if let Some(expected) = s.expected_body_values.clone() {
                let index = s.actual_body_values.len();
                if index < expected.len() {
                    let body = copy.in_message().body().clone();
                    let converted = convert_like(&expected[index], &body).unwrap_or(body);
                    s.actual_body_values.push(converted);
                }
            } else {
                let body = copy.in_message().body().clone();
                s.actual_body_values.push(body);
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            copy.set_property("CamelReceivedTimestamp", now);
            s.add_received(copy);
            s.counter += 1;
            s.processors
                .get(&s.counter)
                .cloned()
                .or_else(|| s.default_processor.clone())
        };
        if let Some(p) = processor {
            if let Err(e) = p.process(exchange).await {
                exchange.set_exception(Some(e));
            }
        }
        {
            let mut s = self.state();
            if let Some(l) = s.latch.as_mut() {
                *l = l.saturating_sub(1);
            }
        }
        self.inner.changed.notify_waiters();
    }

    /// Waits until the expected messages arrived or the timeout elapsed (Java `await`).
    ///
    /// # Returns
    ///
    /// Whether the expected messages arrived in time.
    pub async fn await_messages(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.inner.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.state().latch {
                None | Some(0) => return true,
                _ => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let _ = tokio::time::timeout(remaining, notified).await;
        }
    }

    async fn wait_for_complete_latch(&self) -> TaskResult {
        let (wait, minimum) = {
            let s = self.state();
            (
                if s.result_wait_time == 0 {
                    10000
                } else {
                    s.result_wait_time
                },
                s.result_minimum_wait_time,
            )
        };
        let watch = Instant::now();
        if !self.await_messages(Duration::from_millis(wait)).await {
            log::warn!("The latch did not reach 0 within the specified time");
        }
        let delta = watch.elapsed().as_millis() as u64;
        if minimum > 0 && delta < minimum {
            return Err(format!("Expected minimum {minimum} millis waiting on the result, but was faster with {delta} millis."));
        }
        Ok(())
    }

    async fn do_assert_is_satisfied(&self, timeout_for_empty: u64) -> TaskResult {
        let (expected, minimum, counter) = {
            let s = self.state();
            (s.expected_count, s.expected_minimum_count, s.counter as i32)
        };
        if expected == 0 {
            if timeout_for_empty > 0 {
                tokio::time::sleep(Duration::from_millis(timeout_for_empty)).await;
            }
            let c = self.received_counter() as i32;
            if c != expected {
                return Err(format!(
                    "Received message count. Expected: <{expected}> but was: <{c}>"
                ));
            }
        } else if expected > 0 {
            if expected != counter {
                self.wait_for_complete_latch().await?;
            }
            let c = self.received_counter() as i32;
            if c != expected {
                return Err(format!(
                    "Received message count. Expected: <{expected}> but was: <{c}>"
                ));
            }
        } else if minimum > 0 && counter < minimum {
            self.wait_for_complete_latch().await?;
        }
        let s = self.state();
        if minimum >= 0 {
            let c = s.counter as i32;
            if minimum > c {
                return Err(format!(
                    "Received message count {c}, expected at least {minimum}"
                ));
            }
        }
        for test in s.tests.clone() {
            test(&s)?;
        }
        if let Some(f) = s.failures.first() {
            return Err(format!(
                "Caught exception on {} due to: {f}",
                self.inner.uri
            ));
        }
        Ok(())
    }

    /// Checks the expectations, waiting for the expected messages first.
    ///
    /// # Errors
    ///
    /// Returns the first failed expectation.
    pub async fn try_assert_is_satisfied(&self) -> Result<(), MockAssertionError> {
        let sleep = self.state().sleep_for_empty_test;
        self.try_assert_is_satisfied_with(sleep).await
    }

    /// Checks the expectations; when no message is expected, waits `timeout_for_empty`
    /// millis first to check that really no message arrives.
    ///
    /// # Errors
    ///
    /// Returns the first failed expectation.
    pub async fn try_assert_is_satisfied_with(
        &self,
        timeout_for_empty: u64,
    ) -> Result<(), MockAssertionError> {
        log::info!("Asserting: {} is satisfied", self.inner.uri);
        let wrap = |m: String| MockAssertionError {
            message: format!("{} {m}", self.inner.uri),
        };
        self.do_assert_is_satisfied(timeout_for_empty)
            .await
            .map_err(wrap)?;
        let period = self.state().assert_period;
        if period > 0 {
            tokio::time::sleep(Duration::from_millis(period)).await;
            log::info!(
                "Re-asserting: {} is satisfied after {period} millis",
                self.inner.uri
            );
            self.do_assert_is_satisfied(0).await.map_err(wrap)?;
        }
        Ok(())
    }

    /// Asserts the expectations, waiting for the expected messages first.
    ///
    /// # Panics
    ///
    /// Panics with the first failed expectation.
    pub async fn assert_is_satisfied(&self) {
        if let Err(e) = self.try_assert_is_satisfied().await {
            panic!("{e}");
        }
    }

    /// Asserts the expectations; when no message is expected, waits `timeout_for_empty` millis
    /// first (Java `assertIsSatisfied(long)`).
    ///
    /// # Panics
    ///
    /// Panics with the first failed expectation.
    pub async fn assert_is_satisfied_with(&self, timeout_for_empty: u64) {
        if let Err(e) = self.try_assert_is_satisfied_with(timeout_for_empty).await {
            panic!("{e}");
        }
    }

    /// Asserts that the expectations are *not* met, waiting `timeout_for_empty` millis when
    /// no message is expected (Java `assertIsNotSatisfied(long)`).
    ///
    /// # Panics
    ///
    /// Panics if the expectations are met.
    pub async fn assert_is_not_satisfied_with(&self, timeout_for_empty: u64) {
        if self
            .try_assert_is_satisfied_with(timeout_for_empty)
            .await
            .is_ok()
        {
            panic!(
                "{} Expected some failures but none occurred",
                self.inner.uri
            );
        }
    }

    /// Asserts that the expectations are *not* met.
    ///
    /// # Panics
    ///
    /// Panics with `<uri> Expected some failures but none occurred` if they are met.
    pub async fn assert_is_not_satisfied(&self) {
        if self.try_assert_is_satisfied().await.is_ok() {
            panic!(
                "{} Expected some failures but none occurred",
                self.inner.uri
            );
        }
    }
}

impl Service for MockEndpoint {}

impl Endpoint for MockEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.inner.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.inner
            .context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(MockProducer {
            endpoint: (*self).clone(),
        }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            format!("You cannot consume from this endpoint: {}", self.inner.uri),
        ))
    }

    fn is_lenient_properties(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The producer of a mock endpoint: records the exchange.
struct MockProducer {
    endpoint: MockEndpoint,
}

impl Service for MockProducer {}

impl Processor for MockProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            self.endpoint.on_exchange(exchange).await;
            Ok(())
        })
    }
}

impl Producer for MockProducer {
    fn endpoint(&self) -> SharedEndpoint {
        Arc::new(self.endpoint.clone())
    }
}
