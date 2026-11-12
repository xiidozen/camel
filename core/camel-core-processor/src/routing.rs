//! Routing EIPs: the loop (Java `LoopProcessor`), the routing slip and the dynamic router
//! (Java `RoutingSlip`, `DynamicRouter`), sort (Java `SortProcessor`) and step (Java
//! `StepProcessor`).

use crate::dynamic::{recipient, send_to, ProducerCache};
use crate::multicast::split_items;
use crate::pipeline::Pipeline;
use crate::{continue_processing, prepare_out_to_in, process_and_capture};
use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{LOOP_INDEX, LOOP_SIZE, SLIP_ENDPOINT, STEP_ID};
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedExpression, SharedPredicate,
    SharedProcessor,
};
use camel_support::exchange_helper::copy_results;
use camel_util::value::Value;
use std::cmp::Ordering;
use std::sync::Arc;

/// Processes the steps a number of times, or while a predicate matches (the loop EIP).
///
/// Each iteration has the `CamelLoopIndex` property (from 0); with a count the exchange has the
/// `CamelLoopSize` property. With `copy`, each iteration starts from a copy of the original
/// exchange, else from the result of the previous one.
pub struct LoopProcessor {
    count: Option<SharedExpression>,
    while_predicate: Option<SharedPredicate>,
    copy: bool,
    processor: SharedProcessor,
}

impl LoopProcessor {
    /// A loop of `count` iterations.
    pub fn count(count: SharedExpression, processor: SharedProcessor) -> Self {
        LoopProcessor {
            count: Some(count),
            while_predicate: None,
            copy: false,
            processor,
        }
    }

    /// A loop while the predicate matches (Java `loopDoWhile`).
    pub fn do_while(predicate: SharedPredicate, processor: SharedProcessor) -> Self {
        LoopProcessor {
            count: None,
            while_predicate: Some(predicate),
            copy: false,
            processor,
        }
    }

    /// Each iteration starts from a copy of the original exchange (Java `copy()`).
    pub fn with_copy(mut self, copy: bool) -> Self {
        self.copy = copy;
        self
    }

    async fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let count = match &self.count {
            Some(e) => {
                let v = e.evaluate_mut(exchange)?;
                let n = i64::from_value(&v)?.ok_or_else(|| {
                    CamelError::new(
                        ErrorKind::NoTypeConversionAvailable,
                        format!("No type converter available to convert from type: {} to the required type: java.lang.Integer with value {v}", v.type_name()),
                    )
                })?;
                exchange.set_property(LOOP_SIZE, n);
                Some(n)
            }
            None => None,
        };
        let original = if self.copy {
            Some(exchange.clone())
        } else {
            None
        };
        let mut index: i64 = 0;
        loop {
            if !continue_processing(exchange, "so breaking out of loop") {
                break;
            }
            let do_while = match &self.while_predicate {
                Some(p) => p.matches(exchange)?,
                None => true,
            };
            let do_loop = count.map_or(true, |c| index < c);
            if !(do_while && do_loop) {
                break;
            }
            match &original {
                Some(o) => {
                    let mut copy = o.clone();
                    copy.set_property(LOOP_INDEX, index);
                    process_and_capture(self.processor.as_ref(), &mut copy).await;
                    *exchange = copy;
                }
                None => {
                    prepare_out_to_in(exchange);
                    exchange.set_property(LOOP_INDEX, index);
                    process_and_capture(self.processor.as_ref(), exchange).await;
                }
            }
            log::debug!("LoopProcessor: iteration #{index}");
            index += 1;
        }
        if let Some(o) = &original {
            // the result is the last iteration on the original exchange
            let mut result = o.clone();
            copy_results(&mut result, exchange);
            *exchange = result;
        }
        Ok(())
    }
}

impl Processor for LoopProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.processor.start_processor()
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.processor.stop_processor()
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(self.run(exchange))
    }
}

/// The default delimiter of a routing slip.
pub const DEFAULT_SLIP_DELIMITER: &str = ",";

/// Sends the exchange to the endpoints of the slip one after the other, each one receiving the
/// result of the previous one (the routing slip EIP); with `dynamic`, the expression is
/// evaluated again after each endpoint until it answers nothing (the dynamic router EIP).
///
/// The `CamelSlipEndpoint` property is the uri of the last endpoint.
pub struct RoutingSlipProcessor {
    expression: SharedExpression,
    delimiter: String,
    ignore_invalid_endpoints: bool,
    dynamic: bool,
    cache: Arc<ProducerCache>,
}

impl RoutingSlipProcessor {
    /// A routing slip of the expression (the uris separated by the delimiter).
    pub fn new(expression: SharedExpression, delimiter: Option<&str>) -> Self {
        RoutingSlipProcessor {
            expression,
            delimiter: delimiter.unwrap_or(DEFAULT_SLIP_DELIMITER).to_string(),
            ignore_invalid_endpoints: false,
            dynamic: false,
            cache: Arc::new(ProducerCache::default()),
        }
    }

    /// A dynamic router of the expression (evaluated after each endpoint).
    pub fn dynamic_router(expression: SharedExpression, delimiter: Option<&str>) -> Self {
        let mut p = Self::new(expression, delimiter);
        p.dynamic = true;
        p
    }

    /// Whether invalid endpoint uris are skipped instead of failing.
    pub fn with_ignore_invalid_endpoints(mut self, ignore: bool) -> Self {
        self.ignore_invalid_endpoints = ignore;
        self
    }

