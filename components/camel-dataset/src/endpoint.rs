//! The `dataset` component (Java `DataSetComponent`, `DataSetEndpoint` and `DataSetConsumer`).

use crate::{assert_equals, data_set_of, SharedDataSet};
use camel_api::context::CamelContext;
use camel_api::endpoint::{Component, Consumer, Endpoint, SharedConsumer, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::DATASET_INDEX;
use camel_api::processor::{BoxFuture, SharedProcessor};
use camel_api::service::Service;
use camel_mock::{MockEndpoint, MockExtension};
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use camel_util::value::Value;
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The handling of the `CamelDataSetIndex` header (Java `dataSetIndex`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSetIndex {
    /// Not set by the consumer, nor checked by the producer.
    Off,
    /// Set by the consumer, checked by the producer when present.
    Lenient,
    /// Set by the consumer, required and checked by the producer.
    Strict,
}

impl std::str::FromStr for DataSetIndex {
    type Err = CamelError;

    fn from_str(s: &str) -> CamelResult<Self> {
        match s {
            "off" => Ok(DataSetIndex::Off),
            "lenient" => Ok(DataSetIndex::Lenient),
            "strict" => Ok(DataSetIndex::Strict),
            _ => Err(CamelError::illegal_argument(format!(
                "Invalid value specified for the dataSetIndex URI parameter:{s}Supported values are strict, lenient and off "
            ))),
        }
    }
}

/// The options of a `dataset` endpoint.
#[derive(Debug, Clone)]
pub struct DataSetConfig {
    /// The minimum rate (accepted).
    pub min_rate: i64,
    /// Millis between the messages sent by the consumer.
    pub produce_delay: u64,
    /// Millis spent per message received by the producer.
    pub consume_delay: u64,
    /// The number of messages sent when the consumer starts.
    pub preload_size: i64,
    /// Millis before the consumer sends the (other) messages.
    pub initial_delay: u64,
    /// The handling of the index header.
    pub data_set_index: DataSetIndex,
}

impl Default for DataSetConfig {
    fn default() -> Self {
        DataSetConfig {
            min_rate: 0,
            produce_delay: 3,
            consume_delay: 0,
            preload_size: 0,
            initial_delay: 1000,
            data_set_index: DataSetIndex::Lenient,
        }
    }
}

/// The `dataset` component.
#[derive(Default)]
pub struct DataSetComponent;

impl DataSetComponent {
    /// Creates the component.
    pub fn new() -> Self {
        DataSetComponent
    }
}

impl Service for DataSetComponent {}

fn millis(parameters: &mut Parameters, name: &str, default: u64) -> CamelResult<u64> {
    match get_and_remove_parameter::<String>(parameters, name)? {
        Some(t) => camel_util::time_utils::to_milli_seconds(&t)
            .map(|m| m.max(0) as u64)
            .map_err(|e| CamelError::illegal_argument(format!("Invalid {name}: {e}"))),
        None => Ok(default),
    }
}

impl Component for DataSetComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let name = remaining.trim_start_matches('/');
        let data_set = context
            .registry()
            .and_then(|r| r.lookup_by_name(name))
            .as_ref()
            .and_then(data_set_of)
            .ok_or_else(|| {
                CamelError::no_such_bean_of_type(
                    Some(name),
                    "org.apache.camel.component.dataset.DataSet",
                )
            })?;
        let d = DataSetConfig::default();
        let config = DataSetConfig {
            min_rate: get_and_remove_parameter(parameters, "minRate")?.unwrap_or(d.min_rate),
            produce_delay: millis(parameters, "produceDelay", d.produce_delay)?,
            consume_delay: millis(parameters, "consumeDelay", d.consume_delay)?,
            preload_size: get_and_remove_parameter(parameters, "preloadSize")?
                .unwrap_or(d.preload_size),
            initial_delay: millis(parameters, "initialDelay", d.initial_delay)?,
            data_set_index: match get_and_remove_parameter::<String>(parameters, "dataSetIndex")? {
                Some(i) => i.parse()?,
                None => d.data_set_index,
            },
        };
        let extension = Arc::new(DataSetExtension {
            data_set,
            config,
            received_counter: AtomicI64::new(0),
        });
        let mock = MockEndpoint::with_extension(uri, name, Arc::downgrade(context), extension);
        if let Some(log) = get_and_remove_parameter::<bool>(parameters, "log")? {
            mock.set_log(log);
        }
        Ok(Arc::new(mock))
    }

    fn default_name(&self) -> Option<&str> {
        Some("dataset")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The dataset behaviour of the mock endpoint of a `dataset` uri (Java `DataSetEndpoint`).
pub struct DataSetExtension {
    data_set: SharedDataSet,
    config: DataSetConfig,
    received_counter: AtomicI64,
}

impl DataSetExtension {
    /// The extension of the mock endpoint, if it is a `dataset` endpoint.
    pub fn of(endpoint: &MockEndpoint) -> Option<&DataSetExtension> {
        endpoint
            .extension()
            .and_then(|e| e.as_any().downcast_ref::<DataSetExtension>())
    }

    /// The dataset.
    pub fn data_set(&self) -> &SharedDataSet {
        &self.data_set
    }

    /// The options.
    pub fn config(&self) -> &DataSetConfig {
        &self.config
    }

    /// The number of messages received by the producers.
    pub fn received_counter(&self) -> i64 {
        self.received_counter.load(Ordering::SeqCst)
    }

    /// The exchange of the index of the dataset (Java `createExchange(messageIndex)`).
    ///
    /// # Errors
    ///
    /// The message cannot be populated.
    pub fn create_exchange(&self, endpoint: &MockEndpoint, index: i64) -> CamelResult<Exchange> {
        let mut e = endpoint.create_exchange();
        self.data_set.populate_message(&mut e, index)?;
        if self.config.data_set_index != DataSetIndex::Off {
            e.message_mut().set_header(DATASET_INDEX, index);
        }
        Ok(e)
    }

    fn index_header(actual: &Exchange) -> Option<i64> {
        actual
            .message()
            .header(DATASET_INDEX)
            .and_then(|v| v.to_string().trim().parse::<i64>().ok())
    }
}

impl MockExtension for DataSetExtension {
    fn on_create_producer(&self, endpoint: &MockEndpoint) {
        endpoint.expected_message_count(self.data_set.size() as i32);
    }

    fn create_consumer(
        &self,
        endpoint: &MockEndpoint,
        processor: SharedProcessor,
    ) -> Option<CamelResult<SharedConsumer>> {
        Some(Ok(Arc::new(DataSetConsumer {
            endpoint: endpoint.clone(),
            processor,
            stopped: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        })))
    }

    fn before_exchange<'a>(
        &'a self,
        _endpoint: &'a MockEndpoint,
        _exchange: &'a mut Exchange,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if self.config.consume_delay > 0 {
                tokio::time::sleep(Duration::from_millis(self.config.consume_delay)).await;
            }
        })
    }

    /// Java `DataSetEndpoint.performAssertions` and `assertMessageExpected`.
    fn perform_assertions(
        &self,
        endpoint: &MockEndpoint,
        actual: &mut Exchange,
    ) -> CamelResult<()> {
        let index = self.received_counter.fetch_add(1, Ordering::SeqCst);
        let expected = self.create_exchange(endpoint, index)?;
        match self.config.data_set_index {
            DataSetIndex::Off => {}
            DataSetIndex::Strict => {
                let actual_index = Self::index_header(actual).ok_or_else(|| {
                    CamelError::runtime(format!(
                        "No '{DATASET_INDEX}' header available of type: java.lang.Long. Exchange[{}]",
                        actual.exchange_id()
                    ))
                })?;
                assert_equals(
                    &format!("Header: {DATASET_INDEX}"),
                    &Value::Int(index),
                    &Value::Int(actual_index),
                    actual,
                )?;
            }
            DataSetIndex::Lenient => match Self::index_header(actual) {
                Some(i) => assert_equals(
                    &format!("Header: {DATASET_INDEX}"),
                    &Value::Int(index),
                    &Value::Int(i),
                    actual,
                )?,
                None => actual.message_mut().set_header(DATASET_INDEX, index),
            },
        }
        self.data_set
            .assert_message_expected(&expected, actual, index)
    }

    fn on_reset(&self) {
        self.received_counter.store(0, Ordering::SeqCst);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The consumer sending the messages of the dataset (Java `DataSetConsumer`).
struct DataSetConsumer {
    endpoint: MockEndpoint,
    processor: SharedProcessor,
    stopped: Arc<AtomicBool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Sends the messages of the indexes; whether it was not stopped.
async fn send_messages(
    endpoint: &MockEndpoint,
    processor: &SharedProcessor,
    stopped: &AtomicBool,
    start: i64,
    end: i64,
) {
    let Some(ext) = DataSetExtension::of(endpoint) else {
        return;
    };
    for i in start..end {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        match ext.create_exchange(endpoint, i) {
            Ok(mut exchange) => {
                if let Err(e) = processor.process(&mut exchange).await {
                    exchange.set_exception(Some(e));
                }
                if let Some(e) = exchange.exception() {
                    log::warn!(
                        "Error processing exchange. Exchange[{}]. Caused by: [{e}]",
                        exchange.exchange_id()
                    );
                }
            }
            Err(e) => log::warn!("Cannot create the dataset message {i}: {e}"),
        }
        if ext.config.produce_delay > 0 {
            tokio::time::sleep(Duration::from_millis(ext.config.produce_delay)).await;
        }
    }
}

impl Service for DataSetConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let ext = DataSetExtension::of(&self.endpoint)
                .ok_or_else(|| CamelError::illegal_state("Not a dataset endpoint"))?;
            let (preload, size, initial_delay) = (
                ext.config.preload_size,
                ext.data_set.size(),
                ext.config.initial_delay,
            );
            self.stopped.store(false, Ordering::SeqCst);
            send_messages(&self.endpoint, &self.processor, &self.stopped, 0, preload).await;
            let (endpoint, processor, stopped) = (
                self.endpoint.clone(),
                self.processor.clone(),
                self.stopped.clone(),
            );
            *self.task.lock().unwrap() = Some(tokio::spawn(async move {
                if initial_delay > 0 {
                    tokio::time::sleep(Duration::from_millis(initial_delay)).await;
                }
                send_messages(&endpoint, &processor, &stopped, preload, size).await;
            }));
            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(t) = self.task.lock().unwrap().take() {
            t.abort();
        }
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Consumer for DataSetConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        Arc::new(self.endpoint.clone())
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}
