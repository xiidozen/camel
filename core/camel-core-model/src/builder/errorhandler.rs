//! The error handler and `onException` DSL (Java `RouteBuilder.errorHandler(..)`,
//! `deadLetterChannel(..)`, `defaultErrorHandler()`, `noErrorHandler()` and
//! `onException(..)`).
//!
//! ```
//! use camel_api::error::ErrorKind;
//! use camel_core_model::builder::*;
//!
//! let builder = |r: &mut Routes| {
//!     r.error_handler(dead_letter_channel("mock:dead").maximum_redeliveries(2).redelivery_delay(0));
//!     r.on_exception(on_exception(ErrorKind::IllegalArgument).handled(true).to("mock:invalid"));
//!     r.add(from("direct:start").to("mock:result"));
//! };
//! let defs = builder.build();
//! assert_eq!("DeadLetterChannel[mock:dead]", defs[0].error_handler.as_ref().unwrap().to_string());
//! assert_eq!(
//!     "OnException[[java.lang.IllegalArgumentException] -> [to[mock:invalid]]]",
//!     defs[0].builder_on_exceptions[0].to_string()
//! );
//! ```

use super::{Dsl, IntoErrorTypes};
use crate::errorhandler::{
    ErrorHandlerDefinition, ErrorHandlerKind, OnExceptionDefinition, RedeliveryPolicyDefinition,
};
use crate::expression::ExpressionDefinition;
use crate::processor::{ProcessRef, ProcessorDefinition};
use camel_api::enums::LoggingLevel;
use camel_api::processor::SharedProcessor;
use camel_support::builder::constant;

fn constant_predicate(value: bool) -> ExpressionDefinition {
    ExpressionDefinition::Predicate(constant(value).as_predicate())
}

/// The redelivery options shared by the error handler and `onException` builders.
pub trait RedeliveryOptions: Sized {
    /// The redelivery options to change.
    fn redelivery_mut(&mut self) -> &mut RedeliveryPolicyDefinition;

    /// The maximum number of redeliveries (-1 forever).
    fn maximum_redeliveries(mut self, n: i32) -> Self {
        self.redelivery_mut().maximum_redeliveries = Some(n);
        self
    }

    /// Disables redelivery (Java `disableRedelivery()`: sets the maximum redeliveries to 0).
    fn disable_redelivery(self) -> Self {
        self.maximum_redeliveries(0)
    }

    /// The delay (millis) before the first redelivery.
    fn redelivery_delay(mut self, millis: u64) -> Self {
        self.redelivery_mut().redelivery_delay = Some(millis);
        self
    }

    /// Exponential back off of the delays.
    fn use_exponential_back_off(mut self) -> Self {
        self.redelivery_mut().use_exponential_back_off = Some(true);
        self
    }

    /// The multiplier of the exponential back off.
    fn back_off_multiplier(mut self, multiplier: f64) -> Self {
        self.redelivery_mut().back_off_multiplier = Some(multiplier);
        self
    }

    /// The maximum delay (millis).
    fn maximum_redelivery_delay(mut self, millis: u64) -> Self {
        self.redelivery_mut().maximum_redelivery_delay = Some(millis);
        self
    }

    /// The level to log redelivery attempts at.
    fn retry_attempted_log_level(mut self, level: LoggingLevel) -> Self {
        self.redelivery_mut().retry_attempted_log_level = Some(level);
        self
    }

    /// The level to log exhausted failures at.
    fn retries_exhausted_log_level(mut self, level: LoggingLevel) -> Self {
        self.redelivery_mut().retries_exhausted_log_level = Some(level);
        self
    }

    /// Whether to log the stack trace of exhausted failures.
    fn log_stack_trace(mut self, log: bool) -> Self {
        self.redelivery_mut().log_stack_trace = Some(log);
        self
    }

    /// Whether to log handled failures.
    fn log_handled(mut self, log: bool) -> Self {
        self.redelivery_mut().log_handled = Some(log);
        self
    }

