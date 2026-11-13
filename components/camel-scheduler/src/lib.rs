//! # camel-scheduler
//!
//! The scheduler component (Java `SchedulerComponent`): `scheduler:name` fires an exchange on
//! each poll of a scheduled poll consumer (see
//! `camel_support::scheduled_poll::ScheduledPollConfig` for the
//! scheduling options such as `initialDelay`, `delay`, `repeatCount`, `greedy` and the backoff
//! ones). A route can tell the scheduler that nothing was polled by setting the
//! `CamelSchedulerPolledMessages` property to `false`, which counts for the backoff and the
//! greedy polling. This crate is the Rust port of the Java `components/camel-scheduler`
//! module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `includeMetadata` | `false` | Whether to include metadata in the exchange such as fired time, timer name, timer count etc. |
//! | `poolSize` | `1` | Number of core threads in the thread pool used by the scheduling thread pool (accepted: the polls run on tokio tasks). |
//! | `synchronous` | `false` | Sets whether synchronous processing should be strictly used (accepted). |
//!
//! The consumers are tokio tasks, so the Java shared thread pool per scheduler name has no
//! equivalent.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Consumer, Endpoint, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange_constants::{
    MESSAGE_TIMESTAMP, SCHEDULER_POLLED_MESSAGES, TIMER_FIRED_TIME, TIMER_NAME,
};
use camel_api::processor::{BoxFuture, SharedProcessor};
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_support::scheduled_poll::{PollTask, ScheduledPollConfig, ScheduledPollConsumer};
use camel_util::date::Date;
use camel_util::uri_support::Parameters;
use camel_util::value::Value;
use std::any::Any;
use std::sync::{Arc, Mutex, Weak};

/// The scheduler component.
pub struct SchedulerComponent {
    include_metadata: Mutex<bool>,
    pool_size: Mutex<usize>,
}

impl Default for SchedulerComponent {
    fn default() -> Self {
        SchedulerComponent {
            include_metadata: Mutex::new(false),
            pool_size: Mutex::new(1),
        }
    }
}

impl SchedulerComponent {
    /// Creates the component.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether the endpoints include the metadata by default.
    pub fn set_include_metadata(&self, include: bool) {
        *self.include_metadata.lock().unwrap() = include;
    }

    /// Sets the default pool size (accepted: the polls run on tokio tasks).
    pub fn set_pool_size(&self, size: usize) {
        *self.pool_size.lock().unwrap() = size;
    }

    /// The default pool size.
    pub fn pool_size(&self) -> usize {
        *self.pool_size.lock().unwrap()
    }
}

impl Service for SchedulerComponent {}

impl Component for SchedulerComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let include_metadata = get_and_remove_parameter::<bool>(parameters, "includeMetadata")?
            .unwrap_or(*self.include_metadata.lock().unwrap());
        let pool_size = get_and_remove_parameter::<i64>(parameters, "poolSize")?
            .map_or_else(|| self.pool_size(), |n| n.max(1) as usize);
        let synchronous =
            get_and_remove_parameter::<bool>(parameters, "synchronous")?.unwrap_or(false);
        let config = ScheduledPollConfig::from_parameters(
            context,
            parameters,
            ScheduledPollConfig::default(),
        )?;
        Ok(Arc::new(SchedulerEndpoint {
            uri: uri.to_string(),
            name: remaining.trim_start_matches('/').to_string(),
            context: Arc::downgrade(context),
            include_metadata,
            pool_size,
            synchronous,
            config,
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("scheduler")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The scheduler endpoint (Java `SchedulerEndpoint`), consumer only.
pub struct SchedulerEndpoint {
    uri: String,
    name: String,
    context: Weak<dyn CamelContext>,
    include_metadata: bool,
    pool_size: usize,
    synchronous: bool,
    config: ScheduledPollConfig,
}

impl SchedulerEndpoint {
    /// The name of the scheduler.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the exchanges include the metadata.
    pub fn is_include_metadata(&self) -> bool {
        self.include_metadata
    }

    /// The pool size (accepted).
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    /// Whether synchronous processing is used (accepted).
    pub fn is_synchronous(&self) -> bool {
        self.synchronous
    }

    /// The scheduling options.
    pub fn config(&self) -> &ScheduledPollConfig {
        &self.config
    }
}

impl Service for SchedulerEndpoint {}

impl Endpoint for SchedulerEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            "Scheduler cannot be used as a producer",
        ))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        let task = Arc::new(SchedulerTask {
            endpoint: self.clone(),
            processor: processor.clone(),
        });
        let poller = ScheduledPollConsumer::new(self.config.clone(), task);
        Ok(Arc::new(SchedulerConsumer {
            endpoint: self,
            processor,
            poller,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The poll of the scheduler: one exchange (Java `SchedulerConsumer.sendTimerExchange`).
struct SchedulerTask {
    endpoint: Arc<SchedulerEndpoint>,
    processor: SharedProcessor,
}

impl PollTask for SchedulerTask {
    fn endpoint_uri(&self) -> &str {
        &self.endpoint.uri
    }

    fn poll(&self) -> BoxFuture<'_, CamelResult<usize>> {
        Box::pin(async move {
            let mut exchange = self.endpoint.create_exchange();
            if self.endpoint.include_metadata {
                exchange.set_property(TIMER_NAME, self.endpoint.name.as_str());
                let now = Date::now();
                exchange.set_property(TIMER_FIRED_TIME, now.to_value());
                exchange
                    .message_mut()
                    .set_header(MESSAGE_TIMESTAMP, now.millis());
            }
            log::trace!("Timer {} is firing", self.endpoint.name);
            if let Err(e) = self.processor.process(&mut exchange).await {
                exchange.set_exception(Some(e));
            }
            if let Some(e) = exchange.exception() {
                log::warn!(
                    "Error processing exchange. Exchange[{}]. Caused by: [{e}]",
                    exchange.exchange_id()
                );
            }
            // the route may tell nothing was polled (Java default true)
            let polled = exchange
                .property(SCHEDULER_POLLED_MESSAGES)
                .map_or(true, |v| {
                    !matches!(v, Value::Bool(false)) && !matches!(v, Value::Text(t) if t == "false")
                });
            Ok(usize::from(polled))
        })
    }
}

/// The scheduler consumer (Java `SchedulerConsumer`).
struct SchedulerConsumer {
    endpoint: Arc<SchedulerEndpoint>,
    processor: SharedProcessor,
    poller: ScheduledPollConsumer,
}

impl Service for SchedulerConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.poller.start()))
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.poller.stop();
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Consumer for SchedulerConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}
