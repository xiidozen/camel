//! # camel-controlbus
//!
//! The control bus component (Java `ControlBusComponent`): `controlbus:route?routeId=foo&action=start`
//! starts, stops, fails, suspends, resumes or restarts a route, or returns its status, and
//! `controlbus:language:simple` evaluates the message body as an expression of the language.
//! The route id `current` is the route of the exchange. With `async` the task runs on another
//! task and gives no result. This crate is the Rust port of the Java
//! `components/camel-controlbus` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `routeId` | | To specify a route by its id. The special keyword `current` indicates the current route. |
//! | `action` | | To denote an action that can be either: start, stop, fail, suspend, resume, restart, status or stats. |
//! | `restartDelay` | `1000` | The delay in millis to use when restarting a route. |
//! | `async` | `false` | Whether to execute the control bus task asynchronously. |
//! | `loggingLevel` | `INFO` | Logging level used for logging when task is done, or if any exceptions occurred during processing the task. |
//!
//! The `stats` action needs JMX in Java; here it answers that the stats are not available.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::enums::LoggingLevel;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::EXCEPTION_CAUGHT;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_api::service::Service;
use camel_api::spi::language::Language;
use camel_base_engine::context::DefaultCamelContext;
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use camel_util::value::Value;
use std::any::Any;
use std::sync::{Arc, Weak};
use std::time::Duration;

/// The control bus component.
#[derive(Default)]
pub struct ControlBusComponent;

impl ControlBusComponent {
    /// Creates the component.
    pub fn new() -> Self {
        ControlBusComponent
    }
}

impl Service for ControlBusComponent {}

