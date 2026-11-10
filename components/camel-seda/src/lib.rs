//! # camel-seda
//!
//! The SEDA component: `seda:name` sends exchanges to an in-memory queue, consumed
//! asynchronously by concurrent consumers (tokio tasks) of the route `from("seda:name")`. When
//! the exchange expects a reply (`InOut`), the producer waits for the consumer to finish and
//! copies the result back (Java `waitForTaskToComplete=IfReplyExpected`). The queues are shared
//! by the endpoints of the same component with the same name. This crate is the Rust port of
//! the Java `components/camel-seda` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `size` | `1000` | The maximum capacity of the queue. |
//! | `concurrentConsumers` | `1` | The number of concurrent consumers. |
//! | `waitForTaskToComplete` | `IfReplyExpected` | `Never`, `IfReplyExpected` or `Always`. |
//! | `timeout` | `30000` | The time (millis) to wait for the reply (0 forever). |
//! | `blockWhenFull` | `false` | Wait for space instead of failing when the queue is full. |
//! | `offerTimeout` | `0` | The time (millis) to wait for space with `blockWhenFull` (0 forever). |
//! | `discardWhenFull` | `false` | Discard the message when the queue is full. |
//! | `failIfNoConsumers` | `false` | Fail when the queue has no consumer. |
//! | `discardIfNoConsumers` | `false` | Discard the message when the queue has no consumer. |
//! | `purgeWhenStopping` | `false` | Purge the queue when the consumer stops. |
//!
//! Not ported yet: `multipleConsumers` (publish-subscribe), custom queues and queue factories,
//! polling consumers, `pollTimeout`, `virtualThreadPerTask`, `browseLimit`.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Consumer, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::enums::WaitForTaskToComplete;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_support::exchange_helper::{copy_results, create_correlated_copy};
use camel_util::uri_support::{sanitize_uri, Parameters};
use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{oneshot, Notify};

/// The Java class name of the error when there is no consumer.
pub const SEDA_CONSUMER_NOT_AVAILABLE: &str =
    "org.apache.camel.component.seda.SedaConsumerNotAvailableException";

/// The default queue size (Java `SedaConstants.QUEUE_SIZE`).
pub const QUEUE_SIZE: usize = 1000;

struct Item {
    exchange: Box<Exchange>,
    reply: Option<oneshot::Sender<Exchange>>,
}

/// An in-memory queue of exchanges (Java `QueueReference` with a `BlockingQueue`).
pub struct SedaQueue {
    items: Mutex<VecDeque<Item>>,
    capacity: usize,
    not_empty: Notify,
    not_full: Notify,
    consumers: AtomicUsize,
}

impl SedaQueue {
    fn new(capacity: usize) -> Self {
        SedaQueue {
            items: Mutex::new(VecDeque::new()),
            capacity,
            not_empty: Notify::new(),
            not_full: Notify::new(),
            consumers: AtomicUsize::new(0),
        }
    }

    /// The number of queued exchanges.
    pub fn len(&self) -> usize {
        self.items.lock().unwrap().len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of active consumers.
    pub fn consumer_count(&self) -> usize {
        self.consumers.load(Ordering::SeqCst)
    }

    fn try_offer(&self, item: Item) -> Result<(), Item> {
        let mut items = self.items.lock().unwrap();
        if items.len() >= self.capacity {
            return Err(item);
        }
        items.push_back(item);
        drop(items);
        self.not_empty.notify_one();
        Ok(())
    }

    /// Adds the item, waiting for space up to the timeout (`None` forever).
    async fn offer_wait(&self, mut item: Item, timeout: Option<Duration>) -> Result<(), Item> {
        let deadline = timeout.map(|t| tokio::time::Instant::now() + t);
        loop {
            let notified = self.not_full.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.try_offer(item) {
                Ok(()) => return Ok(()),
                Err(back) => item = back,
            }
            match deadline {
                Some(d) => {
                    if tokio::time::timeout_at(d, notified).await.is_err() {
                        return self.try_offer(item);
                    }
                }
                None => notified.await,
            }
        }
    }

    async fn take(&self, stopped: &AtomicBool) -> Option<Item> {
        loop {
            let notified = self.not_empty.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if stopped.load(Ordering::SeqCst) {
                return None;
            }
            let item = self.items.lock().unwrap().pop_front();
            if let Some(i) = item {
                self.not_full.notify_one();
                return Some(i);
            }
            notified.await;
        }
    }

    fn remove(&self, exchange_id: &str) {
        self.items
            .lock()
            .unwrap()
            .retain(|i| i.exchange.exchange_id() != exchange_id);
    }

    fn purge(&self) {
        self.items.lock().unwrap().clear();
    }
}

/// The SEDA component (Java `SedaComponent`).
pub struct SedaComponent {
    queues: Mutex<HashMap<String, Arc<SedaQueue>>>,
}

impl Default for SedaComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl SedaComponent {
    /// Creates the component.
    pub fn new() -> Self {
        SedaComponent {
            queues: Mutex::new(HashMap::new()),
        }
    }

