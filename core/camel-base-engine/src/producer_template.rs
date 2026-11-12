//! Sending messages to endpoints from application code and tests (Java
//! `org.apache.camel.ProducerTemplate` / `DefaultProducerTemplate`).

use crate::context::DefaultCamelContext;
use camel_api::context::CamelContext;
use camel_api::endpoint::SharedProducer;
use camel_api::enums::ExchangePattern;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::spi::event::{notify, CamelEvent};
use camel_util::value::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Sends messages to endpoints by uri. The producers are created and started on first use and
/// cached per endpoint.
///
/// The `send_*` methods are fire and forget (`InOnly`), the `request_*` methods wait for the
/// reply (`InOut`) and return its body. Both fail with a `CamelExecutionException` wrapping
/// the error of a failed exchange.
pub struct ProducerTemplate {
    context: Arc<DefaultCamelContext>,
    producers: Mutex<HashMap<String, SharedProducer>>,
}

impl ProducerTemplate {
    /// Creates a template for the context.
    pub fn new(context: Arc<DefaultCamelContext>) -> Self {
        ProducerTemplate {
            context,
            producers: Mutex::new(HashMap::new()),
        }
    }

    async fn producer(&self, uri: &str) -> CamelResult<SharedProducer> {
        let endpoint = self.context.endpoint(uri)?;
        let key = endpoint.endpoint_key();
        if let Some(p) = self.producers.lock().unwrap().get(&key) {
            return Ok(p.clone());
        }
        let producer = endpoint.clone().create_producer()?;
        producer.start().await?;
        if endpoint.is_singleton_producer() {
            self.producers.lock().unwrap().insert(key, producer.clone());
        }
        Ok(producer)
    }

    /// Creates an exchange for the endpoint (Java `endpoint.createExchange(pattern)`).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be resolved.
    pub fn create_exchange(&self, uri: &str, pattern: ExchangePattern) -> CamelResult<Exchange> {
        Ok(self
            .context
            .endpoint(uri)?
            .create_exchange_with_pattern(pattern))
    }

    /// Sends the exchange to the endpoint and returns it; a failure is set on the returned
    /// exchange (as in Java `send(uri, exchange)`).
    ///
    /// # Errors
    ///
    /// Returns an error only if the endpoint or its producer cannot be created.
    pub async fn send(&self, uri: &str, mut exchange: Exchange) -> CamelResult<Exchange> {
        let producer = self.producer(uri).await?;
        let endpoint = producer.endpoint().endpoint_uri().to_string();
        let context = self.context.as_shared();
        notify(
            context.as_ref(),
            CamelEvent::ExchangeSending {
                exchange: &exchange,
                endpoint: &endpoint,
            },
        );
        let started = Instant::now();
        if let Err(e) = producer.process(&mut exchange).await {
            exchange.set_exception(Some(e));
        }
        notify(
            context.as_ref(),
            CamelEvent::ExchangeSent {
                exchange: &exchange,
                endpoint: &endpoint,
                time_taken: started.elapsed(),
            },
        );
        Ok(exchange)
    }

    async fn send_with(
        &self,
        uri: &str,
        pattern: ExchangePattern,
        body: Value,
        headers: Vec<(String, Value)>,
    ) -> CamelResult<Exchange> {
        let mut exchange = self.create_exchange(uri, pattern)?;
        exchange.message_mut().set_body(body);
        for (k, v) in headers {
            exchange.message_mut().set_header(&k, v);
        }
        let exchange = self.send(uri, exchange).await?;
        if let Some(e) = exchange.exception() {
            return Err(CamelError::wrap_camel_execution(Some(&exchange), e.clone()));
        }
        Ok(exchange)
    }

    /// Sends the body (`InOnly`).
    ///
    /// # Errors
    ///
    /// Returns a `CamelExecutionException` if the exchange failed.
    pub async fn send_body(&self, uri: &str, body: impl Into<Value>) -> CamelResult<()> {
        self.send_with(uri, ExchangePattern::InOnly, body.into(), vec![])
            .await
            .map(|_| ())
    }

    /// Sends the body with a header (`InOnly`).
    ///
    /// # Errors
    ///
    /// Returns a `CamelExecutionException` if the exchange failed.
    pub async fn send_body_and_header(
        &self,
        uri: &str,
        body: impl Into<Value>,
        name: &str,
        value: impl Into<Value>,
    ) -> CamelResult<()> {
        self.send_with(
            uri,
            ExchangePattern::InOnly,
            body.into(),
            vec![(name.to_string(), value.into())],
        )
        .await
        .map(|_| ())
    }

    /// Sends the body with the headers (`InOnly`).
    ///
    /// # Errors
    ///
    /// Returns a `CamelExecutionException` if the exchange failed.
    pub async fn send_body_and_headers(
        &self,
        uri: &str,
        body: impl Into<Value>,
        headers: Vec<(String, Value)>,
    ) -> CamelResult<()> {
        self.send_with(uri, ExchangePattern::InOnly, body.into(), headers)
            .await
            .map(|_| ())
    }

    /// Sends the body and returns the reply body (`InOut`): the out message body if there is
    /// one, else the in message body (Java `ExchangeHelper.extractResultBody`).
    ///
    /// # Errors
    ///
    /// Returns a `CamelExecutionException` if the exchange failed.
    pub async fn request_body(&self, uri: &str, body: impl Into<Value>) -> CamelResult<Value> {
        let exchange = self
            .send_with(uri, ExchangePattern::InOut, body.into(), vec![])
            .await?;
        Ok(result_body(&exchange))
    }

    /// Sends the body with a header and returns the reply body (`InOut`).
    ///
    /// # Errors
    ///
    /// Returns a `CamelExecutionException` if the exchange failed.
    pub async fn request_body_and_header(
        &self,
        uri: &str,
        body: impl Into<Value>,
        name: &str,
        value: impl Into<Value>,
    ) -> CamelResult<Value> {
        let exchange = self
            .send_with(
                uri,
                ExchangePattern::InOut,
                body.into(),
                vec![(name.to_string(), value.into())],
            )
            .await?;
        Ok(result_body(&exchange))
    }

    /// Stops the cached producers.
    ///
    /// # Errors
    ///
    /// Returns the first failure.
    pub async fn stop(&self) -> CamelResult<()> {
        let producers: Vec<SharedProducer> = self
            .producers
            .lock()
            .unwrap()
            .drain()
            .map(|(_, p)| p)
            .collect();
        let mut first = None;
        for p in producers {
            if let Err(e) = p.stop().await {
                first.get_or_insert(e);
            }
        }
        first.map_or(Ok(()), Err)
    }
}

/// The result body of the exchange: the out body if out capable and present, else the in body.
pub fn result_body(exchange: &Exchange) -> Value {
    match exchange.out_message() {
        Some(out) if exchange.pattern().is_out_capable() => out.body().clone(),
        _ => exchange.in_message().body().clone(),
    }
}
