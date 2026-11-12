//! The endpoint, component, producer and consumer contracts (Java `org.apache.camel.Endpoint`,
//! `Component`, `Producer`, `Consumer` and `PollingConsumer`).

use crate::context::CamelContext;
use crate::enums::ExchangePattern;
use crate::error::{CamelError, CamelResult, ErrorKind};
use crate::exchange::Exchange;
use crate::processor::{BoxFuture, Processor, SharedProcessor};
use crate::service::Service;
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

/// A shared endpoint.
pub type SharedEndpoint = Arc<dyn Endpoint>;
/// A shared producer.
pub type SharedProducer = Arc<dyn Producer>;
/// A shared consumer.
pub type SharedConsumer = Arc<dyn Consumer>;
/// A shared component.
pub type SharedComponent = Arc<dyn Component>;

/// An endpoint: a named destination messages are sent to (by a [`Producer`]) or received from
/// (by a [`Consumer`]), identified by its uri such as `seda:orders?size=100`.
///
/// Endpoints are created by their [`Component`] and are usually singletons per uri.
pub trait Endpoint: Service {
    /// The endpoint uri.
    fn endpoint_uri(&self) -> &str;

    /// The key used to look up the endpoint (the normalized uri).
    fn endpoint_key(&self) -> String {
        camel_util::uri_support::normalize_uri(self.endpoint_uri())
            .unwrap_or_else(|_| self.endpoint_uri().to_string())
    }

    /// The uri without the query parameters.
    fn endpoint_base_uri(&self) -> String {
        camel_util::uri_support::strip_query(self.endpoint_uri())
    }

    /// The default exchange pattern of the exchanges created by this endpoint.
    fn exchange_pattern(&self) -> ExchangePattern {
        ExchangePattern::InOnly
    }

    /// The CamelContext.
    fn camel_context(&self) -> Arc<dyn CamelContext>;

    /// Creates a new exchange for communicating with this endpoint.
    fn create_exchange(&self) -> Exchange {
        self.create_exchange_with_pattern(self.exchange_pattern())
    }

    /// Creates a new exchange with the pattern for communicating with this endpoint.
    fn create_exchange_with_pattern(&self, pattern: ExchangePattern) -> Exchange {
        let mut e = Exchange::with_pattern(self.camel_context(), pattern);
        self.configure_exchange(&mut e);
        e
    }

    /// Configures the exchange created by this endpoint (sets the from endpoint by default).
    fn configure_exchange(&self, exchange: &mut Exchange) {
        exchange.set_from_endpoint(self.endpoint_uri());
    }

    /// Creates a producer for sending messages to this endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint does not support producers or the producer cannot be
    /// created.
    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer>;

    /// Creates a consumer receiving messages from this endpoint, passing them to the processor
    /// (the route).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint does not support consumers or the consumer cannot be
    /// created.
    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer>;

    /// Creates a polling consumer to receive messages on demand.
    ///
    /// # Errors
    ///
    /// By default polling consumers are not supported.
    fn create_polling_consumer(self: Arc<Self>) -> CamelResult<Arc<dyn PollingConsumer>> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            format!(
                "Polling consumer is not supported by endpoint: {}",
                self.endpoint_uri()
            ),
        ))
    }

    /// Whether the endpoint is a singleton (one instance per uri).
    fn is_singleton(&self) -> bool {
        true
    }

    /// Whether producers of this endpoint are singletons (shared).
    fn is_singleton_producer(&self) -> bool {
        self.is_singleton()
    }

    /// Whether unknown uri parameters are allowed (and kept) instead of failing.
    fn is_lenient_properties(&self) -> bool {
        false
    }

    /// Whether the endpoint is remote (communicates over the network).
    fn is_remote(&self) -> bool {
        false
    }

    /// The endpoint as `Any`, to access the concrete type.
    fn as_any(&self) -> &dyn Any;
}

/// A component: a factory of endpoints for a uri scheme (such as `seda` or `file`).
pub trait Component: Service {
    /// Creates the endpoint for the uri.
    ///
    /// # Arguments
    ///
    /// * `context` - the CamelContext
    /// * `uri` - the full (normalized) endpoint uri
    /// * `remaining` - the part of the uri after the scheme and before the query
    /// * `parameters` - the uri query parameters; the component removes the ones it uses, and
    ///   the remaining ones are reported as unknown (unless the endpoint is lenient)
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be created.
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint>;

    /// Whether the component wants the raw (not decoded) uri parameter values.
    fn use_raw_uri(&self) -> bool {
        false
    }

    /// Whether the `RAW(value)` parameter values are resolved to their value before the
    /// endpoint is created (Java `resolveRawParameterValues`, default `true`); a component
    /// proxying another endpoint may keep them.
    fn resolve_raw_parameter_values(&self) -> bool {
        true
    }

    /// The default name (scheme) of the component, if any.
    fn default_name(&self) -> Option<&str> {
        None
    }

    /// Whether autowiring of options from the registry is enabled.
    fn is_autowired_enabled(&self) -> bool {
        true
    }

    /// The component as `Any`, to access the concrete type.
    fn as_any(&self) -> &dyn Any;
}

/// A producer: sends exchanges to its endpoint (it is a processor, so it can be used in routes).
pub trait Producer: Processor + Service {
    /// The endpoint this producer sends to.
    fn endpoint(&self) -> SharedEndpoint;

    /// Whether the producer can be shared.
    fn is_singleton(&self) -> bool {
        true
    }
}

