//! The CamelContext contract (Java `org.apache.camel.CamelContext`).
//!
//! The Java interface has hundreds of methods; the Rust trait grows with the port. The
//! implementation lives in the engine crate (`camel-base-engine`).

use crate::spi::uuid::{DefaultUuidGenerator, UuidGenerator};
use std::any::Any;
use std::sync::{Arc, OnceLock};

/// The context of a Camel application: the registry of components, endpoints, routes and
/// services.
pub trait CamelContext: Send + Sync {
    /// The name of the context.
    fn name(&self) -> String;

    /// The generator of unique ids (exchange ids, message ids, ...).
    fn uuid_generator(&self) -> Arc<dyn UuidGenerator>;

    /// Whether message history is enabled.
    fn is_message_history(&self) -> bool {
        false
    }

    /// The context as `Any`, to access the concrete implementation.
    fn as_any(&self) -> &dyn Any;
}

/// A minimal [`CamelContext`] for tests and standalone use of exchanges, without any routes or
/// components.
#[derive(Debug)]
pub struct StandaloneContext {
    name: String,
    uuid: Arc<DefaultUuidGenerator>,
}

impl StandaloneContext {
    /// Creates a context with the name.
    pub fn new(name: &str) -> Self {
        StandaloneContext {
            name: name.to_string(),
            uuid: Arc::new(DefaultUuidGenerator::new()),
        }
    }

    /// A shared standalone context named `camel-standalone`.
    pub fn shared() -> Arc<dyn CamelContext> {
        static CTX: OnceLock<Arc<dyn CamelContext>> = OnceLock::new();
        CTX.get_or_init(|| Arc::new(StandaloneContext::new("camel-standalone")))
            .clone()
    }
}

impl CamelContext for StandaloneContext {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn uuid_generator(&self) -> Arc<dyn UuidGenerator> {
        self.uuid.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