    fn slip(&self, exchange: &mut Exchange) -> CamelResult<Vec<String>> {
        let value = self.expression.evaluate_mut(exchange)?;
        Ok(split_items(value, &self.delimiter)
            .iter()
            .filter_map(recipient)
            .collect())
    }

    /// Sends to one endpoint; `false` to stop the slip.
    async fn send(&self, exchange: &mut Exchange, uri: &str) -> bool {
        match self.cache.acquire(exchange, uri).await {
            Ok(target) => {
                exchange.set_property(SLIP_ENDPOINT, target.uri());
                send_to(&target, None, exchange).await;
            }
            Err(e) if self.ignore_invalid_endpoints => {
                log::debug!("Endpoint uri is invalid: {uri}. This exception will be ignored. {e}");
                return true;
            }
            Err(e) => {
                exchange.set_exception(Some(e));
                return false;
            }
        }
        continue_processing(exchange, "so breaking out of the routing slip")
    }

    async fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        exchange.remove_property(SLIP_ENDPOINT);
        if !self.dynamic {
            let slip = match self.slip(exchange) {
                Ok(s) => s,
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            for uri in slip {
                if !self.send(exchange, &uri).await {
                    break;
                }
                prepare_out_to_in(exchange);
            }
            return Ok(());
        }
        loop {
            let slip = match self.slip(exchange) {
                Ok(s) => s,
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            if slip.is_empty() {
                return Ok(());
            }
            for uri in slip {
                if !self.send(exchange, &uri).await {
                    return Ok(());
                }
                prepare_out_to_in(exchange);
            }
        }
    }
}

impl Processor for RoutingSlipProcessor {
    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { self.cache.stop().await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(self.run(exchange))
    }
}

/// A comparator of the sort EIP.
pub type SharedComparator = Arc<dyn Fn(&Value, &Value) -> Ordering + Send + Sync>;

/// Sorts the list of the expression (by default the body) and sets it as the body (the sort
/// EIP). Without a comparator, the values are sorted by their natural order (numbers, texts,
/// ...).
pub struct SortProcessor {
    expression: SharedExpression,
    comparator: Option<SharedComparator>,
}

impl SortProcessor {
    /// Creates the processor.
    pub fn new(expression: SharedExpression, comparator: Option<SharedComparator>) -> Self {
        SortProcessor {
            expression,
            comparator,
        }
    }
}

impl Processor for SortProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let result = self.expression.evaluate_mut(exchange).map(|v| {
            let mut list = split_items(v, ",");
            match &self.comparator {
                Some(c) => list.sort_by(|a, b| c(a, b)),
                None => list.sort_by(camel_support::object_helper::type_coerce_compare),
            }
            exchange.message_mut().set_body(Value::List(list));
        });
        Box::pin(std::future::ready(result))
    }
}

/// Runs the steps as a step with an id (the step EIP): the `CamelStepId` property is the id of
/// the step while its steps run.
pub struct StepProcessor {
    id: String,
    pipeline: Pipeline,
}

impl StepProcessor {
    /// Creates the processor.
    pub fn new(id: &str, processors: Vec<SharedProcessor>) -> Self {
        StepProcessor {
            id: id.to_string(),
            pipeline: Pipeline::new(processors),
        }
    }
}

impl Processor for StepProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors(self.pipeline.processors()).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(self.pipeline.processors()).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let old = exchange.remove_property(STEP_ID);
            exchange.set_property(STEP_ID, self.id.as_str());
            let r = self.pipeline.process(exchange).await;
            exchange.remove_property(STEP_ID);
            if let Some(o) = old {
                exchange.set_property(STEP_ID, o);
            }
            r
        })
    }
}

/// Lets one exchange through per number of messages, or per period, stopping the others (the
/// sample EIP, Java `SamplingThrottler`).
pub struct SamplingProcessor {
    message_frequency: u64,
    period: std::time::Duration,
    state: std::sync::Mutex<SampleState>,
}

#[derive(Default)]
struct SampleState {
    count: u64,
    last: Option<std::time::Instant>,
}

impl SamplingProcessor {
    /// Lets one exchange through per period.
    ///
    /// # Errors
    ///
    /// A zero period.
    pub fn per_period(period: std::time::Duration) -> CamelResult<Self> {
        if period.is_zero() {
            return Err(CamelError::illegal_argument(
                "A positive value is required for the sampling period",
            ));
        }
        Ok(SamplingProcessor {
            message_frequency: 0,
            period,
            state: std::sync::Mutex::default(),
        })
    }

    /// Lets one exchange through per number of messages.
    ///
    /// # Errors
    ///
    /// A zero frequency.
    pub fn per_messages(message_frequency: u64) -> CamelResult<Self> {
        if message_frequency == 0 {
            return Err(CamelError::illegal_argument(
                "A positive value is required for the sampling message frequency",
            ));
        }
        Ok(SamplingProcessor {
            message_frequency,
            period: std::time::Duration::ZERO,
            state: std::sync::Mutex::default(),
        })
    }
}

impl Processor for SamplingProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let send = {
            let mut s = self.state.lock().unwrap();
            if self.message_frequency > 0 {
                s.count += 1;
                s.count % self.message_frequency == 0
            } else {
                let now = std::time::Instant::now();
                match s.last {
                    Some(last) if now < last + self.period => false,
                    _ => {
                        s.last = Some(now);
                        true
                    }
                }
            }
        };
        if !send {
            exchange.set_route_stop(true);
        }
        Box::pin(std::future::ready(Ok(())))
    }
}
