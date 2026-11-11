//! Error handling of routes (Java `org.apache.camel.processor.errorhandler`): the default
//! error handler and the dead letter channel, with redelivery and `onException` policies
//! (Java `RedeliveryErrorHandler`, `DefaultErrorHandler`, `DeadLetterChannel`).
//!
//! The reifier wraps each step of a route in a [`RedeliveryErrorHandler`] (except the steps
//! of `doTry` blocks and `onException` clauses, as in Java).

pub mod exception_policy;
pub mod redelivery_policy;

use crate::log_processor::to_log_level;
use crate::prepare_out_to_in;
use crate::try_catch::caught_value;
use camel_api::enums::LoggingLevel;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    EXCEPTION_CAUGHT, EXCEPTION_HANDLED, FAILURE_ENDPOINT, FAILURE_ROUTE_ID, REDELIVERED,
    REDELIVERY_COUNTER, REDELIVERY_DELAY, REDELIVERY_MAX_COUNTER, TO_ENDPOINT,
};
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedPredicate, SharedProcessor,
};
use exception_policy::{find_exception_policy, ExceptionPolicy};
use redelivery_policy::RedeliveryPolicy;
use std::sync::Arc;
use std::time::Duration;

/// The dead letter of a dead letter channel.
#[derive(Clone)]
pub struct DeadLetter {
    /// The dead letter endpoint uri (for the log messages).
    pub uri: String,
    /// The processor sending to the dead letter endpoint.
    pub processor: SharedProcessor,
    /// Whether a new error while sending to the dead letter endpoint is handled (default
    /// `true`).
    pub handle_new_exception: bool,
}

/// The configuration of an error handler, shared by the error handlers of all the steps of a
/// route.
#[derive(Clone)]
pub struct ErrorHandlerConfig {
    /// The dead letter (a dead letter channel), or `None` (the default error handler).
    pub dead_letter: Option<DeadLetter>,
    /// The redelivery policy.
    pub redelivery_policy: RedeliveryPolicy,
    /// The `onException` policies.
    pub exception_policies: Vec<Arc<ExceptionPolicy>>,
    /// Redeliver while this predicate matches.
    pub retry_while: Option<SharedPredicate>,
    /// A processor called before each redelivery.
    pub on_redelivery: Option<SharedProcessor>,
    /// A processor called when an error occurs.
    pub on_exception_occurred: Option<SharedProcessor>,
    /// A processor called before the failure processor (dead letter) gets the exchange.
    pub on_prepare_failure: Option<SharedProcessor>,
    /// The logger name (the `log` target).
    pub logger_name: String,
    /// The level of the logger (`OFF` disables the logging).
    pub logger_level: LoggingLevel,
    /// Whether the failure processor gets the original in message (as it was when the unit
    /// of work started) instead of the current one.
    pub use_original_message: bool,
    /// Whether the failure processor gets the original in message body.
    pub use_original_body: bool,
}

impl Default for ErrorHandlerConfig {
    fn default() -> Self {
        ErrorHandlerConfig {
            dead_letter: None,
            redelivery_policy: RedeliveryPolicy::default(),
            exception_policies: Vec::new(),
            retry_while: None,
            on_redelivery: None,
            on_exception_occurred: None,
            on_prepare_failure: None,
            logger_name: "org.apache.camel.processor.errorhandler.DefaultErrorHandler".to_string(),
            logger_level: LoggingLevel::ERROR,
            use_original_message: false,
            use_original_body: false,
        }
    }
}

impl ErrorHandlerConfig {
    /// The configuration of a dead letter channel to the processor (sending to the uri).
    pub fn dead_letter_channel(uri: &str, processor: SharedProcessor) -> Self {
        ErrorHandlerConfig {
            dead_letter: Some(DeadLetter {
                uri: uri.to_string(),
                processor,
                handle_new_exception: true,
            }),
            logger_name: "org.apache.camel.processor.errorhandler.DeadLetterChannel".to_string(),
            ..Default::default()
        }
    }

    /// Whether redelivery may happen (Java `redeliveryEnabled`).
    fn redelivery_enabled(&self) -> bool {
        self.redelivery_policy.maximum_redeliveries != 0
            || self.retry_while.is_some()
            || self.exception_policies.iter().any(|p| {
                p.retry_while.is_some() || p.redelivery.maximum_redeliveries.is_some_and(|m| m != 0)
            })
    }
}

