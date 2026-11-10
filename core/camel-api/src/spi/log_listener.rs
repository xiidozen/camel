//! Listeners of log messages (Java `org.apache.camel.spi.LogListener`).

use crate::exchange::Exchange;

/// Called with the messages logged by the log component and the log EIP; it may change the
/// message (for example to mask data).
pub trait LogListener: Send + Sync {
    /// Called before the message is logged; returns the message to log.
    ///
    /// # Arguments
    ///
    /// * `exchange` - the exchange being logged
    /// * `logger_name` - the name of the logger
    /// * `message` - the message
    fn on_log(&self, exchange: &Exchange, logger_name: &str, message: &str) -> String;
}
