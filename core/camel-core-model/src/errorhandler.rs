//! Error handlers and `onException` in the model (Java `ErrorHandlerDefinition`,
//! `DefaultErrorHandlerDefinition`, `DeadLetterChannelDefinition`, `NoErrorHandlerDefinition`,
//! `RedeliveryPolicyDefinition` and `OnExceptionDefinition`).

use crate::expression::ExpressionDefinition;
use crate::processor::{ProcessRef, ProcessorDefinition, Steps};
use camel_api::enums::LoggingLevel;
use camel_api::error::ErrorType;
use std::fmt;

/// The redelivery options (each `None` keeps the default or the inherited value).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RedeliveryPolicyDefinition {
    /// The maximum number of redeliveries (-1 forever).
    pub maximum_redeliveries: Option<i32>,
    /// The delay (millis) before the first redelivery.
    pub redelivery_delay: Option<u64>,
    /// Whether the delay grows exponentially.
    pub use_exponential_back_off: Option<bool>,
    /// The multiplier of the exponential back off.
    pub back_off_multiplier: Option<f64>,
    /// The maximum delay (millis).
    pub maximum_redelivery_delay: Option<u64>,
    /// The level to log exhausted failures at.
    pub retries_exhausted_log_level: Option<LoggingLevel>,
    /// The level to log redelivery attempts at.
    pub retry_attempted_log_level: Option<LoggingLevel>,
    /// Whether to log the stack trace of exhausted failures.
    pub log_stack_trace: Option<bool>,
    /// Whether to log handled failures.
    pub log_handled: Option<bool>,
    /// Whether to log exhausted failures.
    pub log_exhausted: Option<bool>,
    /// A delay pattern such as `0:1000;5:5000`.
    pub delay_pattern: Option<String>,
}

/// The kind of error handler.
#[derive(Clone, Debug, PartialEq)]
pub enum ErrorHandlerKind {
    /// The default error handler: redeliveries, then the failure goes back to the caller.
    Default,
    /// The dead letter channel: redeliveries, then the exchange goes to the dead letter
    /// endpoint and the failure is handled.
    DeadLetterChannel {
        /// The dead letter endpoint uri.
        uri: String,
        /// Whether a new error while sending to the dead letter is handled.
        handle_new_exception: bool,
    },
    /// No error handling.
    NoErrorHandler,
}

/// An error handler.
#[derive(Clone, Debug)]
pub struct ErrorHandlerDefinition {
    /// The kind.
    pub kind: ErrorHandlerKind,
    /// The redelivery options.
    pub redelivery: RedeliveryPolicyDefinition,
    /// A processor called before each redelivery.
    pub on_redelivery: Option<ProcessRef>,
    /// A processor called when an error occurs.
    pub on_exception_occurred: Option<ProcessRef>,
    /// A processor called before the dead letter gets the exchange.
    pub on_prepare_failure: Option<ProcessRef>,
    /// Redeliver while this predicate matches.
    pub retry_while: Option<ExpressionDefinition>,
    /// The logger name.
    pub log_name: Option<String>,
    /// Whether the failure processor gets the original in message (as it was when the unit of
    /// work started) instead of the current message (Java `useOriginalMessage`).
    pub use_original_message: bool,
    /// Whether the failure processor gets the original in message body, keeping the current
    /// headers (Java `useOriginalBody`).
    pub use_original_body: bool,
}

impl ErrorHandlerDefinition {
    /// An error handler of the kind with the default options.
    pub fn new(kind: ErrorHandlerKind) -> Self {
        ErrorHandlerDefinition {
            kind,
            redelivery: RedeliveryPolicyDefinition::default(),
            on_redelivery: None,
            on_exception_occurred: None,
            on_prepare_failure: None,
            retry_while: None,
            log_name: None,
            use_original_message: false,
            use_original_body: false,
        }
    }
}

impl fmt::Display for ErrorHandlerDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorHandlerKind::Default => f.write_str("DefaultErrorHandler"),
            ErrorHandlerKind::DeadLetterChannel { uri, .. } => write!(
                f,
                "DeadLetterChannel[{}]",
                camel_util::uri_support::sanitize_uri(uri)
            ),
            ErrorHandlerKind::NoErrorHandler => f.write_str("NoErrorHandler"),
        }
    }
}

/// An `onException` clause: what to do for some errors.
#[derive(Clone, Debug, Default)]
pub struct OnExceptionDefinition {
    /// The types of the errors.
    pub exceptions: Vec<ErrorType>,
    /// An extra predicate the exchange must match.
    pub on_when: Option<ExpressionDefinition>,
    /// Redeliver while this predicate matches.
    pub retry_while: Option<ExpressionDefinition>,
    /// Whether the error is handled.
    pub handled: Option<ExpressionDefinition>,
    /// Whether routing continues after the failed step.
    pub continued: Option<ExpressionDefinition>,
    /// The redelivery options.
    pub redelivery: RedeliveryPolicyDefinition,
    /// A processor called before each redelivery.
    pub on_redelivery: Option<ProcessRef>,
    /// A processor called when the error occurs.
    pub on_exception_occurred: Option<ProcessRef>,
    /// Whether the failure processor gets the original in message (as it was when the unit of
    /// work started) instead of the current message (Java `useOriginalMessage`).
    pub use_original_message: bool,
    /// Whether the failure processor gets the original in message body, keeping the current
    /// headers (Java `useOriginalBody`).
    pub use_original_body: bool,
    /// The steps of the clause.
    pub steps: Vec<ProcessorDefinition>,
}

impl fmt::Display for OnExceptionDefinition {
    /// Java `toString()`: `OnException[[types] -> [steps]]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.exceptions.iter().map(|e| e.to_string()).collect();
        let when = self
            .on_when
            .as_ref()
            .map(|w| format!(" when {w}"))
            .unwrap_or_default();
        write!(
            f,
            "OnException[[{}]{when} -> {}]",
            names.join(", "),
            Steps(&self.steps)
        )
    }
}