/// Java `ExchangeHelper.logIds`: `(MessageId: x on ExchangeId: y)`.
pub fn log_ids(exchange: &Exchange) -> String {
    let message_id = exchange
        .message()
        .message_id()
        .unwrap_or(exchange.exchange_id())
        .to_string();
    format!(
        "(MessageId: {message_id} on ExchangeId: {})",
        exchange.exchange_id()
    )
}

/// The error handler of one step: runs the step, redelivers it on failure according to the
/// policies, then hands the exchange to the failure processor (the `onException` block or the
/// dead letter).
pub struct RedeliveryErrorHandler {
    output: SharedProcessor,
    config: Arc<ErrorHandlerConfig>,
}

/// The state of the handling of one exchange (Java `RedeliveryTask`).
struct Task<'a> {
    config: &'a ErrorHandlerConfig,
    original: Option<Exchange>,
    redelivery_counter: i32,
    redelivery_delay: u64,
    policy: RedeliveryPolicy,
    retry_while: Option<SharedPredicate>,
    failure_processor: Option<SharedProcessor>,
    handled: Option<SharedPredicate>,
    continued: Option<SharedPredicate>,
    on_redelivery: Option<SharedProcessor>,
    on_exception_occurred: Option<SharedProcessor>,
    use_original_message: bool,
    use_original_body: bool,
}

impl<'a> Task<'a> {
    fn new(config: &'a ErrorHandlerConfig, exchange: &Exchange) -> Self {
        let mut t = Task {
            config,
            original: if config.redelivery_enabled() {
                Some(exchange.clone())
            } else {
                None
            },
            redelivery_counter: 0,
            redelivery_delay: 0,
            policy: RedeliveryPolicy::default(),
            retry_while: None,
            failure_processor: None,
            handled: None,
            continued: None,
            on_redelivery: None,
            on_exception_occurred: None,
            use_original_message: false,
            use_original_body: false,
        };
        t.use_error_handler_defaults();
        t
    }

    fn use_error_handler_defaults(&mut self) {
        self.policy = self.config.redelivery_policy.clone();
        self.retry_while = self.config.retry_while.clone();
        self.failure_processor = None;
        self.handled = None;
        self.continued = None;
        self.on_redelivery = self.config.on_redelivery.clone();
        self.on_exception_occurred = self.config.on_exception_occurred.clone();
        self.use_original_message = self.config.use_original_message;
        self.use_original_body = self.config.use_original_body;
    }

    fn is_dead_letter_channel(&self) -> bool {
        self.config.dead_letter.is_some()
    }

    /// Java `handleException`: records the error, selects the exception policy, logs the
    /// attempt and counts it.
    fn handle_exception(&mut self, exchange: &mut Exchange) {
        let Some(e) = exchange.exception().cloned() else {
            return;
        };
        exchange.set_property(EXCEPTION_CAUGHT, caught_value(&e));
        self.use_error_handler_defaults();
        if let Some(p) = find_exception_policy(&self.config.exception_policies, exchange, &e) {
            self.policy = p.redelivery.apply(&self.policy);
            self.handled = p.handled.clone();
            self.continued = p.continued.clone();
            self.use_original_message = p.use_original_message;
            self.use_original_body = p.use_original_body;
            if p.retry_while.is_some() {
                self.retry_while = p.retry_while.clone();
            }
            if let Some(pr) = &p.processor {
                self.failure_processor = Some(pr.clone());
            }
            if let Some(pr) = &p.on_redelivery {
                self.on_redelivery = Some(pr.clone());
            }
            if let Some(pr) = &p.on_exception_occurred {
                self.on_exception_occurred = Some(pr.clone());
            }
        }
        if let Some(id) = exchange.from_route_id().map(str::to_string) {
            exchange.set_property(FAILURE_ROUTE_ID, id);
        }
        if !exchange.is_failure_handled() {
            let msg = format!(
                "Failed delivery for {}. On delivery attempt: {} caught: {e}",
                log_ids(exchange),
                self.redelivery_counter
            );
            self.log_failed_delivery(true, false, false, false, exchange, &msg, Some(&e));
        }
        self.redelivery_counter = self.increment_redelivery_counter(exchange);
    }

    fn increment_redelivery_counter(&self, exchange: &mut Exchange) -> i32 {
        let counter = exchange
            .in_message()
            .header_as::<i64>(REDELIVERY_COUNTER)
            .ok()
            .flatten();
        let base = counter
            .map(|c| (c as i32).max(self.redelivery_counter))
            .unwrap_or(self.redelivery_counter);
        let next = base + 1;
        exchange
            .in_message_mut()
            .set_header(REDELIVERY_COUNTER, next as i64);
        exchange.in_message_mut().set_header(REDELIVERED, true);
        exchange.set_redelivery_counter(next);
        if self.policy.maximum_redeliveries > 0 {
            exchange.in_message_mut().set_header(
                REDELIVERY_MAX_COUNTER,
                self.policy.maximum_redeliveries as i64,
            );
            exchange.set_redelivery_max_counter(self.policy.maximum_redeliveries);
        }
        next
    }

