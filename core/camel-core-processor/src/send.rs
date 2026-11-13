//! Sending to an endpoint (Java `org.apache.camel.processor.SendProcessor`, the `to` EIP).

use camel_api::endpoint::{SharedEndpoint, SharedProducer};
use camel_api::enums::ExchangePattern;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor};
use camel_api::service::Service;
use camel_api::spi::event::{notify, CamelEvent};
use std::sync::Mutex;
use std::time::Instant;

/// Sends the exchange to an endpoint, using a producer created on first use. An optional
/// exchange pattern is set for the duration of the send.
pub struct SendProcessor {
    endpoint: SharedEndpoint,
    pattern: Option<ExchangePattern>,
    producer: Mutex<Option<SharedProducer>>,
    variable_send: Option<String>,
    variable_receive: Option<String>,
}

impl SendProcessor {
    /// Creates the processor.
    pub fn new(endpoint: SharedEndpoint, pattern: Option<ExchangePattern>) -> Self {
        SendProcessor {
            endpoint,
            pattern,
            producer: Mutex::new(None),
            variable_send: None,
            variable_receive: None,
        }
    }

    /// Sends the value of the variable `send` as body, and stores the reply in the variable
    /// `receive` instead of the message (Java `variableSend` and `variableReceive`).
    pub fn with_variables(mut self, send: Option<String>, receive: Option<String>) -> Self {
        self.variable_send = send;
        self.variable_receive = receive;
        self
    }

    /// The destination endpoint.
    pub fn endpoint(&self) -> &SharedEndpoint {
        &self.endpoint
    }

    /// The producer: the cached one, or a new one, created and started on first use (Java
    /// `LazyStartProducer`).
    async fn producer(&self) -> CamelResult<SharedProducer> {
        let cached = self.producer.lock().unwrap().clone();
        if let Some(existing) = cached {
            return Ok(existing);
        }
        let created = self.endpoint.clone().create_producer()?;
        created.start().await?;
        if self.endpoint.is_singleton_producer() {
            let raced = {
                let mut p = self.producer.lock().unwrap();
                match p.as_ref() {
                    Some(existing) => Some(existing.clone()),
                    None => {
                        *p = Some(created.clone());
                        None
                    }
                }
            };
            if let Some(existing) = raced {
                // another exchange created one meanwhile; keep that one
                let _ = created.stop().await;
                return Ok(existing);
            }
        }
        Ok(created)
    }
}

impl Service for SendProcessor {
    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let producer = self.producer.lock().unwrap().take();
            match producer {
                Some(p) => p.stop().await,
                None => Ok(()),
            }
        })
    }
}

impl Processor for SendProcessor {
    /// Creates and starts the producer with the route (Java `SendProcessor.doStart`).
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            if self.endpoint.is_singleton_producer() {
                self.producer().await?;
            }
            Ok(())
        })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { Service::stop(self).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let producer = self.producer().await?;
            // Java SendProcessor: the last endpoint sent to
            exchange.set_property(
                camel_api::exchange_constants::TO_ENDPOINT,
                self.endpoint.endpoint_uri().to_string(),
            );
            let original = exchange.pattern();
            let pattern = self.pattern.or_else(|| {
                camel_support::endpoint_helper::resolve_exchange_pattern_from_url(
                    self.endpoint.endpoint_uri(),
                )
            });
            if let Some(p) = pattern {
                exchange.set_pattern(p);
            }
            let context = exchange.context().clone();
            let uri = self.endpoint.endpoint_uri().to_string();
            notify(
                context.as_ref(),
                CamelEvent::ExchangeSending {
                    exchange,
                    endpoint: &uri,
                },
            );
            let originals =
                (self.variable_send.is_some() || self.variable_receive.is_some()).then(|| {
                    (
                        exchange.message().body().clone(),
                        exchange.message().headers().clone(),
                    )
                });
            if let Some(v) = &self.variable_send {
                let value = exchange.variable(v).cloned().unwrap_or_default();
                exchange.message_mut().set_body(value);
            }
            let started = Instant::now();
            let r = producer.process(exchange).await;
            exchange.set_pattern(original);
            if r.is_ok()
                && camel_support::exchange_helper::should_set_variable_result(
                    exchange,
                    self.variable_receive.as_deref(),
                )
            {
                let name = self.variable_receive.clone().unwrap_or_default();
                let reply = exchange.message().clone();
                camel_support::exchange_helper::set_variable_from_message_body_and_headers(
                    exchange, &name, &reply,
                );
                if let Some((body, headers)) = originals {
                    exchange.message_mut().set_body(body);
                    exchange.message_mut().set_headers(headers);
                }
            }
            notify(
                context.as_ref(),
                CamelEvent::ExchangeSent {
                    exchange,
                    endpoint: &uri,
                    time_taken: started.elapsed(),
                },
            );
            r
        })
    }
}
