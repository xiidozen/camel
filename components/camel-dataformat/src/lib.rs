//! # camel-dataformat
//!
//! The data format component (Java `DataFormatComponent`): `dataformat:name:marshal` and
//! `dataformat:name:unmarshal` marshal or unmarshal the message body with the data format of
//! the name, the endpoint options being the options of the data format (such as
//! `dataformat:base64:marshal?lineLength=64`). This crate is the Rust port of the Java
//! `components/camel-dataformat` module.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_api::service::Service;
use camel_api::spi::data_format::SharedDataFormat;
use camel_core_processor::data_format::{DataFormatOptions, MarshalProcessor, UnmarshalProcessor};
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::{Arc, Weak};

/// The data format component.
#[derive(Default)]
pub struct DataFormatComponent;

impl DataFormatComponent {
    /// Creates the component.
    pub fn new() -> Self {
        DataFormatComponent
    }
}

impl Service for DataFormatComponent {}

impl Component for DataFormatComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let remaining = remaining.trim_start_matches('/');
        let (name, operation) = remaining.split_once(':').unwrap_or((remaining, ""));
        if operation != "marshal" && operation != "unmarshal" {
            return Err(CamelError::illegal_argument(format!(
                "Operation must be either marshal or unmarshal, was: {operation}"
            )));
        }
        // the options of the endpoint are the options of the data format
        let options: Vec<(String, String)> = parameters
            .drain(..)
            .map(|(k, v)| (k, v.to_string()))
            .collect();
        let data_format = context.resolve_data_format(name, &options)?;
        data_format.set_camel_context(Arc::downgrade(context));
        let marshal = operation == "marshal";
        let processor: SharedProcessor = if marshal {
            Arc::new(MarshalProcessor::new(
                data_format.clone(),
                DataFormatOptions::default(),
            ))
        } else {
            Arc::new(UnmarshalProcessor::new(
                data_format.clone(),
                DataFormatOptions::default(),
            ))
        };
        Ok(Arc::new(DataFormatEndpoint {
            uri: uri.to_string(),
            name: name.to_string(),
            marshal,
            context: Arc::downgrade(context),
            data_format,
            processor,
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("dataformat")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The data format endpoint (Java `DataFormatEndpoint`), producer only.
pub struct DataFormatEndpoint {
    uri: String,
    name: String,
    marshal: bool,
    context: Weak<dyn CamelContext>,
    data_format: SharedDataFormat,
    processor: SharedProcessor,
}

impl DataFormatEndpoint {
    /// The name of the data format.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The operation: `marshal` or `unmarshal`.
    pub fn operation(&self) -> &str {
        if self.marshal {
            "marshal"
        } else {
            "unmarshal"
        }
    }

    /// The data format.
    pub fn data_format(&self) -> &SharedDataFormat {
        &self.data_format
    }
}

impl Service for DataFormatEndpoint {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.data_format.start()?;
            self.processor.start_processor().await
        })
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.processor.stop_processor().await?;
            self.data_format.stop()
        })
    }
}

impl Endpoint for DataFormatEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(DataFormatProducer { endpoint: self }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            "Cannot consume from data format",
        ))
    }

    fn is_lenient_properties(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The producer: the marshal or unmarshal processor.
struct DataFormatProducer {
    endpoint: Arc<DataFormatEndpoint>,
}

impl Service for DataFormatProducer {}

impl Processor for DataFormatProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        self.endpoint.processor.process(exchange)
    }
}

impl Producer for DataFormatProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
