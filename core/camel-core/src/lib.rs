//! # camel-core
//!
//! The Apache Camel integration framework with its core components: the crate to depend on
//! to write and run routes in Rust (the Java `camel-core` module).
//!
//! [`default_context`] creates a CamelContext with the core components registered, and
//! [`prelude`] brings the route DSL and the common types into scope.
//!
//! # Examples
//!
//! ```
//! use camel_core::prelude::*;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = camel_core::default_context();
//! context
//!     .add_routes(|r: &mut Routes| {
//!         r.add(
//!             from("direct:start")
//!                 .filter(header("foo").is_equal_to("bar"), |f| f.to("mock:result")),
//!         );
//!     })
//!     .await
//!     .unwrap();
//! context.start().await.unwrap();
//!
//! let mock = MockEndpoint::resolve(context.as_ref(), "mock:result").unwrap();
//! mock.expected_bodies_received(["Hello"]);
//! let template = context.create_producer_template();
//! template.send_body_and_header("direct:start", "Hello", "foo", "bar").await.unwrap();
//! template.send_body_and_header("direct:start", "Bye", "foo", "baz").await.unwrap();
//! mock.assert_is_satisfied().await;
//! context.stop().await.unwrap();
//! # });
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::bean_binding::BeanProcessorFactory;
use camel_api::endpoint::SharedComponent;
use camel_api::spi::language::Language;
use camel_base_engine::context::DefaultCamelContext;
use std::sync::Arc;

pub use camel_api as api;
pub use camel_base_engine as engine;
pub use camel_core_languages as languages;
pub use camel_core_model as model;
pub use camel_core_processor as processor;
pub use camel_support as support;
pub use camel_util as util;

/// The route DSL and the common types.
pub mod prelude {
    pub use camel_api::context::CamelContext;
    pub use camel_api::enums::{ExchangePattern, LoggingLevel};
    pub use camel_api::error::{CamelError, CamelResult, ErrorKind};
    pub use camel_api::exchange::Exchange;
    pub use camel_api::processor::{Expression, Predicate, Processor};
    pub use camel_api::spi::registry::Registry;
    pub use camel_base_engine::context::DefaultCamelContext;
    pub use camel_base_engine::producer_template::ProducerTemplate;
    pub use camel_core_engine::ModelCamelContext;
    pub use camel_core_model::builder::*;
    pub use camel_mock::MockEndpoint;
    pub use camel_util::value::{Value, ValueMap};
}

/// Registers the factories of the core components (`direct`, `mock`, `log`, `seda`, `bean`,
/// `timer`) on
/// the context, so they are created on first use (Rust has no classpath scanning), and the
/// bean processor factory of the bean EIP.
pub fn register_core_components(context: &DefaultCamelContext) {
    context.register_component_factory(
        "direct",
        Arc::new(|| Arc::new(camel_direct::DirectComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "mock",
        Arc::new(|| Arc::new(camel_mock::MockComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "log",
        Arc::new(|| Arc::new(camel_log::LogComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "seda",
        Arc::new(|| Arc::new(camel_seda::SedaComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "browse",
        Arc::new(|| Arc::new(camel_browse::BrowseComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "language",
        Arc::new(|| Arc::new(camel_language::LanguageComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "scheduler",
        Arc::new(|| Arc::new(camel_scheduler::SchedulerComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "controlbus",
        Arc::new(|| Arc::new(camel_controlbus::ControlBusComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "ref",
        Arc::new(|| Arc::new(camel_ref::RefComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "stub",
        Arc::new(|| Arc::new(camel_stub::StubComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "timer",
        Arc::new(|| Arc::new(camel_timer::TimerComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "file",
        Arc::new(|| Arc::new(camel_file::FileComponent::new()) as SharedComponent),
    );
    context.register_component_factory(
        "bean",
        Arc::new(|| Arc::new(camel_bean::BeanComponent::new()) as SharedComponent),
    );
    // the repository of the inflight exchanges
    context.add_plugin(camel_api::spi::inflight::InflightRepository::default());
    // the configuration of the message history
    context.add_plugin(camel_api::message_history::MessageHistoryFactory::default());
    // the custom functions of the simple language
    context.add_plugin(camel_api::spi::simple_function::SimpleFunctionRegistry::default());
    // the factory of the processors of the bean EIP (Java BeanProcessorFactory plugin)
    context.add_plugin::<Arc<dyn BeanProcessorFactory>>(Arc::new(
        camel_bean::DefaultBeanProcessorFactory,
    ));
}

/// Registers the factories of the core languages (`simple`, `constant`, `header`,
/// `exchangeProperty`, `variable`) and of the `bean` language on the context.
pub fn register_core_languages(context: &DefaultCamelContext) {
    camel_core_languages::register_core_languages(|name, f| {
        context.register_language_factory(name, Arc::new(f))
    });
    context.register_language_factory(
        camel_bean::language::LANGUAGE,
        Arc::new(|| Arc::new(camel_bean::BeanLanguage::new()) as Arc<dyn Language>),
    );
}

/// A new CamelContext with the core components and languages registered (what Java `new
/// DefaultCamelContext()` finds on the classpath).
pub fn default_context() -> Arc<DefaultCamelContext> {
    let context = DefaultCamelContext::new();
    register_core_components(&context);
    register_core_languages(&context);
    context
}
