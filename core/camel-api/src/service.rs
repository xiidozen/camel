//! The service lifecycle (Java `org.apache.camel.Service`, `StatefulService`,
//! `SuspendableService`, `ShutdownableService`), and [`ServiceState`], a helper that tracks the
//! lifecycle status (the state machine of Java's `ServiceSupport`).

use crate::enums::ServiceStatus;
use crate::error::CamelResult;
use crate::processor::BoxFuture;
use std::sync::Mutex;

/// A service with a lifecycle: build, init, start and stop. Components, endpoints, producers,
/// consumers, routes and the context are services.
///
/// Starting and stopping are asynchronous, as they may open or close connections.
pub trait Service: Send + Sync {
    /// Optional build phase (before init), done once.
    ///
    /// # Errors
    ///
    /// Returns an error if building fails.
    fn build(&self) -> CamelResult<()> {
        Ok(())
    }

    /// Optional initialization phase (before start), done once.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    fn init(&self) -> CamelResult<()> {
        Ok(())
    }

    /// Starts the service.
    ///
    /// # Errors
    ///
    /// Returns an error if the service cannot be started.
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// Stops the service.
    ///
    /// # Errors
    ///
    /// Returns an error if the service cannot be stopped.
    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(Ok(())))
    }
}

/// A service whose lifecycle status can be queried (Java `StatefulService`).
pub trait StatefulService: Service {
    /// The current status.
    fn status(&self) -> ServiceStatus;

    /// Whether the service is started.
    fn is_started(&self) -> bool {
        self.status().is_started()
    }

    /// Whether the service is starting.
    fn is_starting(&self) -> bool {
        self.status().is_starting()
    }

    /// Whether the service is stopping.
    fn is_stopping(&self) -> bool {
        self.status().is_stopping()
    }

    /// Whether the service is stopped.
    fn is_stopped(&self) -> bool {
        self.status().is_stopped()
    }

    /// Whether the service is suspending.
    fn is_suspending(&self) -> bool {
        self.status().is_suspending()
    }

    /// Whether the service is suspended.
    fn is_suspended(&self) -> bool {
        self.status().is_suspended()
    }

    /// Whether the service allows running (it is not stopping, stopped or suspended).
    fn is_run_allowed(&self) -> bool {
        !matches!(
            self.status(),
            ServiceStatus::Stopping
                | ServiceStatus::Stopped
                | ServiceStatus::Suspending
                | ServiceStatus::Suspended
        )
    }
}

/// A service that can be suspended and resumed (Java `SuspendableService`).
pub trait SuspendableService: Service {
    /// Suspends the service (it keeps its resources, but stops accepting work).
    ///
    /// # Errors
    ///
    /// Returns an error if the service cannot be suspended.
    fn suspend(&self) -> BoxFuture<'_, CamelResult<()>>;

    /// Resumes the suspended service.
    ///
    /// # Errors
    ///
    /// Returns an error if the service cannot be resumed.
    fn resume(&self) -> BoxFuture<'_, CamelResult<()>>;
}

/// A service that must be shut down explicitly to release its resources (Java
/// `ShutdownableService`).
pub trait ShutdownableService: Service {
    /// Shuts down the service; it cannot be started again.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown fails.
    fn shutdown(&self) -> BoxFuture<'_, CamelResult<()>>;
}