    fn decrement_redelivery_counter(&self, exchange: &mut Exchange) {
        let counter = exchange
            .in_message()
            .header_as::<i64>(REDELIVERY_COUNTER)
            .ok()
            .flatten();
        let base = counter
            .map(|c| (c as i32).max(self.redelivery_counter))
            .unwrap_or(self.redelivery_counter);
        let prev = (base - 1).max(0);
        exchange
            .in_message_mut()
            .set_header(REDELIVERY_COUNTER, prev as i64);
        exchange.in_message_mut().set_header(REDELIVERED, prev > 0);
    }

    async fn on_exception_occurred(&self, exchange: &mut Exchange) {
        if let Some(p) = &self.on_exception_occurred {
            if let Err(e) = p.process(exchange).await {
                log::warn!(
                    "Error during processing OnExceptionOccurred. This exception is ignored. {e}"
                );
            }
        }
    }

    fn prepare_for_redelivery(&self, exchange: &mut Exchange) {
        exchange.set_exception(None);
        exchange.set_rollback_only(false);
        if let Some(original) = &self.original {
            exchange.set_in(original.in_message().clone());
        }
        exchange.set_out(None);
        if self.redelivery_counter > 0 {
            exchange
                .in_message_mut()
                .set_header(REDELIVERY_COUNTER, self.redelivery_counter as i64);
            exchange.in_message_mut().set_header(REDELIVERED, true);
            exchange.set_redelivery_counter(self.redelivery_counter);
            if self.policy.maximum_redeliveries > 0 {
                exchange.in_message_mut().set_header(
                    REDELIVERY_MAX_COUNTER,
                    self.policy.maximum_redeliveries as i64,
                );
                exchange.set_redelivery_max_counter(self.policy.maximum_redeliveries);
            }
        }
    }

    fn matches(p: &Option<SharedPredicate>, exchange: &Exchange, name: &str) -> bool {
        match p {
            None => false,
            Some(p) => p.matches(exchange).unwrap_or_else(|e| {
                log::warn!("Error evaluating {name} predicate: {e}. This exception is ignored.");
                false
            }),
        }
    }

    /// Java `deliverToFailureProcessor`.
    async fn deliver_to_failure_processor(&mut self, exchange: &mut Exchange) {
        let caught = exchange.exception().cloned();
        exchange.set_exception(None);
        let should_handle = Self::matches(&self.handled, exchange, "handled");
        let should_continue = Self::matches(&self.continued, exchange, "continued");
        let (processor, is_dlc) = match &self.failure_processor {
            Some(p) => (Some(p.clone()), false),
            None => (
                self.config
                    .dead_letter
                    .as_ref()
                    .map(|d| d.processor.clone()),
                self.is_dead_letter_channel(),
            ),
        };
        let handle_or_continue = is_dlc || should_handle || should_continue;
        if handle_or_continue {
            for h in [REDELIVERED, REDELIVERY_COUNTER, REDELIVERY_MAX_COUNTER] {
                exchange.in_message_mut().remove_header(h);
            }
            exchange.set_redelivery_exhausted(false);
            exchange.set_redelivery_counter(-1);
            exchange.set_redelivery_max_counter(-1);
            exchange.set_rollback_only(false);
        } else {
            self.decrement_redelivery_counter(exchange);
        }
        let allow_failure_processor = !should_continue || !is_dlc;
        if let Some(to) = exchange.property(TO_ENDPOINT).cloned() {
            exchange.set_property(FAILURE_ENDPOINT, to);
        }
        if let Some(id) = exchange.from_route_id().map(str::to_string) {
            exchange.set_property(FAILURE_ROUTE_ID, id);
        }
        let mut suffix = String::new();
        match (&processor, allow_failure_processor) {
            (Some(p), true) => {
                self.prepare_original(exchange);
                if let Some(prepare) = &self.config.on_prepare_failure {
                    if let Err(e) = prepare.process(exchange).await {
                        exchange.set_exception(Some(e));
                    }
                }
                if exchange.exception().is_none() {
                    if let Err(e) = p.process(exchange).await {
                        exchange.set_exception(Some(e));
                    }
                }
                self.prepare_after_failure(exchange, is_dlc, should_handle, should_continue);
                suffix = match &self.config.dead_letter {
                    Some(d) if is_dlc => format!(
                        ". Handled by DeadLetterChannel: [{}]",
                        camel_util::uri_support::sanitize_uri(&d.uri)
                    ),
                    _ => ". Processed by failure processor: onException".to_string(),
                };
            }
            _ => self.prepare_after_failure(exchange, is_dlc, should_handle, should_continue),
        }
        let caught_text = caught
            .map(|c| c.to_string())
            .unwrap_or_else(|| "null".into());
        let msg = format!(
            "Failed delivery for {}. Exhausted after delivery attempt: {} caught: {caught_text}{suffix}",
            log_ids(exchange),
            self.redelivery_counter
        );
        self.log_failed_delivery(
            false,
            false,
            handle_or_continue,
            false,
            exchange,
            &msg,
            None,
        );
    }

