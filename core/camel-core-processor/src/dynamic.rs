//! Sending to endpoints computed per exchange: `toD` (Java `SendDynamicProcessor`), the
//! recipient list (Java `RecipientList`), the wire tap (Java `WireTapProcessor`), and
//! `convertBodyTo` (Java `ConvertBodyProcessor`).

use crate::multicast::{multicast_pairs, process_pairs, split_items, Aggregation, MulticastConfig};
use camel_api::endpoint::SharedProducer;
use camel_api::enums::ExchangePattern;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::TO_ENDPOINT;
use camel_api::processor::{
    BoxFuture, Processor, SharedAggregationStrategy, SharedExpression, SharedProcessor,
};
use camel_support::exchange_helper::create_correlated_copy;
use camel_util::value::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Producers cached by endpoint key, created and started on first use (Java `ProducerCache`).
#[derive(Default)]
pub struct ProducerCache {
    producers: Mutex<HashMap<String, SharedProducer>>,
}

/// An endpoint with its started producer, ready to send to.
#[derive(Clone)]
pub struct Target {
    uri: String,
    producer: SharedProducer,
}

impl ProducerCache {
    /// Resolves the endpoint of the uri and acquires its producer (started on first use).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint or its producer cannot be created or started.
    pub async fn acquire(&self, exchange: &Exchange, uri: &str) -> CamelResult<Target> {
        let endpoint = exchange.context().endpoint(uri)?;
        let key = endpoint.endpoint_key();
        let cached = self.producers.lock().unwrap().get(&key).cloned();
        let producer = match cached {
            Some(p) => p,
            None => {
                let p = endpoint.clone().create_producer()?;
                p.start().await?;
                if endpoint.is_singleton_producer() {
                    self.producers.lock().unwrap().insert(key, p.clone());
                }
                p
            }
        };
        Ok(Target {
            uri: endpoint.endpoint_uri().to_string(),
            producer,
        })
    }

    /// Stops the cached producers (Java: stopping the `ProducerCache`).
    ///
    /// # Errors
    ///
    /// Returns the first error, after stopping all of them.
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

    /// Sends the exchange to the endpoint of the uri; any failure is set on the exchange.
    pub async fn send(&self, uri: &str, pattern: Option<ExchangePattern>, exchange: &mut Exchange) {
        match self.acquire(exchange, uri).await {
            Ok(target) => send_to(&target, pattern, exchange).await,
            Err(e) => exchange.set_exception(Some(e)),
        }
    }
}

/// Sends the exchange to the target, with the pattern if given; a failure is set on the
/// exchange.
pub async fn send_to(target: &Target, pattern: Option<ExchangePattern>, exchange: &mut Exchange) {
    exchange.set_property(TO_ENDPOINT, target.uri.clone());
    let original = exchange.pattern();
    if let Some(p) = pattern {
        exchange.set_pattern(p);
    }
    let r = target.producer.process(exchange).await;
    exchange.set_pattern(original);
    if let Err(e) = r {
        exchange.set_exception(Some(e));
    }
}

/// The recipient text of a value: trimmed, `None` when empty (Java `prepareRecipient`).
fn recipient(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let s = value.to_string().trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Sends to the endpoint of the uri expression (the toD EIP); an empty uri sends nothing.
pub struct SendDynamicProcessor {
    uri: SharedExpression,
    pattern: Option<ExchangePattern>,
    ignore_invalid_endpoint: bool,
    cache: Arc<ProducerCache>,
}

impl SendDynamicProcessor {
    /// Creates the processor.
    pub fn new(
        uri: SharedExpression,
        pattern: Option<ExchangePattern>,
        ignore_invalid_endpoint: bool,
    ) -> Self {
        SendDynamicProcessor {
            uri,
            pattern,
            ignore_invalid_endpoint,
            cache: Arc::new(ProducerCache::default()),
        }
    }
}

impl Processor for SendDynamicProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { self.cache.stop().await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let uri = match self.uri.evaluate(exchange) {
                Ok(v) => recipient(&v),
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            let Some(uri) = uri else {
                log::debug!("Send dynamic evaluated as null so cannot send to any endpoint");
                return Ok(());
            };
            match self.cache.acquire(exchange, &uri).await {
                Ok(target) => send_to(&target, self.pattern, exchange).await,
                Err(e) if self.ignore_invalid_endpoint => {
                    log::debug!(
                        "Endpoint uri is invalid: {uri}. This exception will be ignored. {e}"
                    );
                }
                Err(e) => exchange.set_exception(Some(e)),
            }
            Ok(())
        })
    }
}

/// Sends to a resolved target (one recipient of a recipient list).
struct SendToTarget(Target);

impl Processor for SendToTarget {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            send_to(&self.0, None, exchange).await;
            Ok(())
        })
    }
}

/// The delimiter value that disables splitting of the recipients (Java
/// `IGNORE_DELIMITER_MARKER`).
pub const IGNORE_DELIMITER_MARKER: &str = "false";

/// Sends a copy to each endpoint of the expression (the recipient list EIP); without strategy
/// the last reply is used. All endpoints are resolved before any is sent to.
pub struct RecipientListProcessor {
    expression: SharedExpression,
    delimiter: String,
    strategy: Option<SharedAggregationStrategy>,
    config: MulticastConfig,
    ignore_invalid_endpoints: bool,
    cache: Arc<ProducerCache>,
}

