//! Aggregation strategies (Java `org.apache.camel.processor.aggregate`).

use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::FAILURE_ENDPOINT;
use camel_api::processor::AggregationStrategy;

/// Uses the latest exchange, propagating the errors and failures of the previous ones (Java
/// `UseLatestAggregationStrategy`).
#[derive(Debug, Default, Clone, Copy)]
pub struct UseLatestAggregationStrategy;

impl AggregationStrategy for UseLatestAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        let Some(mut new) = new_exchange else {
            return Ok(old_exchange);
        };
        let Some(old) = old_exchange else {
            return Ok(Some(new));
        };
        // propagate the exception first
        if new.exception().is_none() {
            new.set_exception(old.exception().cloned());
            match old.property(FAILURE_ENDPOINT).cloned() {
                Some(v) => new.set_property(FAILURE_ENDPOINT, v),
                None => {
                    new.remove_property(FAILURE_ENDPOINT);
                }
            }
        }
        if new.exception().is_some() {
            return Ok(Some(new));
        }
        // then the failures
        let failed = old.is_failed()
            || old.is_rollback_only()
            || old.is_rollback_only_last()
            || old.error_handler_handled() == Some(true);
        Ok(Some(if failed { old } else { new }))
    }
}

/// Keeps the first exchange (Java `UseOriginalAggregationStrategy` without original).
#[derive(Debug, Default, Clone, Copy)]
pub struct UseFirstAggregationStrategy;

impl AggregationStrategy for UseFirstAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        Ok(old_exchange.or(new_exchange))
    }
}
