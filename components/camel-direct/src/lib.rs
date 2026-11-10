//! # camel-direct
//!
//! The direct component: a synchronous call from a producer to the consumer of a route in the
//! same CamelContext (`direct:name`). The producer runs the consumer route on the calling task,
//! with the same exchange, so the caller sees the result (and any failure) of that route.
//!
//! This crate is the Rust port of the Java `components/camel-direct` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `block` | `true` | If sending a message to a direct endpoint which has no active consumer, then the producer waits for a consumer to become active. |
//! | `timeout` | `30000` | The timeout value (in millis) to use when block is enabled. |
//! | `failIfNoConsumers` | `true` | Whether the producer should fail by throwing an exception, when sending to a direct endpoint with no active consumers. |
//! | `synchronous` | `false` | Whether synchronous processing is forced (accepted for compatibility: the Rust producer always awaits the consumer route). |
//!
//! # Examples
//!
//! ```
//! use camel_api::processor::processor_fn;
//! use camel_base_engine::context::DefaultCamelContext;
//! use camel_base_engine::route::RouteSpec;
//! use camel_direct::DirectComponent;
//! use std::sync::Arc;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = DefaultCamelContext::new();
//! context.add_component("direct", Arc::new(DirectComponent::new())).unwrap();
//! let upper = processor_fn(|ex| {
//!     let body = ex.message().body_as::<String>()?.unwrap_or_default();
//!     ex.message_mut().set_body(body.to_uppercase());
//!     Ok(())
//! });
//! context.add_route(RouteSpec::new("direct:upper", Arc::new(upper))).await.unwrap();
//! context.start().await.unwrap();
//!
//! let template = context.create_producer_template();
//! let reply = template.request_body("direct:upper", "hello").await.unwrap();
//! assert_eq!("HELLO", reply.to_string());
//! context.stop().await.unwrap();
//! # });
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Consumer, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_api::service::{Service, SuspendableService};
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::{sanitize_uri, Parameters};
use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// The Java class name of the error when there is no consumer (its kind is
/// `CamelExchangeException`, as the Java superclass).
pub const DIRECT_CONSUMER_NOT_AVAILABLE: &str =
    "org.apache.camel.component.direct.DirectConsumerNotAvailableException";

/// The active consumers of a component, by endpoint key.
#[derive(Default)]
struct Consumers {
    map: Mutex<HashMap<String, (u64, SharedProcessor)>>,
    state_counter: AtomicU64,
    changed: Notify,
}

impl Consumers {
    fn add(&self, key: &str, id: u64, processor: SharedProcessor) -> CamelResult<()> {
        let mut map = self.map.lock().unwrap();
        if map.contains_key(key) {
            return Err(CamelError::illegal_argument(format!(
                "Cannot add a 2nd consumer to the same endpoint: {key}. DirectEndpoint only allows one consumer."
            )));
        }
        map.insert(key.to_string(), (id, processor));
        self.state_counter.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
        Ok(())
    }

    fn remove(&self, key: &str, id: u64) {
        let mut map = self.map.lock().unwrap();
        if map.get(key).is_some_and(|(i, _)| *i == id) {
            map.remove(key);
        }
        self.state_counter.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    fn get(&self, key: &str) -> Option<SharedProcessor> {
        self.map.lock().unwrap().get(key).map(|(_, p)| p.clone())
    }

    /// The consumer, waiting up to the timeout for one if `block` (only inside a tokio
    /// runtime, as the wait needs its timer).
    async fn wait_for(&self, key: &str, block: bool, timeout: Duration) -> Option<SharedProcessor> {
        if let Some(p) = self.get(key) {
            return Some(p);
        }
        if !block || tokio::runtime::Handle::try_current().is_err() {
            return None;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // register before checking again, so a consumer added meanwhile is not missed
            notified.as_mut().enable();
            if let Some(p) = self.get(key) {
                return Some(p);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let _ = tokio::time::timeout(remaining, notified).await;
        }
    }
}

/// The direct component (Java `DirectComponent`). Each component instance has its own set of
/// consumers, so two CamelContexts do not see each other's direct endpoints.
pub struct DirectComponent {
    consumers: Arc<Consumers>,
    block: AtomicBool,
    timeout: AtomicU64,
}

impl Default for DirectComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl DirectComponent {
    /// Creates the component with the default options.
    pub fn new() -> Self {
        DirectComponent {
            consumers: Arc::new(Consumers::default()),
            block: AtomicBool::new(true),
            timeout: AtomicU64::new(30000),
        }
    }

    /// Whether producers wait for a consumer to become active (default `true`), for the
    /// endpoints created afterwards.
    pub fn set_block(&self, block: bool) {
        self.block.store(block, Ordering::Relaxed);
    }

    /// Whether producers wait for a consumer to become active.
    pub fn is_block(&self) -> bool {
        self.block.load(Ordering::Relaxed)
    }

    /// The timeout in millis to wait for a consumer when block is enabled (default `30000`),
    /// for the endpoints created afterwards.
    pub fn set_timeout(&self, millis: u64) {
        self.timeout.store(millis, Ordering::Relaxed);
    }

    /// The timeout in millis to wait for a consumer.
    pub fn timeout(&self) -> u64 {
        self.timeout.load(Ordering::Relaxed)
    }
}

impl Service for DirectComponent {}

impl Component for DirectComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let endpoint = DirectEndpoint {
            uri: uri.to_string(),
            key: camel_util::string_helper::before(uri, "?").unwrap_or_else(|| uri.to_string()),
            name: remaining.to_string(),
            context: Arc::downgrade(context),
            consumers: self.consumers.clone(),
            block: get_and_remove_parameter(parameters, "block")?.unwrap_or(self.is_block()),
            timeout: get_and_remove_parameter(parameters, "timeout")?.unwrap_or(self.timeout()),
            fail_if_no_consumers: get_and_remove_parameter(parameters, "failIfNoConsumers")?
                .unwrap_or(true),
            synchronous: get_and_remove_parameter(parameters, "synchronous")?.unwrap_or(false),
        };
        Ok(Arc::new(endpoint))
    }