    /// Whether to log exhausted failures.
    fn log_exhausted(mut self, log: bool) -> Self {
        self.redelivery_mut().log_exhausted = Some(log);
        self
    }

    /// A delay pattern such as `0:1000;5:5000`.
    fn delay_pattern(mut self, pattern: &str) -> Self {
        self.redelivery_mut().delay_pattern = Some(pattern.to_string());
        self
    }
}

/// An error handler being built.
pub struct ErrorHandlerBuilder {
    def: ErrorHandlerDefinition,
}

/// The default error handler (Java `defaultErrorHandler()`).
pub fn default_error_handler() -> ErrorHandlerBuilder {
    ErrorHandlerBuilder {
        def: ErrorHandlerDefinition::new(ErrorHandlerKind::Default),
    }
}

/// A dead letter channel to the endpoint (Java `deadLetterChannel(uri)`).
pub fn dead_letter_channel(uri: &str) -> ErrorHandlerBuilder {
    ErrorHandlerBuilder {
        def: ErrorHandlerDefinition::new(ErrorHandlerKind::DeadLetterChannel {
            uri: uri.to_string(),
            handle_new_exception: true,
        }),
    }
}

/// No error handling (Java `noErrorHandler()`).
pub fn no_error_handler() -> ErrorHandlerBuilder {
    ErrorHandlerBuilder {
        def: ErrorHandlerDefinition::new(ErrorHandlerKind::NoErrorHandler),
    }
}

impl RedeliveryOptions for ErrorHandlerBuilder {
    fn redelivery_mut(&mut self) -> &mut RedeliveryPolicyDefinition {
        &mut self.def.redelivery
    }
}

impl ErrorHandlerBuilder {
    /// A processor called before each redelivery.
    pub fn on_redelivery(mut self, processor: SharedProcessor) -> Self {
        self.def.on_redelivery = Some(ProcessRef::Processor(processor));
        self
    }

    /// A processor bean (by name) called before each redelivery (Java `onRedeliveryRef`).
    pub fn on_redelivery_ref(mut self, name: &str) -> Self {
        self.def.on_redelivery = Some(ProcessRef::Ref(name.to_string()));
        self
    }

    /// A processor called when an error occurs.
    pub fn on_exception_occurred(mut self, processor: SharedProcessor) -> Self {
        self.def.on_exception_occurred = Some(ProcessRef::Processor(processor));
        self
    }

    /// A processor called before the dead letter gets the exchange.
    pub fn on_prepare_failure(mut self, processor: SharedProcessor) -> Self {
        self.def.on_prepare_failure = Some(ProcessRef::Processor(processor));
        self
    }

    /// Redeliver while the predicate matches.
    pub fn retry_while(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.def.retry_while = Some(predicate.into());
        self
    }

    /// Whether a new error while sending to the dead letter is handled (default `true`).
    pub fn dead_letter_handle_new_exception(mut self, handle: bool) -> Self {
        if let ErrorHandlerKind::DeadLetterChannel {
            handle_new_exception,
            ..
        } = &mut self.def.kind
        {
            *handle_new_exception = handle;
        }
        self
    }

    /// The logger name.
    pub fn log_name(mut self, name: &str) -> Self {
        self.def.log_name = Some(name.to_string());
        self
    }

    /// Will use the original input message when an exchange is moved to the dead letter queue (Java
    /// `useOriginalMessage()`).
    ///
    /// The original input message is the message as it was when the exchange entered its
    /// first route (its unit of work started). Useful to move the incoming message to a dead
    /// letter queue as it was received, instead of what the route changed it to. Notice that
    /// it replaces the whole message, including its headers; see
    /// [`use_original_body`](ErrorHandlerBuilder::use_original_body) to keep the current headers.
    ///
    /// The original message is not used if routing continues (an `onException` with [`OnExceptionDsl::continued`]).
    pub fn use_original_message(mut self) -> Self {
        self.def.use_original_message = true;
        self
    }

    /// Will use the original input message body when an exchange is moved to the dead letter queue,
    /// keeping the current headers (Java `useOriginalBody()`); see
    /// [`use_original_message`](ErrorHandlerBuilder::use_original_message).
    pub fn use_original_body(mut self) -> Self {
        self.def.use_original_body = true;
        self
    }

