//! Formatting exchanges as text for logging (Java `org.apache.camel.spi.ExchangeFormatter`).

use crate::exchange::Exchange;

/// Formats an exchange as text (for the log component, the error handler, tracing, ...).
pub trait ExchangeFormatter: Send + Sync {
    /// The text form of the exchange.
    fn format(&self, exchange: &Exchange) -> String;
}
