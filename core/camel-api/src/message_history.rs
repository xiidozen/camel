//! The message history (Java `MessageHistory` and `MessageHistoryFactory`): when enabled on the
//! context, each step an exchange goes through adds an entry to the `CamelMessageHistory`
//! exchange property (the route id, the node id and label, the time and the elapsed time, and
//! the body type and size when the step was reached).

use crate::exchange::Exchange;
use crate::exchange_constants::MESSAGE_HISTORY;
use camel_util::value::{ObjectRef, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

/// An entry of the message history.
#[derive(Debug, Clone)]
pub struct MessageHistory {
    /// The id of the route.
    pub route_id: String,
    /// The id of the node (the step).
    pub node_id: String,
    /// The short name of the node, such as `to` or `step`.
    pub short_name: String,
    /// The label of the node, such as `to[mock:a]`.
    pub label: String,
    /// The time the node was reached (millis since epoch).
    pub time: i64,
    /// The time the node took (millis; 0 while running).
    pub elapsed: i64,
    /// The type of the body when the node was reached (Java class name), if captured.
    pub body_type: Option<String>,
    /// The size of the body when the node was reached, `-1` if unknown or not captured.
    pub body_size: i64,
    /// A copy of the message when the node was reached (with the `copyMessage` option).
    pub message: Option<Box<crate::message::Message>>,
}

/// The Java class name of the type of the value (Java `ObjectHelper.classCanonicalName`), or
/// `null`.
pub fn body_type(value: &Value) -> String {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "java.lang.Boolean",
        Value::Int(_) => "java.lang.Long",
        Value::Float(_) => "java.lang.Double",
        Value::Text(_) => "java.lang.String",
        Value::Bytes(_) => "byte[]",
        Value::List(_) => "java.util.ArrayList",
        Value::Map(_) => "java.util.LinkedHashMap",
        Value::Object(o) => o.type_name(),
    }
    .to_string()
}

/// The size of the body (Java `MessageSizeStrategy.computeBodySize`): the length of a text or
/// bytes, the number of elements of a collection, else `-1`.
pub fn body_size(value: &Value) -> i64 {
    match value {
        Value::Text(t) => t.len() as i64,
        Value::Bytes(b) => b.len() as i64,
        Value::List(l) => l.len() as i64,
        Value::Map(m) => m.len() as i64,
        _ => -1,
    }
}

/// The configuration of the message history of a context (Java `MessageHistoryFactory` and
/// the `MessageSizeStrategy`), a plugin of the context; it also gives the steps without id
/// their node ids (Java `NodeIdFactory`: `to1`, `to2`, `log1`, ...).
#[derive(Default)]
pub struct MessageHistoryFactory {
    node_pattern: RwLock<Option<String>>,
    size_enabled: AtomicBool,
    copy_message: AtomicBool,
    counters: Mutex<HashMap<String, u32>>,
}

impl MessageHistoryFactory {
    /// The pattern of the nodes in the history (their short name or id, such as `step`);
    /// `None` for all.
    pub fn node_pattern(&self) -> Option<String> {
        self.node_pattern.read().unwrap().clone()
    }

    /// Sets the pattern of the nodes in the history (Java `setNodePattern`).
    pub fn set_node_pattern(&self, pattern: Option<&str>) {
        *self.node_pattern.write().unwrap() = pattern.map(str::to_string);
    }

    /// Whether the body sizes are computed (Java `getMessageSizeStrategy().isEnabled()`,
    /// default `false`).
    pub fn is_size_enabled(&self) -> bool {
        self.size_enabled.load(Ordering::SeqCst)
    }

    /// Sets whether the body sizes are computed.
    pub fn set_size_enabled(&self, enabled: bool) {
        self.size_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Whether the entries keep a copy of the message (Java `setCopyMessage`, default
    /// `false`).
    pub fn is_copy_message(&self) -> bool {
        self.copy_message.load(Ordering::SeqCst)
    }

    /// Sets whether the entries keep a copy of the message.
    pub fn set_copy_message(&self, copy: bool) {
        self.copy_message.store(copy, Ordering::SeqCst);
    }

    /// A new node id for the short name (`to1`, `to2`, ...).
    pub fn next_node_id(&self, short_name: &str) -> String {
        let mut counters = self.counters.lock().unwrap();
        let n = counters.entry(short_name.to_string()).or_insert(0);
        *n += 1;
        format!("{short_name}{n}")
    }
}

/// The message history of the exchange, if enabled.
pub fn message_history(exchange: &Exchange) -> Option<Vec<MessageHistory>> {
    match exchange.property(MESSAGE_HISTORY) {
        Some(Value::List(l)) => Some(
            l.iter()
                .filter_map(|v| v.downcast_ref::<MessageHistory>().cloned())
                .collect(),
        ),
        _ => None,
    }
}

/// Adds the entry to the history of the exchange.
///
/// # Returns
///
/// The index of the entry.
pub fn add_message_history(exchange: &mut Exchange, entry: MessageHistory) -> usize {
    let value = Value::Object(ObjectRef::new_display(entry));
    let mut list = match exchange.remove_property(MESSAGE_HISTORY) {
        Some(Value::List(l)) => l,
        _ => Vec::new(),
    };
    list.push(value);
    let index = list.len() - 1;
    exchange.set_property(MESSAGE_HISTORY, Value::List(list));
    index
}

/// Sets the elapsed time of the entry at the index.
pub fn set_message_history_elapsed(exchange: &mut Exchange, index: usize, elapsed: i64) {
    if let Some(Value::List(l)) = exchange.property(MESSAGE_HISTORY) {
        let mut list = l.clone();
        if let Some(entry) = list
            .get(index)
            .and_then(|v| v.downcast_ref::<MessageHistory>())
        {
            let mut entry = entry.clone();
            entry.elapsed = elapsed;
            list[index] = Value::Object(ObjectRef::new_display(entry));
            exchange.set_property(MESSAGE_HISTORY, Value::List(list));
        }
    }
}

/// Java `DefaultMessageHistory.toString`.
impl std::fmt::Display for MessageHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DefaultMessageHistory[routeId={}, node={}]",
            self.route_id, self.node_id
        )
    }
}
