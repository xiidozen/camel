//! # camel-log
//!
//! The log component: `log:name?options` logs the exchanges sent to it with the logger `name`
//! (the `log` crate target), formatted by the
//! [`DefaultExchangeFormatter`],
//! or the throughput every `groupSize` messages. This crate is the Rust port of the Java
//! `components/camel-log` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `level` | `INFO` | The logging level (`TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`, `OFF`). |
//! | `groupSize` | | Log the throughput every n messages instead of each message. |
//! | `showAll`, `showExchangeId`, `showExchangePattern`, `showProperties`, `showAllProperties`, `showVariables`, `showHeaders`, `showBody` (`true`), `showBodyType` (`true`), `showException`, `showCaughtException`, `showStackTrace`, `showRouteId`, `showRouteGroup` | `false` | What to log. |
//! | `multiline` | `false` | Each information on a new line. |
//! | `skipBodyLineSeparator` | `true` | Log the body in one line. |
//! | `maxChars` | `10000` | Limits the number of characters logged per line. |
//! | `style` | `Default` | `Default`, `Tab` or `Fixed`. |
//! | `plain` | `false` | Only log the body. |
//!
//! Not ported yet: `groupInterval`, `groupDelay`, `groupActiveOnly`, `logMask`, `marker`,
//! `exchangeFormatter`, `sourceLocationLoggerName`, the stream/file options.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::enums::LoggingLevel;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_api::service::Service;
use camel_api::spi::exchange_formatter::ExchangeFormatter;
use camel_core_processor::log_processor::to_log_level;
use camel_support::component_support::get_and_remove_parameter;
use camel_support::exchange_formatter::{DefaultExchangeFormatter, OutputStyle};
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

/// The log component (Java `LogComponent`).
#[derive(Default)]
pub struct LogComponent;

impl LogComponent {
    /// Creates the component.
    pub fn new() -> Self {
        LogComponent
    }
}

impl Service for LogComponent {}

fn bool_option(parameters: &mut Parameters, name: &str, value: &mut bool) -> CamelResult<()> {
    if let Some(v) = get_and_remove_parameter::<bool>(parameters, name)? {
        *value = v;
    }
    Ok(())
}

