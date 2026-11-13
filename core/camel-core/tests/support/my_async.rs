//! The `async:` test component (Java `MyAsyncComponent`, `MyAsyncEndpoint` and
//! `MyAsyncProducer` of the async tests): the producer replies after a delay (25 ms by
//! default) with the words of the uri path capitalized (`async:bye:camel` replies `Bye
//! Camel`), appended to the body with `append=true`; the first `failFirstAttempts` exchanges
//! fail with `Simulated error at attempt N`.

use camel_core::api::context::{CamelContext, StandaloneContext};
use camel_core::api::endpoint::{
    Component, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_core::api::error::{CamelError, CamelResult};
use camel_core::api::exchange::Exchange;
use camel_core::api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_core::api::service::Service;
use camel_core::prelude::ModelCamelContext;
use camel_util::uri_support::Parameters;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

/// The `async` component.
pub struct MyAsyncComponent;

impl Service for MyAsyncComponent {}

fn flag(parameters: &mut Parameters, name: &str) -> bool {
    parameters
        .shift_remove(name)
        .and_then(|v| v.as_str().map(|s| s.eq_ignore_ascii_case("true")))
        .unwrap_or(false)
}

fn number(parameters: &mut Parameters, name: &str, default: u64) -> CamelResult<u64> {
    match parameters.shift_remove(name) {
        Some(v) => v
            .as_str()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| CamelError::illegal_argument(format!("Invalid {name}"))),
        None => Ok(default),
    }
}

impl Component for MyAsyncComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        // the words separated by ':' capitalized (Java prepareReply)
        let reply = remaining
            .split(':')
            .map(camel_util::string_helper::capitalize)
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Arc::new(MyAsyncEndpoint {
            uri: uri.to_string(),
            context: Arc::downgrade(context),
            reply,
            append: flag(parameters, "append"),
            delay: number(parameters, "delay", 25)?,
            fail_first_attempts: number(parameters, "failFirstAttempts", 0)? as usize,
            synchronous: flag(parameters, "synchronous"),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The endpoint of the `async` component.
pub struct MyAsyncEndpoint {
    uri: String,
    context: Weak<dyn CamelContext>,
    reply: String,
    append: bool,
    delay: u64,
    fail_first_attempts: usize,
    synchronous: bool,
}

impl MyAsyncEndpoint {
    /// Whether the producer is synchronous.
    pub fn is_synchronous(&self) -> bool {
        self.synchronous
    }
}

impl Service for MyAsyncEndpoint {}

impl Endpoint for MyAsyncEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(MyAsyncProducer {
            endpoint: self,
            counter: AtomicUsize::new(0),
        }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::runtime("Consumer not supported"))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The producer of the `async` component.
pub struct MyAsyncProducer {
    endpoint: Arc<MyAsyncEndpoint>,
    counter: AtomicUsize,
}

impl Service for MyAsyncProducer {}

impl Processor for MyAsyncProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let e = &self.endpoint;
            if e.synchronous {
                std::thread::sleep(Duration::from_millis(e.delay));
            } else {
                tokio::time::sleep(Duration::from_millis(e.delay)).await;
            }
            let count = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
            if e.fail_first_attempts >= count {
                return Err(CamelError::camel_exchange(
                    &format!("Simulated error at attempt {count}"),
                    exchange,
                ));
            }
            let reply = if e.append {
                format!("{} {}", exchange.message().body(), e.reply)
            } else {
                e.reply.clone()
            };
            exchange.message_mut().set_body(reply);
            Ok(())
        })
    }
}

impl Producer for MyAsyncProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}

/// A started context with the `async` component and the routes.
pub async fn async_context(
    routes: impl camel_core::prelude::RouteBuilder,
) -> super::ContextTestSupport {
    let t = super::ContextTestSupport::new();
    t.context
        .add_component("async", Arc::new(MyAsyncComponent))
        .expect("the async component");
    t.context
        .add_routes(routes)
        .await
        .expect("the routes of the test");
    t.context.start().await.expect("the context starts");
    t
}