    /// Moves the original in message (or only its body) to the exchange for the failure
    /// processor, if configured (Java `useOriginalInMessage`/`useOriginalInBody`).
    fn prepare_original(&self, exchange: &mut Exchange) {
        if !self.use_original_message && !self.use_original_body {
            return;
        }
        if let Some(original) = exchange.original_message().cloned() {
            if self.use_original_message {
                log::trace!("Using the original IN message instead of current");
                exchange.set_in(original);
            } else {
                log::trace!("Using the original IN message body instead of current");
                exchange.in_message_mut().set_body(original.body().clone());
            }
        }
        if exchange.has_out() {
            log::trace!("Removing the out message to avoid some uncertain behavior");
            exchange.set_out(None);
        }
    }

    /// Java `prepareExchangeAfterFailure`.
    fn prepare_after_failure(
        &self,
        exchange: &mut Exchange,
        is_dlc: bool,
        should_handle: bool,
        should_continue: bool,
    ) {
        let new_exception = exchange.exception().cloned();
        // Java ExchangeHelper.setFailureHandled
        exchange.set_exception(None);
        exchange.set_failure_handled(true);
        if let Some(handled) = exchange.error_handler_handled() {
            if !handled {
                self.restore_caught(exchange);
            }
            return;
        }
        if should_continue {
            // Java prepareExchangeForContinue
            exchange.set_rollback_only(false);
            for h in [REDELIVERED, REDELIVERY_COUNTER, REDELIVERY_MAX_COUNTER] {
                exchange.in_message_mut().remove_header(h);
            }
            exchange.set_failure_handled(false);
            if let Some(c) = exchange.property(EXCEPTION_CAUGHT).cloned() {
                exchange.set_property(EXCEPTION_HANDLED, c);
            }
        } else if should_handle {
            exchange.set_error_handler_handled(Some(true));
        } else {
            if is_dlc {
                let handled = new_exception.is_none()
                    || self
                        .config
                        .dead_letter
                        .as_ref()
                        .is_some_and(|d| d.handle_new_exception);
                if let Some(ne) = &new_exception {
                    if self.policy.log_new_exception {
                        let uri = self
                            .config
                            .dead_letter
                            .as_ref()
                            .map(|d| camel_util::uri_support::sanitize_uri(&d.uri))
                            .unwrap_or_default();
                        let how = if handled {
                            "The new exception is being handled as deadLetterHandleNewException=true."
                        } else {
                            "The new exception is not handled as deadLetterHandleNewException=false."
                        };
                        let msg = format!("New exception occurred during processing by the DeadLetterChannel[{uri}] due {ne}. {how}");
                        self.log_failed_delivery(
                            false,
                            true,
                            handled,
                            false,
                            exchange,
                            &msg,
                            Some(ne),
                        );
                    }
                }
                if handled {
                    exchange.set_error_handler_handled(Some(true));
                    return;
                }
            }
            exchange.set_error_handler_handled(Some(false));
            self.restore_caught(exchange);
        }
    }

    fn restore_caught(&self, exchange: &mut Exchange) {
        let caught = crate::try_catch::exception_caught(exchange);
        exchange.set_exception(caught);
        if let Some(to) = exchange.property(TO_ENDPOINT).cloned() {
            exchange.set_property(FAILURE_ENDPOINT, to);
        }
    }