impl Component for LogComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let level = match get_and_remove_parameter::<String>(parameters, "level")? {
            Some(l) => l
                .to_uppercase()
                .parse::<LoggingLevel>()
                .map_err(|e| CamelError::illegal_argument(e.to_string()))?,
            None => LoggingLevel::INFO,
        };
        let mut f = DefaultExchangeFormatter::default();
        for (name, field) in [
            ("showRouteId", &mut f.show_route_id),
            ("showRouteGroup", &mut f.show_route_group),
            ("showExchangeId", &mut f.show_exchange_id),
            ("showExchangePattern", &mut f.show_exchange_pattern),
            ("showProperties", &mut f.show_properties),
            ("showAllProperties", &mut f.show_all_properties),
            ("showVariables", &mut f.show_variables),
            ("showHeaders", &mut f.show_headers),
            ("skipBodyLineSeparator", &mut f.skip_body_line_separator),
            ("showBody", &mut f.show_body),
            ("showBodyType", &mut f.show_body_type),
            ("showException", &mut f.show_exception),
            ("showCaughtException", &mut f.show_caught_exception),
            ("showStackTrace", &mut f.show_stack_trace),
            ("showAll", &mut f.show_all),
            ("multiline", &mut f.multiline),
            ("plain", &mut f.plain),
        ] {
            bool_option(parameters, name, field)?;
        }
        if let Some(m) = get_and_remove_parameter::<i64>(parameters, "maxChars")? {
            f.max_chars = m as i32;
        }
        if let Some(s) = get_and_remove_parameter::<String>(parameters, "style")? {
            f.style = match s.as_str() {
                "Default" => OutputStyle::Default,
                "Tab" => OutputStyle::Tab,
                "Fixed" => OutputStyle::Fixed,
                other => {
                    return Err(CamelError::illegal_argument(format!(
                        "No enum constant OutputStyle.{other}"
                    )))
                }
            };
        }
        let group_size = get_and_remove_parameter::<i64>(parameters, "groupSize")?;
        if let Some(g) = group_size {
            if g <= 0 {
                return Err(CamelError::illegal_argument(format!(
                    "groupSize must be positive, was: {g}"
                )));
            }
        }
        Ok(Arc::new(LogEndpoint {
            uri: uri.to_string(),
            logger_name: remaining.to_string(),
            level,
            formatter: Arc::new(f),
            group_size: group_size.map(|g| g as u64),
            context: Arc::downgrade(context),
            throughput: Arc::new(Throughput::default()),
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("log")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A log endpoint (Java `LogEndpoint`).
pub struct LogEndpoint {
    uri: String,
    logger_name: String,
    level: LoggingLevel,
    formatter: Arc<dyn ExchangeFormatter>,
    group_size: Option<u64>,
    context: Weak<dyn CamelContext>,
    throughput: Arc<Throughput>,
}

impl LogEndpoint {
    /// The logger name.
    pub fn logger_name(&self) -> &str {
        &self.logger_name
    }

    /// The logging level.
    pub fn level(&self) -> LoggingLevel {
        self.level
    }

    /// The last throughput message (with `groupSize`).
    pub fn last_log_message(&self) -> Option<String> {
        self.throughput.last.lock().unwrap().clone()
    }

    /// The number of received messages (with `groupSize`).
    pub fn received_counter(&self) -> u64 {
        self.throughput.received.load(Ordering::SeqCst)
    }
}

impl Service for LogEndpoint {}

impl Endpoint for LogEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(LogProducer { endpoint: self }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            format!("You cannot consume from this endpoint: {}", self.uri),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Java `ThroughputLogger` (group size mode).
struct Throughput {
    received: AtomicU64,
    group_watch: Mutex<Instant>,
    test_watch: Instant,
    last: Mutex<Option<String>>,
}

impl Default for Throughput {
    fn default() -> Self {
        Throughput {
            received: AtomicU64::new(0),
            group_watch: Mutex::new(Instant::now()),
            test_watch: Instant::now(),
            last: Mutex::new(None),
        }
    }
}

/// Formats the number as Java `NumberFormat.getNumberInstance()` (English): grouping and at
/// most 3 fraction digits.
///
/// # Examples
///
/// ```
/// assert_eq!("1,234.568", camel_log::format_number(1234.5678));
/// assert_eq!("12", camel_log::format_number(12.0));
/// assert_eq!("∞", camel_log::format_number(f64::INFINITY));
/// ```
pub fn format_number(value: f64) -> String {
    if value.is_infinite() {
        return if value > 0.0 {
            "∞".to_string()
        } else {
            "-∞".to_string()
        };
    }
    if value.is_nan() {
        return "NaN".to_string();
    }
    let rounded = format!("{:.3}", value.abs());
    let (int, frac) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let frac = frac.trim_end_matches('0');
    let sign = if value < 0.0 && (int != "0" || !frac.is_empty()) {
        "-"
    } else {
        ""
    };
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

/// The producer of a log endpoint (Java `LogProducer` with `CamelLogProcessor` or
/// `ThroughputLogger`).
struct LogProducer {
    endpoint: Arc<LogEndpoint>,
}

impl Service for LogProducer {}

impl Processor for LogProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let e = &self.endpoint;
        if let Some(level) = to_log_level(e.level) {
            match e.group_size {
                Some(size) => {
                    let count = e.throughput.received.fetch_add(1, Ordering::SeqCst) + 1;
                    if count % size == 0 {
                        let group = {
                            let mut w = e.throughput.group_watch.lock().unwrap();
                            let d = w.elapsed().as_millis() as u64;
                            *w = Instant::now();
                            d
                        };
                        let test = e.throughput.test_watch.elapsed().as_millis() as u64;
                        let rate = (size as f64 * 1000.0) / group as f64;
                        let average = (count as f64 * 1000.0) / test as f64;
                        let msg = format!(
                            "Received: {count} messages so far. Last group took: {group} millis which is: {} messages per second. average: {}",
                            format_number(rate),
                            format_number(average)
                        );
                        log::log!(target: &e.logger_name, level, "{msg}");
                        *e.throughput.last.lock().unwrap() = Some(msg);
                    }
                }
                None => {
                    let listeners = exchange.context().log_listeners();
                    if !listeners.is_empty() || log::log_enabled!(target: &e.logger_name, level) {
                        let mut output = e.formatter.format(exchange);
                        // Java CamelLogProcessor.fireListeners
                        for l in &listeners {
                            output = l.on_log(exchange, &e.logger_name, &output);
                        }
                        log::log!(target: &e.logger_name, level, "{output}");
                    }
                }
            }
        }
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Producer for LogProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
