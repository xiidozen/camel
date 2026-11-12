//! Helpers of the route policies (Java `RoutePolicySupport`): stopping and starting the consumer
//! of a route from a policy callback.
//!
//! A callback runs while the consumer routes an exchange, and stopping a polling consumer
//! waits for its polling task, so the consumer is stopped (or started) on another task, as the
//! Java consumer service is stopped without waiting for its threads.

use camel_api::endpoint::SharedConsumer;

/// Stops the consumer (Java `RoutePolicySupport.stopConsumer`), on another task when in a
/// tokio runtime.
pub fn stop_consumer(consumer: SharedConsumer) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(async move {
                if let Err(e) = consumer.stop().await {
                    log::warn!("Error stopping consumer: {e}");
                }
            });
        }
        Err(_) => {
            if let Err(e) = camel_api::processor::block_on(consumer.stop()) {
                log::warn!("Error stopping consumer: {e}");
            }
        }
    }
}

/// Starts the consumer (Java `RoutePolicySupport.startConsumer`), on another task when in a
/// tokio runtime.
pub fn start_consumer(consumer: SharedConsumer) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(async move {
                if let Err(e) = consumer.start().await {
                    log::warn!("Error starting consumer: {e}");
                }
            });
        }
        Err(_) => {
            if let Err(e) = camel_api::processor::block_on(consumer.start()) {
                log::warn!("Error starting consumer: {e}");
            }
        }
    }
}
