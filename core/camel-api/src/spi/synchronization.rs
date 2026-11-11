//! Callbacks on the completion of an exchange (Java `org.apache.camel.spi.Synchronization`,
//! `SynchronizationVetoable` and `SynchronizationRouteAware`, merged into one trait with
//! default methods).
//!
//! Synchronizations are normally executed when the [`Exchange`] completes at the end. If the
//! exchange is processed asynchronously, the synchronizations are handed over to the next task
//! (see [`Exchange::handover_completions`]). This ensures, for example, that the file consumer
//! deletes the processed file at the very end, when the exchange has been completed
//! successfully. A synchronization can veto the handover with
//! [`Synchronization::allow_handover`], such as when doing asynchronous request/reply over
//! SEDA.

use crate::exchange::Exchange;
use crate::processor::BoxFuture;
use std::future::ready;
use std::sync::Arc;

/// The highest priority of [`Synchronization::order`] (Java `Ordered.HIGHEST`).
pub const ORDER_HIGHEST: i32 = i32::MIN;
/// The lowest priority of [`Synchronization::order`] (Java `Ordered.LOWEST`).
pub const ORDER_LOWEST: i32 = i32::MAX;

/// Callbacks invoked when an exchange is complete: for example to commit or roll back a
/// transaction, or to delete a consumed file.
///
/// The callbacks are async, as they may route the exchange (the onCompletion EIP). Errors
/// cannot be returned: a failing callback logs, as in Java, where the exceptions of the
/// callbacks are ignored.
pub trait Synchronization: Send + Sync {
    /// Called when the exchange completed successfully.
    fn on_complete<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()>;

    /// Called when the exchange failed (it has an error, or is marked for rollback).
    fn on_failure<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()>;

    /// The order of the callback: lower values run first (Java `Ordered`; default 0).
    fn order(&self) -> i32 {
        0
    }

    /// Invoked before the [`Exchange`] is being routed by the given route (Java
    /// `SynchronizationRouteAware.onBeforeRoute`).
    ///
    /// Notice if the exchange is being routed through multiple routes, there will be callbacks
    /// for each route.
    ///
    /// **Important:** this callback may not be invoked if the synchronization is added to the
    /// exchange after the routing has started.
    ///
    /// # Arguments
    ///
    /// * `route_id` - the id of the route
    /// * `exchange` - the exchange
    fn on_before_route<'a>(
        &'a self,
        route_id: &'a str,
        exchange: &'a mut Exchange,
    ) -> BoxFuture<'a, ()> {
        let _ = (route_id, exchange);
        Box::pin(ready(()))
    }

    /// Invoked after the [`Exchange`] has been routed by the given route (Java
    /// `SynchronizationRouteAware.onAfterRoute`).
    ///
    /// Notice if the exchange is being routed through multiple routes, there will be callbacks
    /// for each route.
    ///
    /// This invocation happens before these callbacks:
    ///
    /// - the consumer of the route writes any response back to the caller (if in InOut mode);
    /// - the unit of work is done calling either [`on_complete`](Synchronization::on_complete)
    ///   or [`on_failure`](Synchronization::on_failure).
    ///
    /// This allows custom logic to be executed after all routing is done, but before the
    /// consumer prepares and writes any data back to the caller (if in InOut mode).
    ///
    /// # Arguments
    ///
    /// * `route_id` - the id of the route
    /// * `exchange` - the exchange
    fn on_after_route<'a>(
        &'a self,
        route_id: &'a str,
        exchange: &'a mut Exchange,
    ) -> BoxFuture<'a, ()> {
        let _ = (route_id, exchange);
        Box::pin(ready(()))
    }

    /// Whether or not handover of this synchronization is allowed (Java
    /// `SynchronizationVetoable.allowHandover`).
    ///
    /// For example when an [`Exchange`] is being routed from one task to another, such as
    /// using request/reply over SEDA.
    ///
    /// # Returns
    ///
    /// `true` to allow handover (the default), `false` to deny.
    fn allow_handover(&self) -> bool {
        true
    }

    /// A method to perform optional housekeeping when a synchronization is being handed over
    /// (Java `SynchronizationVetoable.beforeHandover`).
    ///
    /// # Arguments
    ///
    /// * `exchange` - the exchange to which the synchronizations are being transferred
    fn before_handover(&self, exchange: &mut Exchange) {
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
    fn on_complete<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        (self.on_complete)(exchange);
        Box::pin(ready(()))
    }

    fn on_failure<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        (self.on_failure)(exchange);
        Box::pin(ready(()))
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

/// Runs the synchronizations for the exchange: `on_failure` if it failed, else `on_complete`,
/// the latest added first (FILO), then by [`order`](Synchronization::order) (Java
/// `UnitOfWorkHelper.doneSynchronizations`).
pub async fn done_synchronizations(
    exchange: &mut Exchange,
    synchronizations: Vec<SharedSynchronization>,
) {
    let copy = ordered(synchronizations);
    let failed = exchange.is_failed();
    for s in copy {
        if failed {
            s.on_failure(exchange).await;
        } else {
            s.on_complete(exchange).await;
        }
    }
}

/// Calls `on_after_route` of the synchronizations (Java
/// `UnitOfWorkHelper.afterRouteSynchronizations`).
pub async fn after_route_synchronizations(
    route_id: &str,
    exchange: &mut Exchange,
    synchronizations: Vec<SharedSynchronization>,
) {
    for s in ordered(synchronizations) {
        s.on_after_route(route_id, exchange).await;
    }
}

/// FILO, then by order (a stable sort, as Java).
fn ordered(mut list: Vec<SharedSynchronization>) -> Vec<SharedSynchronization> {
    list.reverse();
    list.sort_by_key(|s| s.order());
    list
}