    /// Java `logFailedDelivery`.
    #[allow(clippy::too_many_arguments)]
    fn log_failed_delivery(
        &self,
        should_redeliver: bool,
        new_exception: bool,
        handled: bool,
        continued: bool,
        exchange: &Exchange,
        message: &str,
        error: Option<&CamelError>,
    ) {
        if self.config.logger_level == LoggingLevel::OFF {
            return;
        }
        let p = &self.policy;
        let rollback = exchange.is_rollback_only() || exchange.is_rollback_only_last();
        let skip = new_exception && !p.log_new_exception
            || !new_exception && handled && !p.log_handled
            || !new_exception && continued && !p.log_continued
            || !new_exception && should_redeliver && !p.log_retry_attempted
            || !new_exception
                && should_redeliver
                && p.retry_attempted_log_interval > 1
                && self.redelivery_counter % p.retry_attempted_log_interval != 0
            || !new_exception && !should_redeliver && !p.log_exhausted;
        if !rollback && skip {
            return;
        }
        let mut level = if should_redeliver {
            p.retry_attempted_log_level
        } else {
            p.retries_exhausted_log_level
        };
        if new_exception && level == LoggingLevel::ERROR {
            level = LoggingLevel::WARN;
        }
        if let Some(l) = to_log_level(level) {
            let stack = if should_redeliver {
                p.log_retry_stack_trace
            } else {
                p.log_stack_trace
            };
            match error.filter(|_| stack) {
                Some(e) => log::log!(target: &self.config.logger_name, l, "{message}\n{e:?}"),
                None => log::log!(target: &self.config.logger_name, l, "{message}"),
            }
        }
    }
}

impl RedeliveryErrorHandler {
    /// The error handler of the step.
    pub fn new(output: SharedProcessor, config: Arc<ErrorHandlerConfig>) -> Self {
        RedeliveryErrorHandler { output, config }
    }

    /// Java `isDone`.
    fn is_done(exchange: &mut Exchange) -> bool {
        if exchange.is_interrupted() {
            exchange.set_route_stop(true);
            return true;
        }
        exchange.exception().is_none()
            || exchange.is_failure_handled()
            || exchange.is_redelivery_exhausted()
    }
}

impl RedeliveryErrorHandler {
    /// The output and the processors of the configuration (dead letter, onException, ...).
    fn processors(&self) -> Vec<SharedProcessor> {
        let c = &self.config;
        let mut list = vec![self.output.clone()];
        list.extend(c.dead_letter.as_ref().map(|d| d.processor.clone()));
        list.extend(
            c.on_redelivery
                .iter()
                .chain(&c.on_exception_occurred)
                .chain(&c.on_prepare_failure)
                .cloned(),
        );
        for policy in &c.exception_policies {
            list.extend(
                policy
                    .processor
                    .iter()
                    .chain(&policy.on_redelivery)
                    .cloned(),
            );
        }
        list
    }
}

impl Processor for RedeliveryErrorHandler {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors(&self.processors()).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(&self.processors()).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let mut task = Task::new(&self.config, exchange);
            crate::process_and_capture(self.output.as_ref(), exchange).await;
            loop {
                if Self::is_done(exchange) {
                    return Ok(());
                }
                // the step failed
                task.handle_exception(exchange);
                task.on_exception_occurred(exchange).await;
                let mut redeliver_allowed = true;
                let exhausted = exchange.is_redelivery_exhausted() || exchange.is_rollback_only();
                if !exhausted {
                    redeliver_allowed = task
                        .policy
                        .should_redeliver(
                            exchange,
                            task.redelivery_counter,
                            task.retry_while.as_ref(),
                        )
                        .unwrap_or(false);
                }
                if !redeliver_allowed || exhausted {
                    task.deliver_to_failure_processor(exchange).await;
                    return Ok(());
                }
                let header_delay = exchange
                    .in_message()
                    .header_as::<i64>(REDELIVERY_DELAY)
                    .ok()
                    .flatten();
                task.redelivery_delay = match header_delay {
                    Some(d) => d.max(0) as u64,
                    None => task
                        .policy
                        .calculate_redelivery_delay(task.redelivery_delay, task.redelivery_counter),
                };
                if task.redelivery_delay > 0 {
                    tokio::time::sleep(Duration::from_millis(task.redelivery_delay)).await;
                }
                // redeliver
                task.prepare_for_redelivery(exchange);
                if let Some(p) = &task.on_redelivery {
                    if let Err(e) = p.process(exchange).await {
                        exchange.set_exception(Some(e));
                    }
                }
                if exchange.exception().is_some() {
                    continue;
                }
                if exchange.is_route_stop() {
                    return Ok(());
                }
                log::debug!(
                    "Redelivery attempt {} of {}",
                    task.redelivery_counter,
                    log_ids(exchange)
                );
                prepare_out_to_in(exchange);
                crate::process_and_capture(self.output.as_ref(), exchange).await;
            }
        })
    }
}
