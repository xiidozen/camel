//! # camel-support
//!
//! Base implementations used by the engine and by components: the default registry, header
//! filter strategy, pattern matching and other helpers. This crate is the Rust port of the Java
//! `core/camel-support` module (`org.apache.camel.support`).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod component_support;
pub mod header_filter_strategy;
pub mod pattern_helper;
pub mod simple_registry;
