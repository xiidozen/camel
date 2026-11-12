//! The idempotent consumer (Java `IdempotentConsumer` and `IdempotentOnCompletion`):
//! processes a message once per message id, the duplicates being skipped (or marked with the
//! `CamelDuplicateMessage` property).

use crate::process_and_capture;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::DUPLICATE_MESSAGE;
use camel_api::processor::{BoxFuture, Processor, SharedExpression, SharedProcessor};
use camel_api::spi::idempotent_repository::SharedIdempotentRepository;
use camel_api::spi::synchronization::Synchronization;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// The Java class name of `org.apache.camel.processor.idempotent.NoMessageIdException`.
pub const NO_MESSAGE_ID_EXCEPTION: &str =
    "org.apache.camel.processor.idempotent.NoMessageIdException";

/// The options of the idempotent consumer (Java `IdempotentConsumerDefinition`).
#[derive(Debug, Clone, Copy)]
pub struct IdempotentOptions {
    /// Whether the key is added to the repository before processing (default `true`), else
    /// when the exchange completes.
    pub eager: bool,
    /// Whether the completion work is done when the block ends instead of when the exchange
    /// completes (default `false`).
    pub completion_eager: bool,
    /// Whether duplicates are skipped (default `true`), else routed with the
    /// `CamelDuplicateMessage` property.
    pub skip_duplicate: bool,
    /// Whether the key is removed when the exchange fails (default `true`).
    pub remove_on_failure: bool,
}

impl Default for IdempotentOptions {
    fn default() -> Self {
        IdempotentOptions {
            eager: true,
            completion_eager: false,
            skip_duplicate: true,
            remove_on_failure: true,
        }
    }
}

/// The idempotent consumer EIP.
pub struct IdempotentConsumerProcessor {
    message_id: SharedExpression,
    repository: SharedIdempotentRepository,
    options: IdempotentOptions,
    processor: SharedProcessor,
    duplicates: AtomicU64,
}

impl IdempotentConsumerProcessor {
    /// Creates the processor.
    pub fn new(
        message_id: SharedExpression,
        repository: SharedIdempotentRepository,
        options: IdempotentOptions,
        processor: SharedProcessor,
    ) -> Self {
        IdempotentConsumerProcessor {
            message_id,
            repository,
            options,
            processor,
            duplicates: AtomicU64::new(0),
        }
    }

    /// The number of duplicate messages detected.
    pub fn duplicate_message_count(&self) -> u64 {
        self.duplicates.load(Ordering::SeqCst)
    }

    async fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let id = self.message_id.evaluate_mut(exchange)?;
        if id.is_null() {
            return Err(CamelError::camel_exchange(
                &format!(
                    "No message ID could be found using expression: {} on message exchange: {exchange}",
                    self.message_id.describe()
                ),
                exchange,
            )
            .with_subtype(NO_MESSAGE_ID_EXCEPTION));
        }
        let id = id.to_string();
        let new_key = if self.options.eager {
            self.repository.add(&id)
        } else {
            !self.repository.contains(&id)
        };
        if !new_key {
            exchange.set_property(DUPLICATE_MESSAGE, true);
            self.duplicates.fetch_add(1, Ordering::SeqCst);
            log::debug!("Ignoring duplicate message with id: {id} for exchange: {exchange}");
            if self.options.skip_duplicate {
                return Ok(());
            }
            // the duplicate does not own the key: no completion work
            process_and_capture(self.processor.as_ref(), exchange).await;
            return Ok(());
        }
        let completion = Arc::new(IdempotentOnCompletion {
            repository: self.repository.clone(),
            message_id: id,
            eager: self.options.eager,
            remove_on_failure: self.options.remove_on_failure,
        });
        if self.options.completion_eager {
            process_and_capture(self.processor.as_ref(), exchange).await;
            if exchange.is_failed() {
                completion.on_failure(exchange).await;
            } else {
                completion.on_complete(exchange).await;
            }
        } else {
            exchange.add_on_completion(completion);
            process_and_capture(self.processor.as_ref(), exchange).await;
        }
        Ok(())
    }
}

impl Processor for IdempotentConsumerProcessor {
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

/// Adds (when not eager) and confirms the key when the exchange completes, removes it when
/// the exchange fails or was handled by a dead letter channel (Java `IdempotentOnCompletion`).
struct IdempotentOnCompletion {
    repository: SharedIdempotentRepository,
    message_id: String,
    eager: bool,
    remove_on_failure: bool,
}

impl IdempotentOnCompletion {
    fn failed(&self) {
        if self.remove_on_failure {
            self.repository.remove(&self.message_id);
            log::debug!(
                "Removed from repository as exchange failed with id: {}",
                self.message_id
            );
        }
    }
}

impl Synchronization for IdempotentOnCompletion {
    fn on_complete<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        if exchange.is_failure_handled() {
            // moved to the dead letter queue: not complete
            self.failed();
        } else {
            if !self.eager {
                self.repository.add(&self.message_id);
            }
            self.repository.confirm(&self.message_id);
        }
        Box::pin(std::future::ready(()))
    }

    fn on_failure<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        self.failed();
        Box::pin(std::future::ready(()))
    }
}
