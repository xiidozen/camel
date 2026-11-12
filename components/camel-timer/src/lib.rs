//! The timer component (port of `components/camel-timer`): `timer:name?period=1000` generates
//! a message every period, on a tokio task.
//!
//! Options (Java `TimerEndpoint`):
//!
//! * `timerName` - the name of the timer (default the name in the uri)
//! * `period` - the interval between the messages, in millis or as a duration text (`5s`);
//!   default `1000`. With `0`, the timer fires once.
//! * `delay` - the time before the first message (default `1000`); a negative delay fires the
//!   messages one after the other without waiting
//! * `repeatCount` - the number of messages, `0` (the default) for no limit
//! * `fixedRate` - whether the messages are fired at a fixed rate (the period counted from the
//!   previous start) instead of a fixed delay (counted from the end of the previous one)
//! * `includeMetadata` - whether the exchange has the timer headers and properties
//!   (`CamelTimerCounter`, `CamelTimerName`, `CamelTimerPeriod`, `CamelTimerFiredTime`,
//!   `CamelMessageTimestamp`); default `false`
//! * `time` - the first time to fire (`yyyy-MM-dd HH:mm:ss` or `yyyy-MM-dd'T'HH:mm:ss`, in the
//!   default time zone); `pattern` is not available yet
//! * `runLoggingLevel` - the level of the started/completed log lines (default `TRACE`)
//! * `synchronous`, `daemon`, `timer` - accepted; a tokio task has no daemon flag nor shared
//!   `java.util.Timer`
//!
//! # Examples
//!
//! ```
//! use camel_core::prelude::*;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = camel_core::default_context();
//! context
//!     .add_routes(|r: &mut Routes| {
//!         r.add(from("timer:foo?delay=0&period=10&repeatCount=3").to("mock:result"));
//!     })
//!     .await
//!     .unwrap();
//! let mock = MockEndpoint::resolve(context.as_ref(), "mock:result").unwrap();
//! mock.expected_message_count(3);
//! context.start().await.unwrap();
//! mock.assert_is_satisfied().await;
//! context.stop().await.unwrap();
//! # });
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Consumer, Endpoint, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::enums::LoggingLevel;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange_constants::{
    MESSAGE_TIMESTAMP, TIMER_COUNTER, TIMER_FIRED_TIME, TIMER_NAME, TIMER_PERIOD, TIMER_TIME,
};
use camel_api::processor::{BoxFuture, SharedProcessor};
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_util::date::{Date, TimeZone};
use camel_util::uri_support::{sanitize_uri, Parameters};
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::Notify;

/// The timer component.
#[derive(Debug, Default)]
pub struct TimerComponent {
    include_metadata: AtomicBool,
}

impl TimerComponent {
    /// The component.
    pub fn new() -> Self {
        TimerComponent::default()
    }

    /// Whether the endpoints include the timer metadata by default (default `false`).
    pub fn set_include_metadata(&self, include: bool) {
        self.include_metadata.store(include, Ordering::Relaxed);
    }
}

impl Service for TimerComponent {}

/// A duration option: millis, or a duration text such as `5s` (Java `javaType =
/// "java.time.Duration"`).
fn duration_parameter(parameters: &mut Parameters, name: &str, default: i64) -> CamelResult<i64> {
    match get_and_remove_parameter::<String>(parameters, name)? {
        None => Ok(default),
        Some(text) => match text.trim().parse::<i64>() {
            Ok(v) => Ok(v),
            Err(_) => camel_util::time_utils::to_milli_seconds(&text).map_err(|e| {
                CamelError::new(
                    ErrorKind::PropertyBinding,
                    format!("Error binding property ({name}={text}) with name: {name}: {e}"),
                )
            }),
        },
    }
}

/// Parses the `time` option (Java `SimpleDateFormat` of the default patterns).
fn parse_time(text: &str) -> CamelResult<Date> {
    let invalid = || CamelError::illegal_argument(format!("Unparseable date: \"{text}\""));
    let (date, time) = text
        .split_once('T')
        .or_else(|| text.split_once(' '))
        .ok_or_else(invalid)?;
    let d: Vec<&str> = date.split('-').collect();
    let t: Vec<&str> = time.split(':').collect();
    if d.len() != 3 || t.len() != 3 {
        return Err(invalid());
    }
    let n = |s: &str| s.trim().parse::<i64>().map_err(|_| invalid());
    Ok(Date::of(
        &TimeZone::default_zone(),
        n(d[0])?,
        n(d[1])? as u32,
        n(d[2])? as u32,
        n(t[0])? as u32,
        n(t[1])? as u32,
        n(t[2])? as u32,
        0,
    ))
}

