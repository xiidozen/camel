//! The bean component (Java `BeanComponent`, `BeanEndpoint`, `BeanProducer`):
//! `bean:beanName?method=methodName` calls the method of the bean in the registry, the result
//! becoming the message body.
//!
//! Options: `method` (the method to call, default the only method of the bean), `scope`
//! (accepted, no effect: see the [bean language](crate::language)). The `bean.*` options
//! (setting properties of the bean) are not available: Rust beans have no setters found by
//! reflection.

use crate::processor::BeanProcessor;
use camel_api::bean_binding::BeanReference;
use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::{Arc, Weak};

/// The bean component.
///
/// # Examples
///
/// ```
/// use camel_bean::MethodBean;
/// use camel_core::prelude::*;
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// let context = camel_core::default_context();
/// let bean = MethodBean::new("Greeter").method("hello", |inv| {
///     Ok(Value::from(format!("Hello {}", inv.parameter(0))))
/// });
/// context.registry().unwrap().bind("greeter", bean.into_value()).unwrap();
/// context.start().await.unwrap();
/// let template = context.create_producer_template();
/// let reply = template.request_body("bean:greeter?method=hello", "World").await.unwrap();
/// assert_eq!(Value::from("Hello World"), reply);
/// context.stop().await.unwrap();
/// # });
/// ```
#[derive(Debug, Default)]
pub struct BeanComponent;

impl BeanComponent {
    /// The component.
    pub fn new() -> Self {
        BeanComponent
    }
}

impl Service for BeanComponent {}

impl Component for BeanComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let method: Option<String> = get_and_remove_parameter(parameters, "method")?;
        let _scope: Option<String> = get_and_remove_parameter(parameters, "scope")?;
        Ok(Arc::new(BeanEndpoint {
            uri: uri.to_string(),
            bean_name: remaining.to_string(),
            method,
            context: Arc::downgrade(context),
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("bean")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A bean endpoint (Java `BeanEndpoint`): producers only.
pub struct BeanEndpoint {
    uri: String,
    bean_name: String,
    method: Option<String>,
    context: Weak<dyn CamelContext>,
}

impl BeanEndpoint {
    /// The name of the bean in the registry.
    pub fn bean_name(&self) -> &str {
        &self.bean_name
    }

    /// The method to call, if set.
    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }
}

impl Service for BeanEndpoint {}

impl Endpoint for BeanEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        let processor = BeanProcessor::new(
            BeanReference::Name(self.bean_name.clone()),
            self.method.as_deref(),
        );
        Ok(Arc::new(BeanProducer {
            endpoint: self,
            processor,
        }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            format!("You cannot consume from a bean endpoint: {}", self.uri),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The bean producer (Java `BeanProducer`).
pub struct BeanProducer {
    endpoint: Arc<BeanEndpoint>,
    processor: BeanProcessor,
}

impl Service for BeanProducer {}

impl Processor for BeanProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        self.processor.process(exchange)
    }
}

impl Producer for BeanProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
