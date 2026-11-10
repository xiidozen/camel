//! # camel-api
//!
//! The public API of the Camel routing engine: the exchange and message model, the errors,
//! the processor/endpoint/component contracts and the CamelContext. This crate is the Rust port
//! of the Java `core/camel-api` module (`org.apache.camel`).
//!
//! In the Rust port the exchange and message are concrete types ([`exchange::Exchange`],
//! [`message::Message`]) rather than interfaces, and processing is asynchronous: a
//! [`processor::Processor`] returns a future, so routes run on an async runtime.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod context;
pub mod convert;
pub mod enums;
pub mod error;
pub mod exchange;
pub mod exchange_constants;
pub mod message;
pub mod processor;
pub mod spi;
