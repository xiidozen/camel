//! The content based router (Java `org.apache.camel.processor.ChoiceProcessor`).

use crate::filter::FilterProcessor;
use crate::{continue_processing, process_and_capture};
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use std::sync::atomic::{AtomicU64, Ordering};

/// Routes the exchange to the first `when` whose predicate matches, or to `otherwise` if none
/// matches (and if there is no otherwise, the exchange just continues).
pub struct ChoiceProcessor {
    whens: Vec<FilterProcessor>,
    otherwise: Option<SharedProcessor>,
    not_filtered: AtomicU64,
}

impl ChoiceProcessor {
    /// Creates the choice.
    pub fn new(whens: Vec<FilterProcessor>, otherwise: Option<SharedProcessor>) -> Self {
        ChoiceProcessor {
            whens,
            otherwise,
            not_filtered: AtomicU64::new(0),
        }
    }

    /// The number of exchanges routed to otherwise.
    pub fn not_filtered_count(&self) -> u64 {
        self.not_filtered.load(Ordering::Relaxed)
    }

    /// The when clauses.
    pub fn whens(&self) -> &[FilterProcessor] {
        &self.whens
    }
}

impl Processor for ChoiceProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            for when in &self.whens {
                let matches = match when.matches(exchange) {
                    Ok(m) => m,
                    Err(e) => {
                        exchange.set_exception(Some(e));
                        false
                    }
                };
                if !continue_processing(exchange, "so breaking out of choice") {
                    return Ok(());
                }
                if matches {
                    process_and_capture(when.processor().as_ref(), exchange).await;
                    return Ok(());
                }
            }
            if let Some(otherwise) = &self.otherwise {
                self.not_filtered.fetch_add(1, Ordering::Relaxed);
                process_and_capture(otherwise.as_ref(), exchange).await;
            }
            Ok(())
        })
    }
}
