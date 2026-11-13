//! The default polling consumer of the endpoints without one (Java
//! `EventDrivenPollingConsumer`): the consumer of the endpoint puts the exchanges in a queue the
//! polls take them from.

use camel_api::endpoint::{Consumer, PollingConsumer, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor};
use camel_api::service::Service;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// The default capacity of the queue (Java 1000).
pub const DEFAULT_QUEUE_SIZE: usize = 1000;

struct Queue {
    items: Mutex<VecDeque<Exchange>>,
    capacity: usize,
    not_empty: Notify,
    not_full: Notify,
}

impl Queue {
    fn take(&self) -> Option<Exchange> {
        let e = self.items.lock().unwrap().pop_front();
        if e.is_some() {
            self.not_full.notify_one();
        }
        e
    }
}

/// The processor of the consumer: puts the exchanges in the queue, waiting for space.
struct QueueProcessor(Arc<Queue>);

impl Processor for QueueProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            loop {
                let notified = self.0.not_full.notified();
                {
                    let mut items = self.0.items.lock().unwrap();
                    if self.0.capacity == 0 || items.len() < self.0.capacity {
                        items.push_back(exchange.clone());
                        drop(items);
                        self.0.not_empty.notify_one();
                        return Ok(());
                    }
                }
                notified.await;
            }
        })
    }
}

/// A polling consumer on the (event driven) consumer of an endpoint (Java
/// `EventDrivenPollingConsumer`).
pub struct EventDrivenPollingConsumer {
    endpoint: SharedEndpoint,
    queue: Arc<Queue>,
    consumer: Mutex<Option<Arc<dyn Consumer>>>,
}

impl EventDrivenPollingConsumer {
    /// The polling consumer of the endpoint, with a queue of the capacity (0: unbounded).
    pub fn new(endpoint: SharedEndpoint, capacity: usize) -> Self {
        EventDrivenPollingConsumer {
            endpoint,
            queue: Arc::new(Queue {
                items: Mutex::new(VecDeque::new()),
                capacity,
                not_empty: Notify::new(),
                not_full: Notify::new(),
            }),
            consumer: Mutex::new(None),
        }
    }

    /// The number of queued exchanges.
    pub fn queue_size(&self) -> usize {
        self.queue.items.lock().unwrap().len()
    }

    fn check_started(&self) -> CamelResult<()> {
        if self.consumer.lock().unwrap().is_none() {
            return Err(CamelError::new(
                ErrorKind::RejectedExecution,
                format!(
                    "EventDrivenPollingConsumer[{}] is not started",
                    self.endpoint.endpoint_uri()
                ),
            ));
        }
        Ok(())
    }
}

impl Service for EventDrivenPollingConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            if self.consumer.lock().unwrap().is_some() {
                return Ok(());
            }
            let consumer = self
                .endpoint
                .clone()
                .create_consumer(Arc::new(QueueProcessor(self.queue.clone())))?;
            *self.consumer.lock().unwrap() = Some(consumer.clone());
            consumer.start().await
        })
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let consumer = self.consumer.lock().unwrap().take();
            match consumer {
                Some(c) => c.stop().await,
                None => Ok(()),
            }
        })
    }
}

impl PollingConsumer for EventDrivenPollingConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn receive(&self) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(async move {
            self.check_started()?;
            loop {
                let notified = self.queue.not_empty.notified();
                if let Some(e) = self.queue.take() {
                    return Ok(Some(e));
                }
                if self.consumer.lock().unwrap().is_none() {
                    return Ok(None);
                }
                notified.await;
            }
        })
    }

    fn receive_no_wait(&self) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(async move {
            self.check_started()?;
            Ok(self.queue.take())
        })
    }

    fn receive_timeout(&self, timeout: Duration) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(async move {
            self.check_started()?;
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                let notified = self.queue.not_empty.notified();
                if let Some(e) = self.queue.take() {
                    return Ok(Some(e));
                }
                if tokio::time::timeout_at(deadline, notified).await.is_err() {
                    return Ok(self.queue.take());
                }
            }
        })
    }
}

/// The polling consumer of the endpoint: its own, or an [`EventDrivenPollingConsumer`] on its
/// consumer (Java `DefaultEndpoint.createPollingConsumer`).
///
/// # Errors
///
/// The endpoint fails to create its polling consumer.
pub fn polling_consumer_of(endpoint: SharedEndpoint) -> CamelResult<Arc<dyn PollingConsumer>> {
    match endpoint.clone().create_polling_consumer() {
        Err(e) if e.is(ErrorKind::UnsupportedOperation) => Ok(Arc::new(
            EventDrivenPollingConsumer::new(endpoint, DEFAULT_QUEUE_SIZE),
        )),
        other => other,
    }
}