impl RecipientListProcessor {
    /// Creates the processor.
    pub fn new(
        expression: SharedExpression,
        strategy: Option<SharedAggregationStrategy>,
        config: MulticastConfig,
        ignore_invalid_endpoints: bool,
    ) -> Self {
        RecipientListProcessor {
            expression,
            delimiter: ",".to_string(),
            strategy,
            config,
            ignore_invalid_endpoints,
            cache: Arc::new(ProducerCache::default()),
        }
    }

    /// Sets the delimiter of the uris in a text (default `,`; `false` to not split).
    pub fn with_delimiter(mut self, delimiter: &str) -> Self {
        self.delimiter = delimiter.to_string();
        self
    }
}

impl Processor for RecipientListProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { self.cache.stop().await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let value = match self.expression.evaluate(exchange) {
                Ok(v) => v,
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            let items = if self.delimiter.eq_ignore_ascii_case(IGNORE_DELIMITER_MARKER) {
                vec![value]
            } else {
                split_items(value, &self.delimiter)
            };
            let mut targets: Vec<SharedProcessor> = Vec::new();
            for uri in items.iter().filter_map(recipient) {
                match self.cache.acquire(exchange, &uri).await {
                    Ok(target) => targets.push(Arc::new(SendToTarget(target))),
                    Err(e) if self.ignore_invalid_endpoints => {
                        log::debug!(
                            "Endpoint uri is invalid: {uri}. This exception will be ignored. {e}"
                        );
                    }
                    Err(e) => {
                        // failure so break out
                        exchange.set_exception(Some(e));
                        return Ok(());
                    }
                }
            }
            let pairs = multicast_pairs(exchange, targets);
            process_pairs(
                exchange,
                pairs,
                &Aggregation::Strategy(self.strategy.clone()),
                &self.config,
            )
            .await
        })
    }
}

/// Sends a copy of the exchange (as `InOnly`) to the endpoint of the uri expression in the
/// background (the wire tap EIP); the exchange continues right away. The uri is evaluated on
/// the copy, as Java does.
pub struct WireTapProcessor {
    send: Arc<SendDynamicProcessor>,
}

impl WireTapProcessor {
    /// Creates the processor.
    pub fn new(uri: SharedExpression, ignore_invalid_endpoint: bool) -> Self {
        WireTapProcessor {
            send: Arc::new(SendDynamicProcessor::new(
                uri,
                None,
                ignore_invalid_endpoint,
            )),
        }
    }
}

impl Processor for WireTapProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { self.send.stop_processor().await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            if tokio::runtime::Handle::try_current().is_err() {
                return Err(CamelError::illegal_state(
                    "The wire tap must run within a tokio runtime",
                ));
            }
            let mut copy = create_correlated_copy(exchange);
            copy.set_pattern(ExchangePattern::InOnly);
            let send = self.send.clone();
            tokio::spawn(async move {
                if let Err(e) = send.process(&mut copy).await {
                    copy.set_exception(Some(e));
                }
                if let Some(e) = copy.exception() {
                    log::warn!("Error processing wire-tapped exchange: {copy}. Caused by: {e}");
                }
            });
            Ok(())
        })
    }
}

/// Sets the exchange pattern (the setExchangePattern EIP).
pub struct SetExchangePatternProcessor(pub ExchangePattern);

impl Processor for SetExchangePatternProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        exchange.set_pattern(self.0);
        Box::pin(std::future::ready(Ok(())))
    }
}

/// Converts the body to the Java type name (the convertBodyTo EIP). A mandatory conversion
/// that fails gives an `InvalidPayloadException` (with the conversion error as cause); a not
/// mandatory one sets a `Null` body.
pub struct ConvertBodyProcessor {
    type_name: String,
    mandatory: bool,
}

impl ConvertBodyProcessor {
    /// Creates the processor.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown type name (Java: the class cannot be loaded).
    pub fn new(type_name: &str, mandatory: bool) -> CamelResult<Self> {
        camel_support::type_conversion::convert_to_type(&Value::Null, type_name)?;
        Ok(ConvertBodyProcessor {
            type_name: type_name.to_string(),
            mandatory,
        })
    }
}

impl Processor for ConvertBodyProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let body = exchange.message().body().clone();
        if body.is_null() || exchange.exception().is_some() {
            return Box::pin(std::future::ready(Ok(())));
        }
        let converted = camel_support::type_conversion::convert_to_type(&body, &self.type_name);
        let value = match converted {
            Ok(v) if !v.is_null() || !self.mandatory => v,
            Err(_) if !self.mandatory => Value::Null,
            r => {
                let prefix = format!(
                    "No body available of type: {} but has type: {} on: Message[{}]",
                    self.type_name,
                    body.type_name(),
                    exchange.message().message_id().unwrap_or_default()
                );
                let e = match r {
                    Err(cause) => CamelError::with_cause(
                        ErrorKind::InvalidPayload,
                        format!(
                            "{prefix}. Caused by: {cause}. Exchange[{}]",
                            exchange.exchange_id()
                        ),
                        cause,
                    ),
                    Ok(_) => CamelError::new(
                        ErrorKind::InvalidPayload,
                        format!("{prefix}. Exchange[{}]", exchange.exchange_id()),
                    ),
                };
                exchange.set_exception(Some(e));
                return Box::pin(std::future::ready(Ok(())));
            }
        };
        exchange.message_mut().set_body(value);
        Box::pin(std::future::ready(Ok(())))
    }
}
