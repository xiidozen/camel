//! Service provider interfaces (the Java `org.apache.camel.spi` package): the contracts the
//! routing engine and components plug into.

pub mod exchange_formatter;
pub mod header_filter;
pub mod language;
pub mod log_listener;
pub mod registry;
pub mod synchronization;
pub mod uuid;
