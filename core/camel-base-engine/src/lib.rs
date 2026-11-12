//! # camel-base-engine
//!
//! The routing engine: [`DefaultCamelContext`](context::DefaultCamelContext) resolves
//! components and endpoints, holds the bean registry and runs the routes;
//! [`ProducerTemplate`](producer_template::ProducerTemplate) sends messages to endpoints. This
//! crate is the Rust port of the Java `core/camel-base-engine` module
//! (`org.apache.camel.impl.engine`) and of `DefaultCamelContext` from `camel-core-engine`.
//!
//! The engine runs on the [tokio](https://tokio.rs) runtime: start the context and send
//! messages from async code (for example in `#[tokio::main]` or `#[tokio::test]`).
//!
//! # Examples
//!
//! ```
//! use camel_api::context::CamelContext;
//! use camel_api::error::ErrorKind;
//! use camel_base_engine::context::DefaultCamelContext;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = DefaultCamelContext::with_name("demo");
//! context.start().await.unwrap();
//! assert!(context.status().is_started());
//! // no component is registered for the scheme
//! let err = context.endpoint("unknown:foo").err().unwrap();
//! assert!(err.is(ErrorKind::NoSuchEndpoint));
//! context.stop().await.unwrap();
//! # });
//! ```
//!
//! See the `camel-direct` crate for a complete route.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod consumer_template;
pub mod context;
pub mod duration_route_policy;
pub mod producer_template;
pub mod route;
