//! Receiving messages from endpoints in application code and tests (Java
//! `org.apache.camel.ConsumerTemplate` / `DefaultConsumerTemplate`).

use crate::context::DefaultCamelContext;
use camel_api::context::CamelContext;
use camel_api::endpoint::PollingConsumer;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::spi::synchronization::done_synchronizations;
use camel_util::value::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Receives messages from endpoints by uri with their polling consumers (created and started
/// on first use, cached per endpoint).
///
/// The `receive_*` methods return the exchange: the consumer completes it (such as moving
/// the consumed file) when [`done_uow`](Self::done_uow) is called. The `receive_body*`
/// methods complete it right away and return the body.
pub struct ConsumerTemplate {
    context: Arc<DefaultCamelContext>,
    consumers: Mutex<HashMap<String, Arc<dyn PollingConsumer>>>,
}

impl ConsumerTemplate {
    /// Creates a template for the context.
    pub fn new(context: Arc<DefaultCamelContext>) -> Self {
        ConsumerTemplate {
            context,
            consumers: Mutex::new(HashMap::new()),
        }
    }

    async fn consumer(&self, uri: &str) -> CamelResult<Arc<dyn PollingConsumer>> {
        let endpoint = self.context.endpoint(uri)?;
        let key = endpoint.endpoint_key();
        if let Some(c) = self.consumers.lock().unwrap().get(&key) {
            return Ok(c.clone());
        }
        let consumer = endpoint.create_polling_consumer()?;
        consumer.start().await?;
        self.consumers.lock().unwrap().insert(key, consumer.clone());
        Ok(consumer)
    }

    /// Waits for a message.
    ///
    /// # Errors
    ///
    /// The endpoint cannot be resolved or does not support polling consumers.
    pub async fn receive(&self, uri: &str) -> CamelResult<Option<Exchange>> {
        self.consumer(uri).await?.receive().await
    }

    /// Waits up to the timeout (millis) for a message.
    ///
    /// # Errors
    ///
    /// See [`receive`](Self::receive).
    pub async fn receive_timeout(&self, uri: &str, timeout: u64) -> CamelResult<Option<Exchange>> {
        self.consumer(uri)
            .await?
            .receive_timeout(Duration::from_millis(timeout))
            .await
    }

    /// Receives a message if one is available.
    ///
    /// # Errors
    ///
    /// See [`receive`](Self::receive).
    pub async fn receive_no_wait(&self, uri: &str) -> CamelResult<Option<Exchange>> {
        self.consumer(uri).await?.receive_no_wait().await
    }

    /// Completes the exchange (runs its on completions, Java `doneUoW`).
    pub async fn done_uow(&self, mut exchange: Exchange) {
        let list = exchange.take_on_completions();
        done_synchronizations(&mut exchange, list).await;
    }

    async fn body(&self, exchange: Option<Exchange>) -> Option<Value> {
        let exchange = exchange?;
        let body = exchange.message().body().clone();
        self.done_uow(exchange).await;
        Some(body)
    }

    /// Waits for a message and returns its body.
    ///
    /// # Errors
    ///
    /// See [`receive`](Self::receive).
    pub async fn receive_body(&self, uri: &str) -> CamelResult<Option<Value>> {
        let e = self.receive(uri).await?;
        Ok(self.body(e).await)
    }

    /// Waits up to the timeout (millis) for a message and returns its body.
    ///
    /// # Errors
    ///
    /// See [`receive`](Self::receive).
    pub async fn receive_body_timeout(
        &self,
        uri: &str,
        timeout: u64,
    ) -> CamelResult<Option<Value>> {
        let e = self.receive_timeout(uri, timeout).await?;
        Ok(self.body(e).await)
    }

    /// Returns the body of a message if one is available.
    ///
    /// # Errors
    ///
    /// See [`receive`](Self::receive).
    pub async fn receive_body_no_wait(&self, uri: &str) -> CamelResult<Option<Value>> {
        let e = self.receive_no_wait(uri).await?;
        Ok(self.body(e).await)
    }

    /// Stops the polling consumers.
    ///
    /// # Errors
    ///
    /// The first consumer failing to stop.
    pub async fn stop(&self) -> CamelResult<()> {
        let consumers: Vec<_> = self
            .consumers
            .lock()
            .unwrap()
            .drain()
            .map(|(_, c)| c)
            .collect();
        for c in consumers {
            c.stop().await?;
        }
        Ok(())
    }
}
