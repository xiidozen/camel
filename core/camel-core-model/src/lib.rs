//! # camel-core-model
//!
//! The route model (Java `org.apache.camel.model`): [`RouteDefinition`](route::RouteDefinition)
//! and the tree of [`ProcessorDefinition`](processor::ProcessorDefinition)s describing what a
//! route does, and the **Rust route DSL** ([`builder`]) building that model.
//!
//! The Rust, YAML and XML DSLs all produce this model; the reifier (`camel-core-reifier`)
//! turns it into the running processors.
//!
//! # Examples
//!
//! ```
//! use camel_core_model::builder::*;
//!
//! let route = from("direct:start")
//!     .route_id("greetings")
//!     .filter(header("lang").is_equal_to("en"), |f| f.set_body(constant("Hello")).to("mock:en"))
//!     .choice(|c| {
//!         c.when(body().contains("Hello"), |w| w.to("mock:hello"))
//!             .otherwise(|o| o.to("mock:other"))
//!     })
//!     .to("mock:result");
//! let def = route.build();
//! assert_eq!(Some("greetings"), def.id.as_deref());
//! assert_eq!(
//!     "Route(greetings)[From[direct:start] -> [Filter[header(lang) == en -> [SetBody[Hello], to[mock:en]]], \
//!      Choice[[When[body contains Hello -> [to[mock:hello]]]] Otherwise[[to[mock:other]]]], to[mock:result]]]",
//!     def.to_string()
//! );
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod builder;
pub mod expression;
pub mod processor;
pub mod route;