impl Component for ControlBusComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let language = match remaining.strip_prefix("language:") {
            Some(l) => Some(context.resolve_language(l)?),
            None => None,
        };
        let action = get_and_remove_parameter::<String>(parameters, "action")?;
        if let Some(a) = &action {
            const ACTIONS: [&str; 8] = [
                "start", "stop", "fail", "suspend", "resume", "restart", "status", "stats",
            ];
            if !ACTIONS.contains(&a.as_str()) {
                return Err(CamelError::illegal_argument(format!(
                    "Invalid action: {a} (one of {})",
                    ACTIONS.join(",")
                )));
            }
        }
        let logging_level = match get_and_remove_parameter::<String>(parameters, "loggingLevel")? {
            Some(l) => l
                .parse::<LoggingLevel>()
                .map_err(|_| CamelError::illegal_argument(format!("Invalid loggingLevel: {l}")))?,
            None => LoggingLevel::INFO,
        };
        Ok(Arc::new(ControlBusEndpoint {
            uri: uri.to_string(),
            command: remaining.split(':').next().unwrap_or_default().to_string(),
            context: Arc::downgrade(context),
            language,
            route_id: get_and_remove_parameter(parameters, "routeId")?,
            action,
            restart_delay: get_and_remove_parameter::<u64>(parameters, "restartDelay")?
                .unwrap_or(1000),
            async_: get_and_remove_parameter(parameters, "async")?.unwrap_or(false),
            logging_level,
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("controlbus")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The control bus endpoint (Java `ControlBusEndpoint`), producer only.
pub struct ControlBusEndpoint {
    uri: String,
    command: String,
    context: Weak<dyn CamelContext>,
    language: Option<Arc<dyn Language>>,
    route_id: Option<String>,
    action: Option<String>,
    restart_delay: u64,
    async_: bool,
    logging_level: LoggingLevel,
}

impl ControlBusEndpoint {
    /// The command: `route` or `language`.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The route id of the action.
    pub fn route_id(&self) -> Option<&str> {
        self.route_id.as_deref()
    }

    /// The action on the route.
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    /// Whether the tasks run asynchronously.
    pub fn is_async(&self) -> bool {
        self.async_
    }

    fn log(&self, message: &str) {
        let level = match self.logging_level {
            LoggingLevel::TRACE => log::Level::Trace,
            LoggingLevel::DEBUG => log::Level::Debug,
            LoggingLevel::INFO => log::Level::Info,
            LoggingLevel::WARN => log::Level::Warn,
            LoggingLevel::ERROR => log::Level::Error,
            LoggingLevel::OFF => return,
        };
        log::log!(target: "org.apache.camel.component.controlbus.ControlBusProducer", level, "{message}");
    }

    fn default_context(&self) -> CamelResult<Arc<DefaultCamelContext>> {
        let context = self.camel_context();
        let weak = context
            .as_any()
            .downcast_ref::<DefaultCamelContext>()
            .map(DefaultCamelContext::downgrade)
            .ok_or_else(|| {
                CamelError::illegal_state("The control bus needs a DefaultCamelContext")
            })?;
        weak.upgrade()
            .ok_or_else(|| CamelError::illegal_state("The CamelContext is gone"))
    }

    /// Runs the action on the route (Java `ActionTask`); the result of `status` and `stats`.
    async fn action_task(
        &self,
        action: &str,
        id: Option<&str>,
        cause: Option<CamelError>,
    ) -> CamelResult<Option<Value>> {
        let context = self.default_context()?;
        let id_or_err =
            || id.ok_or_else(|| CamelError::illegal_argument("The routeId must be configured"));
        match action {
            "start" => context.start_route(id_or_err()?).await?,
            "stop" => context.stop_route(id_or_err()?).await?,
            "fail" => {
                let id = id_or_err()?;
                let cause = cause.unwrap_or_else(|| {
                    CamelError::new(
                        ErrorKind::RejectedExecution,
                        format!("Route {id} is forced stopped and marked as failed"),
                    )
                });
                context.stop_route_with_cause(id, cause).await?;
            }
            "suspend" => context.suspend_route(id_or_err()?).await?,
            "resume" => context.resume_route(id_or_err()?).await?,
            "restart" => {
                let id = id_or_err()?;
                context.stop_route(id).await?;
                if self.restart_delay > 0 {
                    tokio::time::sleep(Duration::from_millis(self.restart_delay)).await;
                }
                context.start_route(id).await?;
            }
            "status" => {
                return Ok(id
                    .and_then(|id| context.route_status(id))
                    .map(|s| Value::from(s.name())));
            }
            "stats" => return Ok(Some(Value::from("JMX is disabled, cannot get stats"))),
            _ => {}
        }
        Ok(None)
    }
}

impl Service for ControlBusEndpoint {}

impl Endpoint for ControlBusEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(ControlBusProducer { endpoint: self }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::runtime(format!(
            "Cannot consume from a ControlBusEndpoint: {}",
            self.uri
        )))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The control bus producer (Java `ControlBusProducer`).
struct ControlBusProducer {
    endpoint: Arc<ControlBusEndpoint>,
}

impl ControlBusProducer {
    /// Java `LanguageTask`: the body as an expression, evaluated on a copy.
    fn language_task(
        endpoint: &ControlBusEndpoint,
        language: &Arc<dyn Language>,
        exchange: &Exchange,
    ) -> Option<Value> {
        let mut copy = exchange.clone();
        let task = copy.message().body().to_string();
        let result = language
            .create_expression(&task)
            .and_then(|e| e.evaluate_mut(&mut copy));
        match result {
            Ok(v) => {
                let shown = if v.is_null() {
                    "void".to_string()
                } else {
                    v.to_string()
                };
                endpoint.log(&format!(
                    "ControlBus task done [{task}] with result -> {shown}"
                ));
                (!v.is_null()).then_some(v)
            }
            Err(e) => {
                endpoint.log(&format!(
                    "Error executing ControlBus task [{task}]. This exception will be ignored. {e}"
                ));
                None
            }
        }
    }

    async fn run_action(
        endpoint: Arc<ControlBusEndpoint>,
        action: String,
        id: Option<String>,
        cause: Option<CamelError>,
    ) -> Option<Value> {
        let task = format!("{action} route {}", id.as_deref().unwrap_or("null"));
        match endpoint.action_task(&action, id.as_deref(), cause).await {
            Ok(result) => {
                let shown = result
                    .as_ref()
                    .map_or("void".to_string(), |v| v.to_string());
                endpoint.log(&format!(
                    "ControlBus task done [{task}] with result -> {shown}"
                ));
                result
            }
            Err(e) => {
                endpoint.log(&format!(
                    "Error executing ControlBus task [{task}]. This exception will be ignored. {e}"
                ));
                None
            }
        }
    }
}

impl Service for ControlBusProducer {}

impl Processor for ControlBusProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let endpoint = self.endpoint.clone();
            if let Some(language) = &endpoint.language {
                if endpoint.async_ {
                    let (e, l, copy) = (endpoint.clone(), language.clone(), exchange.clone());
                    tokio::spawn(async move { Self::language_task(&e, &l, &copy) });
                } else if let Some(v) = Self::language_task(&endpoint, language, exchange) {
                    exchange.message_mut().set_body(v);
                }
            } else if let Some(action) = endpoint.action.clone() {
                let id = match endpoint.route_id.as_deref() {
                    Some("current") => exchange.route_id().map(str::to_string),
                    other => other.map(str::to_string),
                };
                let cause = exchange.exception().cloned().or_else(|| {
                    exchange
                        .property(EXCEPTION_CAUGHT)
                        .and_then(|v| v.downcast_ref::<CamelError>())
                        .cloned()
                });
                if endpoint.async_ {
                    tokio::spawn(Self::run_action(endpoint, action, id, cause));
                } else if let Some(v) = Self::run_action(endpoint, action, id, cause).await {
                    exchange.message_mut().set_body(v);
                }
            }
            Ok(())
        })
    }
}

impl Producer for ControlBusProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