/// Tracks the lifecycle status of a service and runs the lifecycle actions only when the
/// transition is allowed (the state machine of Java `ServiceSupport`): starting a started
/// service or stopping a stopped one does nothing.
///
/// # Examples
///
/// ```
/// use camel_api::enums::ServiceStatus;
/// use camel_api::processor::block_on;
/// use camel_api::service::ServiceState;
///
/// let state = ServiceState::new();
/// block_on(state.start(|| async { Ok(()) })).unwrap();
/// assert_eq!(ServiceStatus::Started, state.status());
/// block_on(state.stop(|| async { Ok(()) })).unwrap();
/// assert!(state.status().is_stopped());
/// ```
#[derive(Debug)]
pub struct ServiceState {
    status: Mutex<ServiceStatus>,
    built: Mutex<bool>,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceState {
    /// A new, not yet initialized, state (the Java "new" state is reported as `Initializing`
    /// until [`ServiceState::init`] completes).
    pub fn new() -> Self {
        ServiceState {
            status: Mutex::new(ServiceStatus::Initializing),
            built: Mutex::new(false),
        }
    }

    /// The current status.
    pub fn status(&self) -> ServiceStatus {
        *self.status.lock().unwrap()
    }

    fn set(&self, status: ServiceStatus) {
        *self.status.lock().unwrap() = status;
    }

    /// Runs the build action once.
    ///
    /// # Errors
    ///
    /// Returns the error of the action.
    pub fn build(&self, action: impl FnOnce() -> CamelResult<()>) -> CamelResult<()> {
        let mut built = self.built.lock().unwrap();
        if !*built {
            action()?;
            *built = true;
        }
        Ok(())
    }

    /// Runs the init action once (moves to `Initialized`).
    ///
    /// # Errors
    ///
    /// Returns the error of the action.
    pub fn init(&self, action: impl FnOnce() -> CamelResult<()>) -> CamelResult<()> {
        if self.status() != ServiceStatus::Initializing {
            return Ok(());
        }
        action()?;
        self.set(ServiceStatus::Initialized);
        Ok(())
    }

    /// Runs the start action if the service is startable (initializing first if needed).
    /// On failure the status becomes `Stopped`.
    ///
    /// # Errors
    ///
    /// Returns the error of the action.
    pub async fn start<F, Fut>(&self, action: F) -> CamelResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = CamelResult<()>>,
    {
        if self.status() == ServiceStatus::Initializing {
            self.set(ServiceStatus::Initialized);
        }
        if !self.status().is_startable() {
            return Ok(());
        }
        self.set(ServiceStatus::Starting);
        match action().await {
            Ok(()) => {
                self.set(ServiceStatus::Started);
                Ok(())
            }
            Err(e) => {
                self.set(ServiceStatus::Stopped);
                Err(e)
            }
        }
    }

    /// Runs the stop action if the service is stoppable. The status becomes `Stopped` also if
    /// the action fails.
    ///
    /// # Errors
    ///
    /// Returns the error of the action.
    pub async fn stop<F, Fut>(&self, action: F) -> CamelResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = CamelResult<()>>,
    {
        if !self.status().is_stoppable() {
            if matches!(
                self.status(),
                ServiceStatus::Initializing | ServiceStatus::Initialized
            ) {
                self.set(ServiceStatus::Stopped);
            }
            return Ok(());
        }
        self.set(ServiceStatus::Stopping);
        let r = action().await;
        self.set(ServiceStatus::Stopped);
        r
    }

    /// Runs the suspend action if the service is suspendable.
    ///
    /// # Errors
    ///
    /// Returns the error of the action (the status goes back to `Started`).
    pub async fn suspend<F, Fut>(&self, action: F) -> CamelResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = CamelResult<()>>,
    {
        if !self.status().is_suspendable() {
            return Ok(());
        }
        self.set(ServiceStatus::Suspending);
        match action().await {
            Ok(()) => {
                self.set(ServiceStatus::Suspended);
                Ok(())
            }
            Err(e) => {
                self.set(ServiceStatus::Started);
                Err(e)
            }
        }
    }

    /// Runs the resume action if the service is suspended.
    ///
    /// # Errors
    ///
    /// Returns the error of the action (the status stays `Suspended`).
    pub async fn resume<F, Fut>(&self, action: F) -> CamelResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = CamelResult<()>>,
    {
        if !self.status().is_suspended() {
            return Ok(());
        }
        self.set(ServiceStatus::Starting);
        match action().await {
            Ok(()) => {
                self.set(ServiceStatus::Started);
                Ok(())
            }
            Err(e) => {
                self.set(ServiceStatus::Suspended);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CamelError;
    use crate::processor::block_on;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn lifecycle() {
        let s = ServiceState::new();
        let calls = AtomicU32::new(0);
        s.build(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        s.build(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert_eq!(1, calls.load(Ordering::SeqCst));
        s.init(|| Ok(())).unwrap();
        assert_eq!(ServiceStatus::Initialized, s.status());
        block_on(s.start(|| async { Ok(()) })).unwrap();
        // starting again does nothing
        block_on(s.start(|| async { Err(CamelError::runtime("should not run")) })).unwrap();
        assert!(s.status().is_started());
        block_on(s.suspend(|| async { Ok(()) })).unwrap();
        assert!(s.status().is_suspended());
        block_on(s.resume(|| async { Ok(()) })).unwrap();
        assert!(s.status().is_started());
        block_on(s.stop(|| async { Ok(()) })).unwrap();
        assert!(s.status().is_stopped());
        block_on(s.stop(|| async { Err(CamelError::runtime("should not run")) })).unwrap();
        // restart after stop
        block_on(s.start(|| async { Ok(()) })).unwrap();
        assert!(s.status().is_started());
    }

    #[test]
    fn failures() {
        let s = ServiceState::new();
        assert!(block_on(s.start(|| async { Err(CamelError::runtime("boom")) })).is_err());
        assert!(s.status().is_stopped());
        block_on(s.start(|| async { Ok(()) })).unwrap();
        assert!(block_on(s.suspend(|| async { Err(CamelError::runtime("x")) })).is_err());
        assert!(s.status().is_started());
        assert!(block_on(s.stop(|| async { Err(CamelError::runtime("x")) })).is_err());
        assert!(s.status().is_stopped());
        let never = ServiceState::default();
        block_on(never.stop(|| async { Ok(()) })).unwrap();
        assert!(never.status().is_stopped());
    }

    struct Svc(ServiceState);
    impl Service for Svc {}
    impl StatefulService for Svc {
        fn status(&self) -> ServiceStatus {
            self.0.status()
        }
    }

    #[test]
    fn stateful_defaults() {
        let s = Svc(ServiceState::new());
        assert!(block_on(s.start()).is_ok() && block_on(s.stop()).is_ok());
        assert!(s.build().is_ok() && s.init().is_ok());
        assert!(!s.is_started() && !s.is_stopped() && !s.is_starting() && !s.is_stopping());
        assert!(!s.is_suspended() && !s.is_suspending());
        assert!(s.is_run_allowed());
    }
}
