//! # camel-support
//!
//! Base implementations used by the engine and by components: the default registry, header
//! filter strategy, pattern matching and other helpers. This crate is the Rust port of the Java
//! `core/camel-support` module (`org.apache.camel.support`).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod builder;
pub mod component_support;
pub mod endpoint_helper;
pub mod exchange_formatter;
pub mod exchange_helper;
pub mod header_filter_strategy;
pub mod idempotent;
pub mod message_helper;
pub mod object_helper;
pub mod pattern_helper;
pub mod resource_helper;
pub mod route_policy_support;
pub mod simple_registry;
pub mod tokenizer;
pub mod type_conversion;
pub mod uuid_generators;