    fn default_name(&self) -> Option<&str> {
        Some("direct")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A direct endpoint (Java `DirectEndpoint`).
pub struct DirectEndpoint {
    uri: String,
    key: String,
    name: String,
    context: Weak<dyn CamelContext>,
    consumers: Arc<Consumers>,
    block: bool,
    timeout: u64,
    fail_if_no_consumers: bool,
    synchronous: bool,
}

impl DirectEndpoint {
    /// The name of the endpoint (the part after `direct:`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether producers wait for a consumer to become active.
    pub fn is_block(&self) -> bool {
        self.block
    }

    /// The timeout in millis to wait for a consumer.
    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    /// Whether producers fail when there is no consumer.
    pub fn is_fail_if_no_consumers(&self) -> bool {
        self.fail_if_no_consumers
    }

    /// Whether synchronous processing is forced.
    pub fn is_synchronous(&self) -> bool {
        self.synchronous
    }

    /// Whether the endpoint has an active consumer.
    pub fn has_consumer(&self) -> bool {
        self.consumers.get(&self.key).is_some()
    }
}

impl Service for DirectEndpoint {}

impl Endpoint for DirectEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(DirectProducer { endpoint: self }))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        static IDS: AtomicU64 = AtomicU64::new(0);
        Ok(Arc::new(DirectConsumer {
            endpoint: self,
            processor,
            id: IDS.fetch_add(1, Ordering::SeqCst),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The direct producer (Java `DirectProducer`): runs the consumer route with the exchange.
pub struct DirectProducer {
    endpoint: Arc<DirectEndpoint>,
}

impl Service for DirectProducer {}

impl Processor for DirectProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let e = &self.endpoint;
            match e
                .consumers
                .wait_for(&e.key, e.block, Duration::from_millis(e.timeout))
                .await
            {
                Some(processor) => processor.process(exchange).await,
                None if e.fail_if_no_consumers => Err(CamelError::camel_exchange(
                    &format!(
                        "No consumers available on endpoint: {}",
                        sanitize_uri(&e.uri)
                    ),
                    exchange,
                )
                .with_subtype(DIRECT_CONSUMER_NOT_AVAILABLE)),
                None => {
                    log::warn!(
                        "No consumers available on endpoint: {} to process: {exchange}",
                        sanitize_uri(&e.uri)
                    );
                    Ok(())
                }
            }
        })
    }
}

impl Producer for DirectProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}

/// The direct consumer (Java `DirectConsumer`): registered with the component while started.
pub struct DirectConsumer {
    endpoint: Arc<DirectEndpoint>,
    processor: SharedProcessor,
    id: u64,
}

impl DirectConsumer {
    fn register(&self) -> CamelResult<()> {
        self.endpoint
            .consumers
            .add(&self.endpoint.key, self.id, self.processor.clone())
    }

    fn unregister(&self) {
        self.endpoint.consumers.remove(&self.endpoint.key, self.id);
    }
}

impl Service for DirectConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.register()))
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.unregister();
        Box::pin(std::future::ready(Ok(())))
    }
}

impl SuspendableService for DirectConsumer {
    fn suspend(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.unregister();
        Box::pin(std::future::ready(Ok(())))
    }

    fn resume(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.register()))
    }
}

impl Consumer for DirectConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}
