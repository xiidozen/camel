//! Multicast and splitter (Java `MulticastProcessor` and `Splitter`): each copy of the exchange
//! is processed in sequence, and the results are aggregated.
//!
//! Not ported yet: parallel processing, streaming, timeouts, share unit of work, the
//! aggregation on completion callbacks.

use crate::{continue_processing, process_and_capture};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    CORRELATION_ID, EXCEPTION_CAUGHT, SPLIT_COMPLETE, SPLIT_INDEX, SPLIT_SIZE,
};
use camel_api::processor::{
    BoxFuture, Processor, SharedAggregationStrategy, SharedExpression, SharedProcessor,
};
use camel_support::exchange_helper::{copy_results, create_correlated_copy};
use camel_util::value::Value;

/// How the results of the copies are aggregated.
#[derive(Clone)]
enum Aggregation {
    /// A strategy (Java multicast without strategy: use the latest result).
    Strategy(Option<SharedAggregationStrategy>),
    /// Keep the original exchange, propagating the errors of the copies (Java
    /// `UseOriginalAggregationStrategy` of the splitter).
    UseOriginal,
}

/// Processes the copies in sequence (Java `MulticastProcessor` sequential processing and
/// `doDone`).
async fn process_sequential(
    original: &mut Exchange,
    pairs: Vec<(SharedProcessor, Exchange)>,
    aggregation: &Aggregation,
    stop_on_exception: bool,
) -> CamelResult<()> {
    let mut result: Option<Exchange> = None;
    for (index, (processor, mut sub)) in pairs.into_iter().enumerate() {
        process_and_capture(processor.as_ref(), &mut sub).await;
        let msg = format!("Multicast processing failed for number {index}");
        if !continue_processing(&sub, &msg) && stop_on_exception {
            if let Some(e) = sub.exception().cloned() {
                let wrapped = CamelError::camel_exchange_cause(&msg, &sub, e);
                original.set_exception(Some(wrapped));
            }
            original.set_redelivery_exhausted(
                sub.exception().is_some() || sub.is_redelivery_exhausted(),
            );
            return Ok(());
        }
        match aggregation {
            Aggregation::UseOriginal => {
                if let Some(e) = sub.exception() {
                    original.set_exception(Some(e.clone()));
                }
                if let Some(c) = sub.property(EXCEPTION_CAUGHT).cloned() {
                    original.set_property(EXCEPTION_CAUGHT, c);
                }
            }
            Aggregation::Strategy(Some(s)) => {
                result = s.aggregate_with_input(result, Some(sub), original)?;
            }
            Aggregation::Strategy(None) => result = Some(sub),
        }
    }
    if let Some(r) = result {
        if r.exception().is_some() && stop_on_exception {
            original.set_exception(r.exception().cloned());
        } else {
            let correlation = original.remove_property(CORRELATION_ID);
            copy_results(original, &r);
            match correlation {
                Some(c) => original.set_property(CORRELATION_ID, c),
                None => {
                    original.remove_property(CORRELATION_ID);
                }
            }
        }
    }
    Ok(())
}

/// Sends a copy of the exchange to each processor (the multicast EIP).
pub struct MulticastProcessor {
    processors: Vec<SharedProcessor>,
    strategy: Option<SharedAggregationStrategy>,
    stop_on_exception: bool,
}

impl MulticastProcessor {
    /// Creates the processor; without strategy the result of the last copy is used.
    pub fn new(
        processors: Vec<SharedProcessor>,
        strategy: Option<SharedAggregationStrategy>,
        stop_on_exception: bool,
    ) -> Self {
        MulticastProcessor {
            processors,
            strategy,
            stop_on_exception,
        }
    }
}

impl Processor for MulticastProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let pairs: Vec<(SharedProcessor, Exchange)> = self
                .processors
                .iter()
                .map(|p| (p.clone(), create_correlated_copy(exchange)))
                .collect();
            process_sequential(
                exchange,
                pairs,
                &Aggregation::Strategy(self.strategy.clone()),
                self.stop_on_exception,
            )
            .await
        })
    }
}

/// The items to split a value into (Java `ObjectHelper.createIterator` with the delimiter):
/// a list, the parts of a text separated by the delimiter (empty parts skipped), the values of
/// a map, or the value itself.
pub fn split_items(value: Value, delimiter: &str) -> Vec<Value> {
    match value {
        Value::Null => vec![],
        Value::List(l) => l,
        Value::Map(m) => m.into_values().collect(),
        Value::Text(t) => {
            if delimiter.is_empty() || !t.contains(delimiter) {
                if t.is_empty() {
                    vec![]
                } else {
                    vec![Value::Text(t)]
                }
            } else {
                t.split(delimiter)
                    .filter(|p| !p.is_empty())
                    .map(Value::from)
                    .collect()
            }
        }
        other => vec![other],
    }
}

/// Splits the message into parts and processes each part (the split EIP).
pub struct SplitProcessor {
    expression: SharedExpression,
    processor: SharedProcessor,
    strategy: Option<SharedAggregationStrategy>,
    stop_on_exception: bool,
    delimiter: String,
}

impl SplitProcessor {
    /// Creates the processor; without strategy the original exchange is kept (with the errors
    /// of the parts).
    pub fn new(
        expression: SharedExpression,
        processor: SharedProcessor,
        strategy: Option<SharedAggregationStrategy>,
        stop_on_exception: bool,
    ) -> Self {
        SplitProcessor {
            expression,
            processor,
            strategy,
            stop_on_exception,
            delimiter: ",".to_string(),
        }
    }

    /// Sets the delimiter of texts (default `,`).
    pub fn with_delimiter(mut self, delimiter: &str) -> Self {
        self.delimiter = delimiter.to_string();
        self
    }
}

impl Processor for SplitProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let value = match self.expression.evaluate(exchange) {
                Ok(v) => v,
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            let items = split_items(value, &self.delimiter);
            let size = items.len();
            let pairs: Vec<(SharedProcessor, Exchange)> = items
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    let mut sub = create_correlated_copy(exchange);
                    sub.set_out(None);
                    sub.message_mut().set_body(item);
                    sub.set_property(SPLIT_INDEX, i as i64);
                    sub.set_property(SPLIT_SIZE, size as i64);
                    sub.set_property(SPLIT_COMPLETE, i + 1 == size);
                    (self.processor.clone(), sub)
                })
                .collect();
            let aggregation = match &self.strategy {
                Some(s) => Aggregation::Strategy(Some(s.clone())),
                None => Aggregation::UseOriginal,
            };
            process_sequential(exchange, pairs, &aggregation, self.stop_on_exception).await
        })
    }
}

/// Delays the exchange by the number of millis of the expression (the delay EIP); a `Null`
/// or not positive delay does not wait.
pub struct DelayProcessor {
    expression: SharedExpression,
}

impl DelayProcessor {
    /// Creates the processor.
    pub fn new(expression: SharedExpression) -> Self {
        DelayProcessor { expression }
    }
}

impl Processor for DelayProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let v = self.expression.evaluate(exchange)?;
            let delay = <i64 as camel_api::convert::FromValue>::from_value(&v)?.unwrap_or(0);
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
            }
            Ok(())
        })
    }
}
