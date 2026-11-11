//! The message of an exchange. Port of `org.apache.camel.Message` (and the default
//! implementation `org.apache.camel.support.DefaultMessage`).

use crate::convert::FromValue;
use crate::error::CamelResult;
use camel_util::case_insensitive_map::CaseInsensitiveMap;
use camel_util::value::Value;
use std::fmt;

/// A message: a body and headers, plus an optional message id and timestamp.
///
/// The headers use case-insensitive keys (preserving the original case), as Camel headers do.
///
/// # Examples
///
/// ```
/// use camel_api::message::Message;
/// let mut msg = Message::new();
/// msg.set_body("123");
/// msg.set_header("Foo", 42);
/// assert_eq!(Some(123), msg.body_as::<i32>().unwrap());
/// assert_eq!(Some(42), msg.header_as::<i64>("foo").unwrap());
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Message {
    body: Value,
    headers: CaseInsensitiveMap<Value>,
    message_id: Option<String>,
    message_timestamp: i64,
}

impl Message {
    /// Creates an empty message.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a message with the body.
    pub fn with_body(body: impl Into<Value>) -> Self {
        Message {
            body: body.into(),
            ..Self::default()
        }
    }

    /// The message id, if set.
    pub fn message_id(&self) -> Option<&str> {
        self.message_id.as_deref()
    }

    /// Sets the message id.
    pub fn set_message_id(&mut self, id: impl Into<String>) {
        self.message_id = Some(id.into());
    }

    /// Whether the message has a message id.
    pub fn has_message_id(&self) -> bool {
        self.message_id.is_some()
    }

    /// The timestamp (millis since epoch) the message was created by its consumer, or `0`.
    ///
    /// Without a timestamp, the `CamelMessageTimestamp` header is used (Java
    /// `MessageSupport.getMessageTimestamp`).
    pub fn message_timestamp(&self) -> i64 {
        let ts = if self.message_timestamp == 0 {
            self.header(crate::exchange_constants::MESSAGE_TIMESTAMP)
                .and_then(|v| i64::from_value(v).ok().flatten())
                .unwrap_or(-1)
        } else {
            self.message_timestamp
        };
        ts.max(0)
    }

    /// Sets the message timestamp.
    pub fn set_message_timestamp(&mut self, timestamp: i64) {
        self.message_timestamp = timestamp;
    }

    /// The body.
    pub fn body(&self) -> &Value {
        &self.body
    }

    /// The mutable body.
    pub fn body_mut(&mut self) -> &mut Value {
        &mut self.body
    }

    /// Takes the body, leaving null.
    pub fn take_body(&mut self) -> Value {
        std::mem::take(&mut self.body)
    }

    /// Sets the body.
    pub fn set_body(&mut self, body: impl Into<Value>) {
        self.body = body.into();
    }

    /// Returns the body converted to the type.
    ///
    /// # Returns
    ///
    /// `Ok(None)` if the body is null.
    ///
    /// # Errors
    ///
    /// Returns an error if the body cannot be converted.
    pub fn body_as<T: FromValue>(&self) -> CamelResult<Option<T>> {
        T::from_value(&self.body)
    }

    /// Returns a reference to the body if it is a [`Value::Object`] of the Rust type `T`.
    pub fn body_object<T: std::any::Any>(&self) -> Option<&T> {
        self.body.downcast_ref::<T>()
    }

    /// The header value, or `None` if not present.
    pub fn header(&self, name: &str) -> Option<&Value> {
        self.headers.get(name)
    }

    /// The header converted to the type.
    ///
    /// # Errors
    ///
    /// Returns an error if the header cannot be converted.
    pub fn header_as<T: FromValue>(&self, name: &str) -> CamelResult<Option<T>> {
        match self.headers.get(name) {
            None => Ok(None),
            Some(v) => T::from_value(v),
        }
    }

    /// The header, or the default value if not present.
    pub fn header_or(&self, name: &str, default_value: impl Into<Value>) -> Value {
        self.headers
            .get(name)
            .cloned()
            .unwrap_or_else(|| default_value.into())
    }

    /// Sets the header (a later set with another key case replaces the value, keeping the case
    /// of the first set).
    pub fn set_header(&mut self, name: impl AsRef<str>, value: impl Into<Value>) {
        self.headers.insert(name, value.into());
    }

    /// Removes the header, returning the old value.
    pub fn remove_header(&mut self, name: &str) -> Option<Value> {
        self.headers.remove(name)
    }

    /// Removes the headers whose name matches the pattern (see
    /// [`camel_util::string_helper::matches`]: exact, `*` wildcard or regular expression; names
    /// are compared ignoring case).
    ///
    /// # Returns
    ///
    /// `true` if any header was removed.
    pub fn remove_headers(&mut self, pattern: &str) -> bool {
        self.remove_headers_excluding(pattern, &[])
    }

