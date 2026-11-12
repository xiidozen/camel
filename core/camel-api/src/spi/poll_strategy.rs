//! The strategy of the polls of a scheduled polling consumer (Java
//! `PollingConsumerPollStrategy`): asked before each poll, told the number of polled messages,
//! and deciding whether a failed poll is retried at once.

use crate::error::{CamelError, CamelResult};
use camel_util::value::{ObjectRef, Value};
use std::sync::Arc;

/// The consumer a poll strategy is called for.
pub trait PolledConsumer: Send + Sync {
    /// The uri of the endpoint of the consumer.
    fn endpoint_uri(&self) -> &str;

    /// Stops the polls of the consumer.
    fn stop(&self);
}

/// The strategy of the polls (Java `PollingConsumerPollStrategy`; the defaults are Java
/// `DefaultPollingConsumerPollStrategy`).
pub trait PollingConsumerPollStrategy: Send + Sync {
    /// Called before a poll.
    ///
    /// # Returns
    ///
    /// Whether to poll.
    ///
    /// # Errors
    ///
    /// The poll fails (see [`rollback`](Self::rollback)).
    fn begin(&self, consumer: &dyn PolledConsumer) -> CamelResult<bool> {
        let _ = consumer;
        Ok(true)
    }

    /// Called after a successful poll with the number of polled messages.
    fn commit(&self, consumer: &dyn PolledConsumer, polled_messages: usize) {
        let _ = (consumer, polled_messages);
    }

    /// Called when the poll failed (`retry_counter` is `-1` when [`begin`](Self::begin)
    /// failed, else the number of the attempt).
    ///
    /// # Returns
    ///
    /// Whether to poll again at once (else the error is handled by the consumer).
    ///
    /// # Errors
    ///
    /// The error the consumer handles instead.
    fn rollback(
        &self,
        consumer: &dyn PolledConsumer,
        retry_counter: i32,
        cause: &CamelError,
    ) -> CamelResult<bool> {
        let _ = retry_counter;
        let _ = cause;
        let _ = consumer;
        Ok(false)
    }
}

/// A shared [`PollingConsumerPollStrategy`].
pub type SharedPollStrategy = Arc<dyn PollingConsumerPollStrategy>;

/// The default strategy (Java `DefaultPollingConsumerPollStrategy`).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPollingConsumerPollStrategy;

impl PollingConsumerPollStrategy for DefaultPollingConsumerPollStrategy {}

/// The strategy as a value, to bind it in the registry (`pollStrategy=#name`).
pub fn poll_strategy_value(strategy: SharedPollStrategy) -> Value {
    Value::Object(ObjectRef::new(strategy))
}

/// The strategy held by a value of the registry (see [`poll_strategy_value`]).
pub fn poll_strategy_of(value: &Value) -> Option<SharedPollStrategy> {
    match value {
        Value::Object(o) => o.downcast_ref::<SharedPollStrategy>().cloned(),
        _ => None,
    }
}
