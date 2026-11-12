//! The CamelContext contract (Java `org.apache.camel.CamelContext`).
//!
//! The Java interface has hundreds of methods; the Rust trait grows with the port. The
//! implementation lives in the engine crate (`camel-base-engine`).

use crate::endpoint::SharedEndpoint;
use crate::error::{CamelError, CamelResult};
use crate::spi::language::Language;
use crate::spi::log_listener::LogListener;
use crate::spi::registry::Registry;
use crate::spi::uuid::{DefaultUuidGenerator, UuidGenerator};
use std::any::{Any, TypeId};
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

    /// Resolves the endpoint for the uri, creating it with its component on first use (Java
    /// `getEndpoint(String)`).
    ///
    /// # Errors
    ///
    /// Returns a `NoSuchEndpointException` if there is no component for the uri scheme, or a
    /// `ResolveEndpointFailedException` if the endpoint cannot be created. A context without
    /// components (such as [`StandaloneContext`]) always fails.
    fn endpoint(&self, uri: &str) -> CamelResult<SharedEndpoint> {
        Err(CamelError::no_such_endpoint(uri))
    }

    /// Resolves the language by name (such as `simple` or `constant`), creating it on first use
    /// (Java `resolveLanguage`).
    ///
    /// # Errors
    ///
    /// Returns a `NoSuchLanguageException` if there is no such language. A context without
    /// languages (such as [`StandaloneContext`]) always fails.
    fn resolve_language(&self, name: &str) -> CamelResult<Arc<dyn Language>> {
        Err(CamelError::no_such_language(name))
    }

    /// The listeners of the log messages (Java `CamelContextExtension.getLogListeners`).
    fn log_listeners(&self) -> Vec<Arc<dyn LogListener>> {
        Vec::new()
    }

    /// The bean registry, if the context has one.
    fn registry(&self) -> Option<Arc<dyn Registry>> {
        None
    }

    /// The plugin of the type, if the context has one (Java
    /// `getCamelContextExtension().getContextPlugin(type)`); see [`context_plugin`].
    fn plugin(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        let _ = type_id;
        None
    }

    /// The context as `Any`, to access the concrete implementation.
    fn as_any(&self) -> &dyn Any;
}

/// The plugin of the type `T` of the context (Java `getContextPlugin(type)`), such as the
/// `Arc<dyn BeanProcessorFactory>` the bean EIP uses
/// ([`crate::bean_binding::BeanProcessorFactory`]).
pub fn context_plugin<T: Any + Send + Sync>(context: &dyn CamelContext) -> Option<Arc<T>> {
    context
        .plugin(TypeId::of::<T>())
        .and_then(|p| p.downcast::<T>().ok())
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