impl Component for TimerComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let time = match get_and_remove_parameter::<String>(parameters, "time")? {
            Some(t) => Some(parse_time(&t)?),
            None => None,
        };
        if get_and_remove_parameter::<String>(parameters, "pattern")?.is_some() {
            return Err(CamelError::illegal_argument(
                "The pattern option of the timer is not available in the Rust port yet",
            ));
        }
        let run_logging_level =
            match get_and_remove_parameter::<String>(parameters, "runLoggingLevel")? {
                Some(l) => l.parse::<LoggingLevel>().map_err(|_| {
                    CamelError::illegal_argument(format!("Unknown logging level: {l}"))
                })?,
                None => LoggingLevel::TRACE,
            };
        // accepted, without effect (no java.util.Timer)
        for name in ["daemon", "synchronous", "timer"] {
            let _: Option<String> = get_and_remove_parameter(parameters, name)?;
        }
        let endpoint = TimerEndpoint {
            uri: uri.to_string(),
            timer_name: get_and_remove_parameter::<String>(parameters, "timerName")?
                .unwrap_or_else(|| remaining.to_string()),
            period: duration_parameter(parameters, "period", 1000)?,
            delay: duration_parameter(parameters, "delay", 1000)?,
            repeat_count: get_and_remove_parameter(parameters, "repeatCount")?.unwrap_or(0),
            fixed_rate: get_and_remove_parameter(parameters, "fixedRate")?.unwrap_or(false),
            include_metadata: get_and_remove_parameter(parameters, "includeMetadata")?
                .unwrap_or(self.include_metadata.load(Ordering::Relaxed)),
            run_logging_level,
            time,
            context: Arc::downgrade(context),
        };
        Ok(Arc::new(endpoint))
    }

    fn default_name(&self) -> Option<&str> {
        Some("timer")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A timer endpoint (Java `TimerEndpoint`): consumers only.
pub struct TimerEndpoint {
    uri: String,
    timer_name: String,
    period: i64,
    delay: i64,
    repeat_count: i64,
    fixed_rate: bool,
    include_metadata: bool,
    run_logging_level: LoggingLevel,
    time: Option<Date>,
    context: Weak<dyn CamelContext>,
}

impl TimerEndpoint {
    /// The name of the timer.
    pub fn timer_name(&self) -> &str {
        &self.timer_name
    }

    /// The period in millis.
    pub fn period(&self) -> i64 {
        self.period
    }

    /// The delay in millis.
    pub fn delay(&self) -> i64 {
        self.delay
    }

    /// The number of messages (`0`: no limit).
    pub fn repeat_count(&self) -> i64 {
        self.repeat_count
    }

    /// Whether the timer fires at a fixed rate.
    pub fn is_fixed_rate(&self) -> bool {
        self.fixed_rate
    }

    /// Whether the timer metadata is included.
    pub fn is_include_metadata(&self) -> bool {
        self.include_metadata
    }

    /// The first time to fire, if set.
    pub fn time(&self) -> Option<Date> {
        self.time
    }
}

impl Service for TimerEndpoint {}

impl Endpoint for TimerEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Err(CamelError::new(
            ErrorKind::RuntimeCamel,
            format!("Cannot produce to a TimerEndpoint: {}", self.uri),
        ))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        Ok(Arc::new(TimerConsumer {
            endpoint: self,
            processor,
            state: Arc::new(State::default()),
            task: Mutex::new(None),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The state shared by the consumer and its task.
#[derive(Default)]
struct State {
    stopped: AtomicBool,
    wake: Notify,
    counter: AtomicI64,
}

/// The timer consumer (Java `TimerConsumer`).
pub struct TimerConsumer {
    endpoint: Arc<TimerEndpoint>,
    processor: SharedProcessor,
    state: Arc<State>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TimerConsumer {
    /// The number of messages fired so far.
    pub fn counter(&self) -> i64 {
        self.state.counter.load(Ordering::SeqCst)
    }
}

/// Sleeps the millis, or less if the consumer is stopped; `false` when stopped.
async fn sleep(state: &State, millis: i64) -> bool {
    if state.stopped.load(Ordering::SeqCst) {
        return false;
    }
    if millis > 0 {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(millis as u64)) => {}
            _ = state.wake.notified() => {}
        }
    }
    !state.stopped.load(Ordering::SeqCst)
}

fn log_run(level: LoggingLevel, what: &str, uri: &str) {
    let uri = sanitize_uri(uri);
    match level {
        LoggingLevel::ERROR => log::error!("Timer task {what} on: {uri}"),
        LoggingLevel::WARN => log::warn!("Timer task {what} on: {uri}"),
        LoggingLevel::INFO => log::info!("Timer task {what} on: {uri}"),
        LoggingLevel::DEBUG => log::debug!("Timer task {what} on: {uri}"),
        LoggingLevel::OFF => {}
        _ => log::trace!("Timer task {what} on: {uri}"),
    }
}

async fn fire(endpoint: &TimerEndpoint, processor: &SharedProcessor, count: i64) {
    let mut exchange = endpoint.create_exchange();
    if endpoint.include_metadata {
        exchange.set_property(TIMER_COUNTER, count);
        exchange.set_property(TIMER_NAME, endpoint.timer_name.as_str());
        if let Some(t) = endpoint.time {
            exchange.set_property(TIMER_TIME, t.to_value());
        }
        exchange.set_property(TIMER_PERIOD, endpoint.period);
        let now = Date::now();
        exchange.set_property(TIMER_FIRED_TIME, now.to_value());
        exchange
            .message_mut()
            .set_header(TIMER_FIRED_TIME, now.to_value());
        exchange
            .message_mut()
            .set_header(MESSAGE_TIMESTAMP, now.millis());
    }
    log::trace!("Timer {} is firing #{count} count", endpoint.timer_name);
    log_run(endpoint.run_logging_level, "started", &endpoint.uri);
    if let Err(e) = processor.process(&mut exchange).await {
        exchange.set_exception(Some(e));
    }
    if let Some(e) = exchange.exception() {
        log::warn!(
            "Error processing exchange. This exception will be ignored, to let the timer be able to trigger again. {exchange}. Caused by: [{} - {e}]",
            camel_api::error::error_type_name(e)
        );
    }
    log_run(endpoint.run_logging_level, "completed", &endpoint.uri);
}

async fn run(endpoint: Arc<TimerEndpoint>, processor: SharedProcessor, state: Arc<State>) {
    let repeat = endpoint.repeat_count;
    let allowed = |count: i64| repeat <= 0 || count <= repeat;
    if endpoint.delay < 0 && endpoint.time.is_none() {
        // fire one after the other, without waiting
        loop {
            let count = state.counter.fetch_add(1, Ordering::SeqCst) + 1;
            if !allowed(count) || state.stopped.load(Ordering::SeqCst) {
                break;
            }
            fire(&endpoint, &processor, count).await;
        }
        return;
    }
    let first = match endpoint.time {
        Some(t) => (t.millis() - Date::now().millis()).max(0),
        None => endpoint.delay,
    };
    if !sleep(&state, first).await {
        return;
    }
    loop {
        let started = tokio::time::Instant::now();
        let count = state.counter.fetch_add(1, Ordering::SeqCst) + 1;
        if !allowed(count) {
            log::debug!(
                "Cancelling {} timer as repeat count limit reached after {repeat} counts.",
                endpoint.timer_name
            );
            break;
        }
        fire(&endpoint, &processor, count).await;
        if endpoint.period <= 0 {
            // fires once
            break;
        }
        let wait = if endpoint.fixed_rate {
            endpoint.period - started.elapsed().as_millis() as i64
        } else {
            endpoint.period
        };
        if !sleep(&state, wait.max(0)).await {
            break;
        }
    }
}

impl Service for TimerConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            if tokio::runtime::Handle::try_current().is_err() {
                return Err(CamelError::illegal_state(
                    "The timer consumer must be started within a tokio runtime",
                ));
            }
            let mut task = self.task.lock().unwrap();
            if task.is_some() {
                return Ok(());
            }
            self.state.stopped.store(false, Ordering::SeqCst);
            *task = Some(tokio::spawn(run(
                self.endpoint.clone(),
                self.processor.clone(),
                self.state.clone(),
            )));
            Ok(())
        })
    }

    /// Stops the timer, waiting for the message in flight (graceful shutdown).
    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.state.stopped.store(true, Ordering::SeqCst);
            self.state.wake.notify_waiters();
            let task = self.task.lock().unwrap().take();
            if let Some(t) = task {
                let _ = t.await;
            }
            Ok(())
        })
    }
}

impl Consumer for TimerConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::parse_time;

    #[test]
    fn times() {
        let a = parse_time("2026-11-11 10:30:00").unwrap();
        let b = parse_time("2026-11-11T10:30:00").unwrap();
        assert_eq!(a, b);
        assert!(parse_time("tomorrow").is_err());
    }
}
