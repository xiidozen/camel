//! Service provider interfaces (the Java `org.apache.camel.spi` package): the contracts the
//! routing engine and components plug into.

pub mod aggregation_repository;
pub mod data_format;
pub mod event;
pub mod exchange_formatter;
pub mod header_filter;
pub mod idempotent_repository;
pub mod inflight;
pub mod language;
pub mod log_listener;
pub mod poll_strategy;
pub mod properties;
pub mod registry;
pub mod route_policy;
pub mod simple_function;
pub mod synchronization;
pub mod uuid;
