//! The message filter (Java `org.apache.camel.processor.FilterProcessor`).

use crate::process_and_capture;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedPredicate, SharedProcessor};
use std::sync::atomic::{AtomicU64, Ordering};

/// Routes the exchange to the processor only if the predicate matches.
pub struct FilterProcessor {
    predicate: SharedPredicate,
    processor: SharedProcessor,
    status_property_name: Option<String>,
    filtered: AtomicU64,
}

impl FilterProcessor {
    /// Creates the filter.
    pub fn new(predicate: SharedPredicate, processor: SharedProcessor) -> Self {
        FilterProcessor {
            predicate,
            processor,
            status_property_name: None,
            filtered: AtomicU64::new(0),
        }
    }

    /// Sets the name of an exchange property to store whether the predicate matched.
    pub fn with_status_property_name(mut self, name: &str) -> Self {
        self.status_property_name = Some(name.to_string());
        self
    }

    /// Evaluates the predicate (counting the matches).
    ///
    /// # Errors
    ///
    /// Returns the error of the predicate.
    pub fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        let m = self.predicate.matches(exchange)?;
        log::debug!("Filter matches: {m} for exchange: {exchange}");
        if m {
            self.filtered.fetch_add(1, Ordering::Relaxed);
        }
        Ok(m)
    }

    /// The number of exchanges that matched.
    pub fn filtered_count(&self) -> u64 {
        self.filtered.load(Ordering::Relaxed)
    }

    /// The processor of the matched exchanges.
    pub fn processor(&self) -> &SharedProcessor {
        &self.processor
    }
}

impl Processor for FilterProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let matches = match self.matches(exchange) {
                Ok(m) => {
                    if let Some(name) = &self.status_property_name {
                        exchange.set_property(name, m);
                    }
                    m
                }
                Err(e) => {
                    exchange.set_exception(Some(e));
                    false
                }
            };
            if matches {
                process_and_capture(self.processor.as_ref(), exchange).await;
            }
            Ok(())
        })
    }
}