    /// The definition.
    pub fn build(self) -> ErrorHandlerDefinition {
        self.def
    }
}

impl From<ErrorHandlerBuilder> for ErrorHandlerDefinition {
    fn from(b: ErrorHandlerBuilder) -> Self {
        b.def
    }
}

/// An `onException` clause being built; its steps are added with the step methods (such as
/// `.to(..)`).
pub struct OnExceptionDsl {
    def: OnExceptionDefinition,
}

/// An `onException` clause for the error types (Java `onException(IOException.class)`).
pub fn on_exception(exceptions: impl IntoErrorTypes) -> OnExceptionDsl {
    OnExceptionDsl {
        def: OnExceptionDefinition {
            exceptions: exceptions.into_error_types(),
            ..Default::default()
        },
    }
}

impl RedeliveryOptions for OnExceptionDsl {
    fn redelivery_mut(&mut self) -> &mut RedeliveryPolicyDefinition {
        &mut self.def.redelivery
    }
}

impl OnExceptionDsl {
    /// Whether the error is handled: the caller sees a successful exchange.
    pub fn handled(mut self, handled: bool) -> Self {
        self.def.handled = Some(constant_predicate(handled));
        self
    }

    /// The error is handled when the predicate matches.
    pub fn handled_when(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.def.handled = Some(predicate.into());
        self
    }

    /// Whether routing continues after the failed step.
    pub fn continued(mut self, continued: bool) -> Self {
        self.def.continued = Some(constant_predicate(continued));
        self
    }

    /// Routing continues when the predicate matches.
    pub fn continued_when(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.def.continued = Some(predicate.into());
        self
    }

    /// The clause applies only when the predicate also matches.
    pub fn on_when(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.def.on_when = Some(predicate.into());
        self
    }

    /// Redeliver while the predicate matches.
    pub fn retry_while(mut self, predicate: impl Into<ExpressionDefinition>) -> Self {
        self.def.retry_while = Some(predicate.into());
        self
    }

    /// A processor called before each redelivery.
    pub fn on_redelivery(mut self, processor: SharedProcessor) -> Self {
        self.def.on_redelivery = Some(ProcessRef::Processor(processor));
        self
    }

    /// A processor called when the error occurs.
    pub fn on_exception_occurred(mut self, processor: SharedProcessor) -> Self {
        self.def.on_exception_occurred = Some(ProcessRef::Processor(processor));
        self
    }

    /// Will use the original input message when an exchange is moved to the steps of the clause (Java
    /// `useOriginalMessage()`).
    ///
    /// The original input message is the message as it was when the exchange entered its
    /// first route (its unit of work started). Useful to move the incoming message to a dead
    /// letter queue as it was received, instead of what the route changed it to. Notice that
    /// it replaces the whole message, including its headers; see
    /// [`use_original_body`](OnExceptionDsl::use_original_body) to keep the current headers.
    ///
    /// The original message is not used if routing [continues](OnExceptionDsl::continued).
    pub fn use_original_message(mut self) -> Self {
        self.def.use_original_message = true;
        self
    }

    /// Will use the original input message body when an exchange is moved to the steps of the clause,
    /// keeping the current headers (Java `useOriginalBody()`); see
    /// [`use_original_message`](OnExceptionDsl::use_original_message).
    pub fn use_original_body(mut self) -> Self {
        self.def.use_original_body = true;
        self
    }

    /// The definition.
    pub fn build(self) -> OnExceptionDefinition {
        self.def
    }
}

impl Dsl for OnExceptionDsl {
    fn push(mut self, step: ProcessorDefinition) -> Self {
        self.def.steps.push(step);
        self
    }

    fn last_mut(&mut self) -> Option<&mut ProcessorDefinition> {
        self.def.steps.last_mut()
    }
}

impl From<OnExceptionDsl> for OnExceptionDefinition {
    fn from(b: OnExceptionDsl) -> Self {
        b.def
    }
}
