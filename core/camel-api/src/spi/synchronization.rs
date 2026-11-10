//! Callbacks on the completion of an exchange (Java `org.apache.camel.spi.Synchronization`,
//! `SynchronizationVetoable`).

use crate::exchange::Exchange;
use std::sync::Arc;

/// Callbacks invoked when an exchange is complete: for example to commit or roll back a
/// transaction, or to delete a consumed file.
pub trait Synchronization: Send + Sync {
    /// Called when the exchange completed successfully.
    fn on_complete(&self, exchange: &mut Exchange);

    /// Called when the exchange failed (it has an error, or is marked for rollback).
    fn on_failure(&self, exchange: &mut Exchange);

    /// Whether the synchronization may be handed over to another exchange (for example when an
    /// exchange is copied and continued asynchronously by a wire tap or seda consumer).
    fn allow_handover(&self) -> bool {
        true
    }

    /// Called before the synchronization is handed over to another exchange.
    fn before_handover(&self, exchange: &Exchange) {
        let _ = exchange;
    }
}

/// A shared synchronization.
pub type SharedSynchronization = Arc<dyn Synchronization>;

/// A synchronization from two closures.
pub struct FnSynchronization<C, F> {
    on_complete: C,
    on_failure: F,
}

impl<C, F> Synchronization for FnSynchronization<C, F>
where
    C: Fn(&mut Exchange) + Send + Sync,
    F: Fn(&mut Exchange) + Send + Sync,
{
    fn on_complete(&self, exchange: &mut Exchange) {
        (self.on_complete)(exchange)
    }

    fn on_failure(&self, exchange: &mut Exchange) {
        (self.on_failure)(exchange)
    }
}

/// Creates a synchronization from closures for completion and failure.
pub fn synchronization_fn<C, F>(on_complete: C, on_failure: F) -> FnSynchronization<C, F>
where
    C: Fn(&mut Exchange) + Send + Sync,
    F: Fn(&mut Exchange) + Send + Sync,
{
    FnSynchronization {
        on_complete,
        on_failure,
    }
}