/// A consumer: receives messages from its endpoint and passes them to its processor.
pub trait Consumer: Service {
    /// The endpoint this consumer receives from.
    fn endpoint(&self) -> SharedEndpoint;

    /// The processor (route) the received exchanges are passed to.
    fn processor(&self) -> SharedProcessor;

    /// Creates a new exchange for a received message.
    fn create_exchange(&self) -> Exchange {
        self.endpoint().create_exchange()
    }
}

/// A consumer that receives messages on demand (Java `PollingConsumer`).
pub trait PollingConsumer: Service {
    /// The endpoint this consumer receives from.
    fn endpoint(&self) -> SharedEndpoint;

    /// Waits until a message is available.
    ///
    /// # Errors
    ///
    /// Returns an error if receiving fails.
    fn receive(&self) -> BoxFuture<'_, CamelResult<Option<Exchange>>>;

    /// Receives a message if one is available right away.
    ///
    /// # Errors
    ///
    /// Returns an error if receiving fails.
    fn receive_no_wait(&self) -> BoxFuture<'_, CamelResult<Option<Exchange>>>;

    /// Waits up to the timeout for a message.
    ///
    /// # Errors
    ///
    /// Returns an error if receiving fails.
    fn receive_timeout(&self, timeout: Duration) -> BoxFuture<'_, CamelResult<Option<Exchange>>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StandaloneContext;
    use crate::processor::{block_on, processor_fn};
    use std::sync::Mutex;

    /// A minimal in-memory endpoint: the producer stores the bodies, the consumer is unused.
    struct MemoryEndpoint {
        uri: String,
        received: Mutex<Vec<String>>,
    }

    impl Service for MemoryEndpoint {}

    impl Endpoint for MemoryEndpoint {
        fn endpoint_uri(&self) -> &str {
            &self.uri
        }

        fn camel_context(&self) -> Arc<dyn CamelContext> {
            StandaloneContext::shared()
        }

        fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
            Ok(Arc::new(MemoryProducer(self)))
        }

        fn create_consumer(
            self: Arc<Self>,
            processor: SharedProcessor,
        ) -> CamelResult<SharedConsumer> {
            Ok(Arc::new(MemoryConsumer(self, processor)))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct MemoryProducer(Arc<MemoryEndpoint>);

    impl Service for MemoryProducer {}

    impl Processor for MemoryProducer {
        fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
            Box::pin(async move {
                let body = exchange.message().body_as::<String>()?.unwrap_or_default();
                self.0.received.lock().unwrap().push(body);
                Ok(())
            })
        }
    }

    impl Producer for MemoryProducer {
        fn endpoint(&self) -> SharedEndpoint {
            self.0.clone()
        }
    }

    struct MemoryConsumer(Arc<MemoryEndpoint>, SharedProcessor);

    impl Service for MemoryConsumer {}

    impl Consumer for MemoryConsumer {
        fn endpoint(&self) -> SharedEndpoint {
            self.0.clone()
        }

        fn processor(&self) -> SharedProcessor {
            self.1.clone()
        }
    }

    struct MemoryComponent;

    impl Service for MemoryComponent {}

    impl Component for MemoryComponent {
        fn create_endpoint(
            &self,
            _context: &Arc<dyn CamelContext>,
            uri: &str,
            _remaining: &str,
            _parameters: &mut Parameters,
        ) -> CamelResult<SharedEndpoint> {
            Ok(Arc::new(MemoryEndpoint {
                uri: uri.to_string(),
                received: Mutex::new(vec![]),
            }))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn endpoint_producer_consumer() {
        let ctx = StandaloneContext::shared();
        let endpoint = MemoryComponent
            .create_endpoint(&ctx, "memory:foo?b=2&a=1", "foo", &mut Parameters::new())
            .unwrap();
        assert_eq!("memory://foo?a=1&b=2", endpoint.endpoint_key());
        assert_eq!("memory:foo", endpoint.endpoint_base_uri());
        assert!(endpoint.is_singleton() && endpoint.is_singleton_producer());
        assert!(!endpoint.is_lenient_properties() && !endpoint.is_remote());
        let producer = endpoint.clone().create_producer().unwrap();
        let mut ex = endpoint.create_exchange();
        assert_eq!(Some("memory:foo?b=2&a=1"), ex.from_endpoint());
        ex.message_mut().set_body("hello");
        block_on(producer.process(&mut ex)).unwrap();
        let mem = endpoint.as_any().downcast_ref::<MemoryEndpoint>().unwrap();
        assert_eq!(vec!["hello".to_string()], *mem.received.lock().unwrap());
        assert!(producer.is_singleton());
        assert_eq!(endpoint.endpoint_uri(), producer.endpoint().endpoint_uri());

        let consumer = endpoint
            .clone()
            .create_consumer(Arc::new(processor_fn(|_| Ok(()))))
            .unwrap();
        assert_eq!(
            ExchangePattern::InOnly,
            consumer.create_exchange().pattern()
        );
        assert_eq!(endpoint.endpoint_uri(), consumer.endpoint().endpoint_uri());
        let _ = consumer.processor();
        let err = endpoint.create_polling_consumer().err().unwrap();
        assert!(err.is(ErrorKind::UnsupportedOperation));
        assert!(!MemoryComponent.use_raw_uri() && MemoryComponent.is_autowired_enabled());
        assert_eq!(None, MemoryComponent.default_name());
    }
}
