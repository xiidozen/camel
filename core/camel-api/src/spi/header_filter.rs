//! Filtering of headers between Camel and external systems (Java
//! `org.apache.camel.spi.HeaderFilterStrategy`).
//!
//! Consumers must apply a strategy to the headers of incoming messages, so an external sender
//! cannot set Camel internal headers (`Camel*`, `camel*`, `org.apache.camel.*`), which could
//! otherwise steer the routing (see the security model).

use crate::exchange::Exchange;
use camel_util::value::Value;
use std::sync::Arc;

/// Decides which headers are copied between Camel messages and external messages.
pub trait HeaderFilterStrategy: Send + Sync {
    /// Whether the Camel header must be filtered out (not sent) when creating an external
    /// message.
    fn apply_filter_to_camel_headers(
        &self,
        header_name: &str,
        header_value: &Value,
        exchange: &Exchange,
    ) -> bool;

    /// Whether the external header must be filtered out (not copied) when creating a Camel
    /// message from an external message.
    fn apply_filter_to_external_headers(
        &self,
        header_name: &str,
        header_value: &Value,
        exchange: &Exchange,
    ) -> bool;
}

/// A shared header filter strategy.
pub type SharedHeaderFilterStrategy = Arc<dyn HeaderFilterStrategy>;

/// Whether the header is a Camel internal header (`Camel*`, `camel*` or `org.apache.camel.*`),
/// compared case-insensitively.
///
/// # Examples
///
/// ```
/// use camel_api::spi::header_filter::is_camel_internal_header;
/// assert!(is_camel_internal_header("CamelFileName"));
/// assert!(is_camel_internal_header("CAMELFILENAME"));
/// assert!(is_camel_internal_header("org.apache.camel.foo"));
/// assert!(!is_camel_internal_header("Content-Type"));
/// ```
pub fn is_camel_internal_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("camel") || lower.starts_with("org.apache.camel.")
}