    /// Removes the headers whose name matches the pattern, except those matching any of the
    /// exclude patterns.
    pub fn remove_headers_excluding(&mut self, pattern: &str, exclude_patterns: &[&str]) -> bool {
        let before = self.headers.len();
        self.headers.retain(|k, _| {
            let matched = header_matches(pattern, k);
            matched && exclude_patterns.iter().any(|p| header_matches(p, k)) || !matched
        });
        before != self.headers.len()
    }

    /// All the headers.
    pub fn headers(&self) -> &CaseInsensitiveMap<Value> {
        &self.headers
    }

    /// All the headers (mutable).
    pub fn headers_mut(&mut self) -> &mut CaseInsensitiveMap<Value> {
        &mut self.headers
    }

    /// Replaces all the headers.
    pub fn set_headers(&mut self, headers: CaseInsensitiveMap<Value>) {
        self.headers = headers;
    }

    /// Whether the message has any headers.
    pub fn has_headers(&self) -> bool {
        !self.headers.is_empty()
    }

    /// Copies the body and headers from the other message (Java `copyFrom`).
    pub fn copy_from(&mut self, other: &Message) {
        self.body = other.body.clone();
        self.headers = other.headers.clone();
        self.message_id = other.message_id.clone();
        self.message_timestamp = other.message_timestamp;
    }

    /// Copies the body from the other message (Java `copyFromWithNewBody` with the other body).
    pub fn copy_from_with_new_body(&mut self, other: &Message, body: impl Into<Value>) {
        self.copy_from(other);
        self.body = body.into();
    }
}

/// Whether the header name matches the pattern (Java `PatternHelper.matchPattern`), case
/// insensitive, in this order: exact match, `*` matches everything, wildcard (the pattern ends
/// with `*` and the name starts with the text before it, such as `Camel*`), regular expression.
pub fn header_matches(pattern: &str, name: &str) -> bool {
    if pattern.eq_ignore_ascii_case(name) || pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        if name.to_lowercase().starts_with(&prefix.to_lowercase()) {
            return true;
        }
    }
    regex::Regex::new(&format!("^(?i:{pattern})$"))
        .map(|re| re.is_match(name))
        .unwrap_or(false)
}

impl fmt::Display for Message {
    /// Does not output the content of the message, as it may contain sensitive information.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message_id {
            Some(id) => write!(f, "Message[{id}]"),
            None => f.write_str("Message"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_headers() {
        let mut m = Message::with_body("Hello");
        assert_eq!(Some("Hello".to_string()), m.body_as::<String>().unwrap());
        m.set_header("CamelFileName", "a.txt");
        m.set_header("camelfilename", "b.txt");
        assert_eq!(1, m.headers().len());
        assert_eq!(Some(&Value::from("b.txt")), m.header("CAMELFILENAME"));
        assert_eq!(Value::from("x"), m.header_or("missing", "x"));
        assert!(m.has_headers());
        assert_eq!(Some(Value::from("b.txt")), m.remove_header("CamelFileName"));
        assert!(!m.has_headers());
        assert_eq!("Message", m.to_string());
        m.set_message_id("ID-1");
        assert_eq!("Message[ID-1]", m.to_string());
        assert!(m.has_message_id());
        assert_eq!(Value::from("Hello"), m.take_body());
        assert!(m.body().is_null());
    }

    #[test]
    fn remove_headers() {
        let mut m = Message::new();
        for h in ["CamelA", "CamelB", "foo", "fooBar", "Other"] {
            m.set_header(h, 1);
        }
        assert!(m.remove_headers_excluding("Camel*", &["CamelB"]));
        assert_eq!(
            vec!["CamelB", "foo", "fooBar", "Other"],
            m.headers().keys().collect::<Vec<_>>()
        );
        assert!(m.remove_headers("foo.*"));
        assert_eq!(
            vec!["CamelB", "Other"],
            m.headers().keys().collect::<Vec<_>>()
        );
        assert!(!m.remove_headers("nope"));
        assert!(m.remove_headers("other"));
        assert!(m.remove_headers("*"));
        assert!(!m.has_headers());
    }

    #[test]
    fn copy() {
        let mut a = Message::with_body(1);
        a.set_header("x", "y");
        a.set_message_timestamp(5);
        let mut b = Message::new();
        b.copy_from_with_new_body(&a, "new");
        assert_eq!(&Value::from("new"), b.body());
        assert_eq!(Some(&Value::from("y")), b.header("x"));
        assert_eq!(5, b.message_timestamp());
        #[derive(Debug, PartialEq)]
        struct Order(u8);
        let m = Message::with_body(Value::object(Order(3)));
        assert_eq!(Some(&Order(3)), m.body_object::<Order>());
    }
}
