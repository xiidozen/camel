//! The repository of the idempotent consumer EIP (Java
//! `org.apache.camel.spi.IdempotentRepository`): the message ids already processed.

use std::sync::Arc;

/// The ids of the messages already processed, so the idempotent consumer can detect
/// duplicates.
pub trait IdempotentRepository: Send + Sync {
    /// Adds the key, `false` if it was already there.
    fn add(&self, key: &str) -> bool;

    /// Whether the repository has the key.
    fn contains(&self, key: &str) -> bool;

    /// Removes the key, `false` if it was not there.
    fn remove(&self, key: &str) -> bool;

    /// Confirms the key, once the message is processed (repositories may add keys
    /// provisionally); `true` if confirmed.
    fn confirm(&self, key: &str) -> bool {
        let _ = key;
        true
    }

    /// Removes all the keys.
    fn clear(&self);
}

/// A shared idempotent repository.
pub type SharedIdempotentRepository = Arc<dyn IdempotentRepository>;
