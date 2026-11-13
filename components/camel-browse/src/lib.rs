//! # camel-browse
//!
//! The browse component (Java `BrowseComponent`): `browse:name` keeps the exchanges sent to it
//! so they can be browsed (for testing, debugging or monitoring), and passes them on to the
//! route consuming from the endpoint, if any. This crate is the Rust port of the Java
//! `components/camel-browse` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `browseLimit` | `0` | Maximum number of messages to keep in memory available for browsing. Use 0 for unlimited. |
//! | `filter` | | To use a predicate (`#name` of a bean, see [`filter_value`]) to filter whether to include the message for browsing. Return true to include, false to exclude. |
//!
//! # Examples
//!
//! ```
//! use camel_browse::BrowseEndpoint;
//! use camel_core::prelude::*;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = camel_core::default_context();
//! context.start().await.unwrap();
//! let template = context.create_producer_template();
//! template.send_body("browse:foo", "Hello World").await.unwrap();
//! let endpoint = context.endpoint("browse:foo").unwrap();
//! let browse = endpoint.as_any().downcast_ref::<BrowseEndpoint>().unwrap();
//! assert_eq!("Hello World", browse.exchanges()[0].message().body().to_string());
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
use camel_api::processor::{BoxFuture, Processor, SharedPredicate, SharedProcessor};
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use camel_util::value::{ObjectRef, Value};
use std::any::Any;
use std::sync::{Arc, Mutex, RwLock, Weak};

/// The predicate as a value, to bind it in the registry for the `filter=#name` option.
pub fn filter_value(filter: SharedPredicate) -> Value {
    Value::Object(ObjectRef::new(filter))
}

/// The browse component.
#[derive(Default)]
pub struct BrowseComponent {
    browse_limit: Mutex<usize>,
}

impl BrowseComponent {
    /// Creates the component.
    pub fn new() -> Self {
        Self::default()
    }

    /// The default maximum number of messages kept by the endpoints (0: unlimited).
    pub fn browse_limit(&self) -> usize {
        *self.browse_limit.lock().unwrap()
    }

    /// Sets the default maximum number of messages kept by the endpoints (0: unlimited).
    pub fn set_browse_limit(&self, limit: usize) {
        *self.browse_limit.lock().unwrap() = limit;
    }
}

impl Service for BrowseComponent {}

impl Component for BrowseComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let browse_limit = get_and_remove_parameter::<i64>(parameters, "browseLimit")?
            .map_or_else(|| self.browse_limit(), |l| l.max(0) as usize);
        let filter = match get_and_remove_parameter::<String>(parameters, "filter")? {
            Some(f) => {
                let name = f.strip_prefix('#').unwrap_or(&f);
                let name = name.strip_prefix("bean:").unwrap_or(name);
                let found = context
                    .registry()
                    .and_then(|r| r.lookup_by_name(name))
                    .and_then(|v| match v {
                        Value::Object(o) => o.downcast_ref::<SharedPredicate>().cloned(),
                        _ => None,
                    });
                Some(found.ok_or_else(|| {
                    CamelError::no_such_bean_of_type(Some(name), "java.util.function.Predicate")
                })?)
            }
            None => None,
        };
        Ok(Arc::new(BrowseEndpoint {
            uri: uri.to_string(),
            name: remaining.trim_start_matches('/').to_string(),
            context: Arc::downgrade(context),
            browse_limit,
            filter,
            exchanges: Mutex::new(Vec::new()),
            consumer: RwLock::new(None),
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("browse")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The browse endpoint (Java `BrowseEndpoint`): keeps copies of the exchanges it receives.
pub struct BrowseEndpoint {
    uri: String,
    name: String,
    context: Weak<dyn CamelContext>,
    browse_limit: usize,
    filter: Option<SharedPredicate>,
    exchanges: Mutex<Vec<Exchange>>,
    consumer: RwLock<Option<SharedProcessor>>,
}

impl BrowseEndpoint {
    /// The name of the endpoint.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The maximum number of messages kept (0: unlimited).
    pub fn browse_limit(&self) -> usize {
        self.browse_limit
    }

    /// The kept exchanges, the oldest first (Java `getExchanges`).
    pub fn exchanges(&self) -> Vec<Exchange> {
        self.exchanges.lock().unwrap().clone()
    }

    /// Keeps the exchange, dropping the oldest ones beyond the limit (Java `onExchange`).
    fn keep(&self, exchange: &Exchange) {
        let mut list = self.exchanges.lock().unwrap();
        if self.browse_limit > 0 && list.len() >= self.browse_limit {
            let excess = list.len() + 1 - self.browse_limit;
            list.drain(..excess);
        }
        list.push(exchange.clone());
    }
}

impl Service for BrowseEndpoint {
    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.exchanges.lock().unwrap().clear();
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Endpoint for BrowseEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(BrowseProducer { endpoint: self }))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        *self.consumer.write().unwrap() = Some(processor.clone());
        Ok(Arc::new(BrowseConsumer {
            endpoint: self,
            processor,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The browse producer: keeps the accepted exchanges, then routes them to the consumer.
struct BrowseProducer {
    endpoint: Arc<BrowseEndpoint>,
}

impl Service for BrowseProducer {}

impl Processor for BrowseProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let accept = match &self.endpoint.filter {
                Some(f) => f.matches(exchange)?,
                None => true,
            };
            if accept {
                self.endpoint.keep(exchange);
                let consumer = self.endpoint.consumer.read().unwrap().clone();
                if let Some(c) = consumer {
                    c.process(exchange).await?;
                }
            }
            Ok(())
        })
    }
}

impl Producer for BrowseProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}

/// The browse consumer: the exchanges are routed by the producer.
struct BrowseConsumer {
    endpoint: Arc<BrowseEndpoint>,
    processor: SharedProcessor,
}

impl Service for BrowseConsumer {
    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        let mut c = self.endpoint.consumer.write().unwrap();
        if c.as_ref().is_some_and(|p| Arc::ptr_eq(p, &self.processor)) {
            *c = None;
        }
        Box::pin(std::future::ready(Ok(())))
    }

    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        *self.endpoint.consumer.write().unwrap() = Some(self.processor.clone());
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Consumer for BrowseConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}
