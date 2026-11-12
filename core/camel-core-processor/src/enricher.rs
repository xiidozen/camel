//! The content enrichers (Java `Enricher` and `PollEnricher`): `enrich` sends a copy of the
//! exchange to an endpoint and merges the reply, `pollEnrich` polls a message from an
//! endpoint (such as a seda queue) and merges it. Without an aggregation strategy the
//! result replaces the message.

use crate::dynamic::{recipient, ProducerCache};
use camel_api::enums::ExchangePattern;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{CORRELATION_ID, TO_ENDPOINT};
use camel_api::processor::{BoxFuture, Processor, SharedAggregationStrategy, SharedExpression};
use camel_support::exchange_helper::{copy_results, create_correlated_copy};
use std::sync::Arc;
use std::time::Duration;

/// Copies the results of the source to the target, keeping the correlation id of the target
/// (Java `copyResultsWithoutCorrelationId`).
fn copy_results_without_correlation_id(target: &mut Exchange, source: &Exchange) {
    let correlation = target.remove_property(CORRELATION_ID);
    copy_results(target, source);
    target.remove_property(CORRELATION_ID);
    if let Some(c) = correlation {
        target.set_property(CORRELATION_ID, c);
    }
}

fn aggregation_error(exchange: &Exchange, e: CamelError) -> CamelError {
    CamelError::camel_exchange_cause("Error occurred during aggregation", exchange, e)
}

/// The enrich EIP: sends a copy (`InOut`) of the exchange to the endpoint of the uri
/// expression, then merges the reply into the exchange with the aggregation strategy.
pub struct EnrichProcessor {
    uri: SharedExpression,
    strategy: Option<SharedAggregationStrategy>,
    aggregate_on_exception: bool,
    ignore_invalid_endpoint: bool,
    cache: Arc<ProducerCache>,
}

impl EnrichProcessor {
    /// Creates the processor.
    pub fn new(
        uri: SharedExpression,
        strategy: Option<SharedAggregationStrategy>,
        aggregate_on_exception: bool,
        ignore_invalid_endpoint: bool,
    ) -> Self {
        EnrichProcessor {
            uri,
            strategy,
            aggregate_on_exception,
            ignore_invalid_endpoint,
            cache: Arc::new(ProducerCache::default()),
        }
    }

    async fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let uri = match recipient(&self.uri.evaluate_mut(exchange)?) {
            Some(u) => u,
            None => return Ok(()),
        };
        let mut resource = create_correlated_copy(exchange);
        resource.set_pattern(ExchangePattern::InOut);
        match self.cache.acquire(&resource, &uri).await {
            Ok(target) => crate::dynamic::send_to(&target, None, &mut resource).await,
            Err(e) if self.ignore_invalid_endpoint => {
                log::debug!("Endpoint uri is invalid: {uri}. This exception will be ignored. {e}");
                return Ok(());
            }
            Err(e) => return Err(e),
        }
        if !self.aggregate_on_exception && resource.is_failed() {
            copy_results_without_correlation_id(exchange, &resource);
        } else {
            match &self.strategy {
                None => copy_results_without_correlation_id(exchange, &resource),
                Some(s) => match s.aggregate(Some(exchange.clone()), Some(resource.clone())) {
                    Ok(Some(aggregated)) => {
                        copy_results_without_correlation_id(exchange, &aggregated)
                    }
                    Ok(None) => {}
                    Err(e) => exchange.set_exception(Some(aggregation_error(exchange, e))),
                },
            }
        }
        resource.handover_completions(exchange);
        Ok(())
    }
}

impl Processor for EnrichProcessor {
    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { self.cache.stop().await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(self.run(exchange))
    }
}

/// The pollEnrich EIP: polls a message from the endpoint of the uri expression (waiting
/// without limit for a negative timeout, not at all for `0`, else up to the timeout in
/// millis) and merges it into the exchange with the aggregation strategy. Without a strategy
/// the polled message replaces the message, keeping the headers it does not have; when
/// nothing was polled the body becomes `Null`.
pub struct PollEnrichProcessor {
    uri: SharedExpression,
    timeout: i64,
    strategy: Option<SharedAggregationStrategy>,
    aggregate_on_exception: bool,
    ignore_invalid_endpoint: bool,
}

impl PollEnrichProcessor {
    /// Creates the processor.
    pub fn new(
        uri: SharedExpression,
        timeout: i64,
        strategy: Option<SharedAggregationStrategy>,
        aggregate_on_exception: bool,
        ignore_invalid_endpoint: bool,
    ) -> Self {
        PollEnrichProcessor {
            uri,
            timeout,
            strategy,
            aggregate_on_exception,
            ignore_invalid_endpoint,
        }
    }

    async fn poll(
        &self,
        exchange: &Exchange,
        uri: &str,
    ) -> CamelResult<(String, Option<Exchange>)> {
        let endpoint = exchange.context().endpoint(uri)?;
        let consumer = endpoint.clone().create_polling_consumer()?;
        consumer.start().await?;
        let result = if self.timeout < 0 {
            consumer.receive().await
        } else if self.timeout == 0 {
            consumer.receive_no_wait().await
        } else {
            consumer
                .receive_timeout(Duration::from_millis(self.timeout as u64))
                .await
        };
        let _ = consumer.stop().await;
        let polled = result
            .map_err(|e| CamelError::camel_exchange_cause("Error during poll", exchange, e))?;
        Ok((endpoint.endpoint_uri().to_string(), polled))
    }

    async fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let uri = match recipient(&self.uri.evaluate_mut(exchange)?) {
            Some(u) => u,
            None => return Ok(()),
        };
        let (endpoint_uri, mut resource) = match self.poll(exchange, &uri).await {
            Ok(r) => r,
            Err(e) if self.ignore_invalid_endpoint => {
                log::debug!("Endpoint uri is invalid: {uri}. This exception will be ignored. {e}");
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let failed = resource.as_ref().is_some_and(|r| r.is_failed());
        if !self.aggregate_on_exception && failed {
            if let Some(r) = &resource {
                copy_results(exchange, r);
            }
        } else {
            match &self.strategy {
                None => match &resource {
                    Some(r) => {
                        let original = exchange.message().headers().clone();
                        copy_results(exchange, r);
                        for (k, v) in original.iter() {
                            if exchange.message().header(k).is_none() {
                                exchange.message_mut().set_header(k, v.clone());
                            }
                        }
                    }
                    None => {
                        // nothing polled: an empty body, keeping the headers
                        exchange
                            .in_message_mut()
                            .set_body(camel_util::value::Value::Null);
                        exchange.set_out(None);
                    }
                },
                Some(s) => match s.aggregate(Some(exchange.clone()), resource.clone()) {
                    Ok(Some(aggregated)) => copy_results(exchange, &aggregated),
                    Ok(None) => {}
                    Err(e) => {
                        exchange.set_exception(Some(aggregation_error(exchange, e)));
                        return Ok(());
                    }
                },
            }
            if let Some(r) = resource.as_mut() {
                r.handover_completions(exchange);
            }
        }
        exchange.set_property(TO_ENDPOINT, endpoint_uri);
        Ok(())
    }
}

impl Processor for PollEnrichProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(self.run(exchange))
    }
}
