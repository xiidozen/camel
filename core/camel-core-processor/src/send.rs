//! Sending to an endpoint (Java `org.apache.camel.processor.SendProcessor`, the `to` EIP).

use camel_api::endpoint::{SharedEndpoint, SharedProducer};
use camel_api::enums::ExchangePattern;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor};
use std::sync::Mutex;

/// Sends the exchange to an endpoint, using a producer created on first use. An optional
/// exchange pattern is set for the duration of the send.
pub struct SendProcessor {
    endpoint: SharedEndpoint,
    pattern: Option<ExchangePattern>,
    producer: Mutex<Option<SharedProducer>>,
}

impl SendProcessor {
    /// Creates the processor.
    pub fn new(endpoint: SharedEndpoint, pattern: Option<ExchangePattern>) -> Self {
        SendProcessor {
            endpoint,
            pattern,
            producer: Mutex::new(None),
        }
    }

    /// The destination endpoint.
    pub fn endpoint(&self) -> &SharedEndpoint {
        &self.endpoint
    }

    fn producer(&self) -> CamelResult<SharedProducer> {
        let mut p = self.producer.lock().unwrap();
        if let Some(existing) = p.as_ref() {
            return Ok(existing.clone());
        }
        let created = self.endpoint.clone().create_producer()?;
        if self.endpoint.is_singleton_producer() {
            *p = Some(created.clone());
        }
        Ok(created)
    }
}

impl Processor for SendProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let producer = self.producer()?;
            let original = exchange.pattern();
            if let Some(p) = self.pattern {
                exchange.set_pattern(p);
            }
            let r = producer.process(exchange).await;
            exchange.set_pattern(original);
            r
        })
    }
}
