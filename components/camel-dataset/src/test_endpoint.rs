//! The `dataset-test` component (Java `DataSetTestComponent` and `DataSetTestEndpoint`): a
//! mock endpoint expecting the messages polled from another endpoint.

use camel_api::context::CamelContext;
use camel_api::endpoint::{Component, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::BoxFuture;
use camel_api::service::Service;
use camel_mock::{MockEndpoint, MockExtension};
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use camel_util::value::Value;
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

/// The `dataset-test` component.
#[derive(Default)]
pub struct DataSetTestComponent;

impl DataSetTestComponent {
    /// Creates the component.
    pub fn new() -> Self {
        DataSetTestComponent
    }
}

impl Service for DataSetTestComponent {}

impl Component for DataSetTestComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let timeout = match get_and_remove_parameter::<String>(parameters, "timeout")? {
            Some(t) => camel_util::time_utils::to_milli_seconds(&t)
                .map(|m| m.max(0) as u64)
                .map_err(|e| CamelError::illegal_argument(format!("Invalid timeout: {e}")))?,
            None => 2000,
        };
        let any_order = get_and_remove_parameter::<bool>(parameters, "anyOrder")?.unwrap_or(false);
        let split = get_and_remove_parameter::<bool>(parameters, "split")?.unwrap_or(false);
        let delimiter = get_and_remove_parameter::<String>(parameters, "delimiter")?
            .unwrap_or_else(|| "\\n|\\r".to_string());
        let delimiter = regex::Regex::new(&delimiter).map_err(|e| {
            CamelError::illegal_argument(format!("Invalid delimiter: {delimiter}: {e}"))
        })?;
        let log = get_and_remove_parameter::<bool>(parameters, "log")?;
        // the other parameters are the ones of the endpoint of the expected messages
        let expected_uri = if parameters.is_empty() {
            remaining.to_string()
        } else {
            let u = camel_util::uri_support::append_parameters_to_uri(remaining, parameters)
                .map_err(|e| CamelError::illegal_argument(e.to_string()))?;
            parameters.clear();
            u
        };
        let expected_endpoint = context.endpoint(&expected_uri)?;
        let extension = Arc::new(DataSetTestExtension {
            expected_endpoint,
            timeout,
            any_order,
            split,
            delimiter,
            expected: Arc::new(OnceCell::new()),
        });
        let mock = MockEndpoint::with_extension(uri, remaining, Arc::downgrade(context), extension);
        if let Some(log) = log {
            mock.set_log(log);
        }
        Ok(Arc::new(mock))
    }

    fn default_name(&self) -> Option<&str> {
        Some("dataset-test")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The behaviour of a `dataset-test` mock endpoint: the expected bodies are polled from the
/// endpoint when the first producer is created, and set before the first message is checked.
pub struct DataSetTestExtension {
    expected_endpoint: SharedEndpoint,
    timeout: u64,
    any_order: bool,
    split: bool,
    delimiter: regex::Regex,
    expected: Arc<OnceCell<Vec<Value>>>,
}

impl DataSetTestExtension {
    /// The extension of the mock endpoint, if it is a `dataset-test` endpoint.
    pub fn of(endpoint: &MockEndpoint) -> Option<&DataSetTestExtension> {
        endpoint
            .extension()
            .and_then(|e| e.as_any().downcast_ref::<DataSetTestExtension>())
    }

    /// The bodies polled from the endpoint (Java `EndpointHelper.pollEndpoint`), until a poll
    /// gets nothing within the timeout.
    async fn poll(&self) -> Vec<Value> {
        let mut bodies = Vec::new();
        let consumer = match camel_support::polling_consumer::polling_consumer_of(
            self.expected_endpoint.clone(),
        ) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "Cannot poll the expected messages of {}: {e}",
                    self.expected_endpoint.endpoint_uri()
                );
                return bodies;
            }
        };
        if let Err(e) = consumer.start().await {
            log::warn!("Cannot poll the expected messages: {e}");
            return bodies;
        }
        loop {
            match consumer
                .receive_timeout(Duration::from_millis(self.timeout))
                .await
            {
                Ok(Some(exchange)) => {
                    let body = exchange.message().body().clone();
                    if self.split {
                        let text = match &body {
                            Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                            other => other.to_string(),
                        };
                        for line in self.delimiter.split(&text).filter(|l| !l.is_empty()) {
                            bodies.push(Value::from(line));
                        }
                    } else {
                        bodies.push(body);
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::warn!("Error polling the expected messages: {e}");
                    break;
                }
            }
        }
        let _ = consumer.stop().await;
        log::info!(
            "Received: {} expected message(s) from: {}",
            bodies.len(),
            self.expected_endpoint.endpoint_uri()
        );
        bodies
    }

    fn set_expectations(&self, endpoint: &MockEndpoint, bodies: &[Value]) {
        if self.any_order {
            endpoint.expected_bodies_received_in_any_order(bodies.to_vec());
        } else {
            endpoint.expected_bodies_received(bodies.to_vec());
        }
    }
}

impl MockExtension for DataSetTestExtension {
    fn on_start_producer<'a>(&'a self, endpoint: &'a MockEndpoint) -> BoxFuture<'a, ()> {
        // Java polls when the endpoint starts; here when its producer starts
        Box::pin(async move {
            if !self.expected.initialized() {
                let bodies = self.poll().await;
                self.set_expectations(endpoint, &bodies);
                let _ = self.expected.set(bodies);
            }
        })
    }

    fn before_exchange<'a>(
        &'a self,
        endpoint: &'a MockEndpoint,
        _exchange: &'a mut Exchange,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if !self.expected.initialized() {
                let bodies = self.poll().await;
                self.set_expectations(endpoint, &bodies);
                let _ = self.expected.set(bodies);
            }
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