    /// The queue of the key (the endpoint uri without options, such as `seda://foo`), if
    /// created.
    pub fn queue(&self, key: &str) -> Option<Arc<SedaQueue>> {
        self.queues.lock().unwrap().get(key).cloned()
    }

    fn get_or_create_queue(&self, key: &str, size: Option<usize>) -> CamelResult<Arc<SedaQueue>> {
        let mut queues = self.queues.lock().unwrap();
        if let Some(q) = queues.get(key) {
            if let Some(s) = size {
                if s != q.capacity {
                    return Err(CamelError::illegal_argument(format!(
                        "Cannot use existing queue {key} as the existing queue size {} does not match given queue size {s}",
                        q.capacity
                    )));
                }
            }
            return Ok(q.clone());
        }
        let q = Arc::new(SedaQueue::new(size.unwrap_or(QUEUE_SIZE)));
        queues.insert(key.to_string(), q.clone());
        Ok(q)
    }
}

impl Service for SedaComponent {}

impl Component for SedaComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let size = get_and_remove_parameter::<i64>(parameters, "size")?.map(|s| s.max(1) as usize);
        let wait = match get_and_remove_parameter::<String>(parameters, "waitForTaskToComplete")? {
            Some(w) => w
                .parse::<WaitForTaskToComplete>()
                .map_err(|e| CamelError::illegal_argument(e.to_string()))?,
            None => WaitForTaskToComplete::IfReplyExpected,
        };
        if get_and_remove_parameter::<bool>(parameters, "multipleConsumers")?.unwrap_or(false) {
            return Err(CamelError::new(
                ErrorKind::UnsupportedOperation,
                "The seda option multipleConsumers is not supported yet by the Rust port",
            ));
        }
        // Java getQueueKey: the uri without the options, such as seda://foo
        let key = camel_util::string_helper::before(uri, "?").unwrap_or_else(|| uri.to_string());
        let queue = self.get_or_create_queue(&key, size)?;
        let key = remaining.trim_start_matches('/').to_string();
        Ok(Arc::new(SedaEndpoint {
            uri: uri.to_string(),
            name: key,
            context: Arc::downgrade(context),
            queue,
            concurrent_consumers: get_and_remove_parameter::<i64>(
                parameters,
                "concurrentConsumers",
            )?
            .unwrap_or(1)
            .max(1) as usize,
            wait,
            timeout: get_and_remove_parameter::<u64>(parameters, "timeout")?.unwrap_or(30000),
            block_when_full: get_and_remove_parameter(parameters, "blockWhenFull")?
                .unwrap_or(false),
            offer_timeout: get_and_remove_parameter::<u64>(parameters, "offerTimeout")?
                .unwrap_or(0),
            discard_when_full: get_and_remove_parameter(parameters, "discardWhenFull")?
                .unwrap_or(false),
            fail_if_no_consumers: get_and_remove_parameter(parameters, "failIfNoConsumers")?
                .unwrap_or(false),
            discard_if_no_consumers: get_and_remove_parameter(parameters, "discardIfNoConsumers")?
                .unwrap_or(false),
            purge_when_stopping: get_and_remove_parameter(parameters, "purgeWhenStopping")?
                .unwrap_or(false),
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("seda")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A SEDA endpoint (Java `SedaEndpoint`).
pub struct SedaEndpoint {
    uri: String,
    name: String,
    context: Weak<dyn CamelContext>,
    queue: Arc<SedaQueue>,
    concurrent_consumers: usize,
    wait: WaitForTaskToComplete,
    timeout: u64,
    block_when_full: bool,
    offer_timeout: u64,
    discard_when_full: bool,
    fail_if_no_consumers: bool,
    discard_if_no_consumers: bool,
    purge_when_stopping: bool,
}

impl SedaEndpoint {
    /// The queue name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The queue.
    pub fn queue(&self) -> &Arc<SedaQueue> {
        &self.queue
    }

    /// The number of concurrent consumers.
    pub fn concurrent_consumers(&self) -> usize {
        self.concurrent_consumers
    }
}

impl Service for SedaEndpoint {}

impl Endpoint for SedaEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(SedaProducer { endpoint: self }))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        Ok(Arc::new(SedaConsumer {
            endpoint: self,
            processor,
            stopped: Arc::new(AtomicBool::new(false)),
            tasks: Mutex::new(Vec::new()),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The SEDA producer (Java `SedaProducer`).
struct SedaProducer {
    endpoint: Arc<SedaEndpoint>,
}

impl SedaProducer {
    fn not_available(&self, message: &str, exchange: &Exchange) -> CamelError {
        CamelError::camel_exchange(
            &format!("{message}: {}", sanitize_uri(&self.endpoint.uri)),
            exchange,
        )
        .with_subtype(SEDA_CONSUMER_NOT_AVAILABLE)
    }

    /// Java `addToQueue` / `offerToQueue`: `Ok(false)` when discarded.
    async fn add_to_queue(&self, item: Item) -> CamelResult<bool> {
        let e = &self.endpoint;
        if e.queue.consumer_count() == 0 {
            if e.fail_if_no_consumers {
                return Err(
                    self.not_available("No consumers available on endpoint", &item.exchange)
                );
            } else if e.discard_if_no_consumers {
                return Ok(false);
            }
        }
        if e.discard_when_full {
            return Ok(e.queue.try_offer(item).is_ok());
        }
        if e.block_when_full {
            let timeout = (e.offer_timeout > 0).then(|| Duration::from_millis(e.offer_timeout));
            return match e.queue.offer_wait(item, timeout).await {
                Ok(()) => Ok(true),
                Err(_) => Err(CamelError::illegal_state(format!(
                    "Fails to insert element into queue, after timeout of {} milliseconds",
                    e.offer_timeout
                ))),
            };
        }
        match e.queue.try_offer(item) {
            Ok(()) => Ok(true),
            Err(_) => Err(CamelError::illegal_state("Queue full")),
        }
    }
}

impl Service for SedaProducer {}

impl Processor for SedaProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let e = &self.endpoint;
            let wait = e.wait == WaitForTaskToComplete::Always
                || e.wait == WaitForTaskToComplete::IfReplyExpected
                    && exchange.pattern().is_out_capable();
            let copy = create_correlated_copy(exchange);
            if !wait {
                if let Err(err) = self
                    .add_to_queue(Item {
                        exchange: Box::new(copy),
                        reply: None,
                    })
                    .await
                {
                    exchange.set_exception(Some(err));
                }
                return Ok(());
            }
            let copy_id = copy.exchange_id().to_string();
            let (tx, rx) = oneshot::channel();
            if let Err(err) = self
                .add_to_queue(Item {
                    exchange: Box::new(copy),
                    reply: Some(tx),
                })
                .await
            {
                exchange.set_exception(Some(err));
                return Ok(());
            }
            let result = if e.timeout > 0 {
                match tokio::time::timeout(Duration::from_millis(e.timeout), rx).await {
                    Ok(r) => r.ok(),
                    Err(_) => {
                        e.queue.remove(&copy_id);
                        exchange.set_exception(Some(CamelError::exchange_timed_out(
                            &*exchange, e.timeout, None,
                        )));
                        return Ok(());
                    }
                }
            } else {
                rx.await.ok()
            };
            match result {
                Some(response) => copy_results(exchange, &response),
                None => exchange.set_exception(Some(CamelError::camel_exchange(
                    "The seda consumer stopped before completing the exchange",
                    &*exchange,
                ))),
            }
            Ok(())
        })
    }
}

impl Producer for SedaProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}

/// The SEDA consumer (Java `SedaConsumer`): concurrent tokio tasks taking exchanges from the
/// queue and routing them.
struct SedaConsumer {
    endpoint: Arc<SedaEndpoint>,
    processor: SharedProcessor,
    stopped: Arc<AtomicBool>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl Service for SedaConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            if tokio::runtime::Handle::try_current().is_err() {
                return Err(CamelError::illegal_state(
                    "The seda consumer must be started within a tokio runtime",
                ));
            }
            self.stopped.store(false, Ordering::SeqCst);
            self.endpoint.queue.consumers.fetch_add(1, Ordering::SeqCst);
            let mut tasks = self.tasks.lock().unwrap();
            for _ in 0..self.endpoint.concurrent_consumers {
                let queue = self.endpoint.queue.clone();
                let stopped = self.stopped.clone();
                let processor = self.processor.clone();
                let uri = self.endpoint.uri.clone();
                tasks.push(tokio::spawn(async move {
                    while let Some(item) = queue.take(&stopped).await {
                        let mut exchange = *item.exchange;
                        if let Err(e) = processor.process(&mut exchange).await {
                            exchange.set_exception(Some(e));
                        }
                        match item.reply {
                            Some(tx) => {
                                let _ = tx.send(exchange);
                            }
                            None => {
                                if let Some(e) = exchange.exception() {
                                    log::warn!("Error processing exchange. {exchange}. Caused by: [{} - {e}] on seda endpoint {}", camel_api::error::error_type_name(e), sanitize_uri(&uri));
                                }
                            }
                        }
                    }
                }));
            }
            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            if self.stopped.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            self.endpoint.queue.consumers.fetch_sub(1, Ordering::SeqCst);
            self.endpoint.queue.not_empty.notify_waiters();
            let tasks: Vec<_> = self.tasks.lock().unwrap().drain(..).collect();
            for t in tasks {
                let _ = t.await;
            }
            if self.endpoint.purge_when_stopping {
                self.endpoint.queue.purge();
            }
            Ok(())
        })
    }
}

impl Consumer for SedaConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}
