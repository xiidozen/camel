//! Bootstrapping a standalone Camel application (port of `core/camel-main`): [`Main`] creates
//! the CamelContext, loads the properties (`application.properties`, the profile properties,
//! initial and override properties), applies the `camel.main.*` and `camel.context.*`
//! options, adds the routes, starts the context and runs until it is completed, the duration
//! has elapsed or the process is interrupted (Ctrl-C).
//!
//! # Examples
//!
//! ```
//! use camel_core::prelude::*;
//! use camel_main::Main;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let mut main = Main::new();
//! main.configure().add_routes_builder(|r: &mut Routes| {
//!     r.add(from("direct:start").to("mock:result"));
//! });
//! main.bind("foo", 31);
//! main.start().await.unwrap();
//! let context = main.camel_context().unwrap();
//! assert_eq!(Some(Value::from(31)), context.simple_registry().lookup_by_name("foo"));
//! main.stop().await.unwrap();
//! # });
//! ```
//!
//! Not available yet: the configuration of the components, data formats and languages from
//! `camel.component.*` properties, the bean (`camel.beans.*`) and REST configurations, route
//! discovery by class or package scanning, the routes reload, the health checks, the vaults
//! and the duration in messages or idle time (`durationMaxMessages`, `durationMaxIdleSeconds`
//! are kept in the configuration but do not stop the application).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult};
use camel_api::spi::properties::PropertiesComponent;
use camel_core::prelude::*;
use camel_core::util::time_utils::to_milli_seconds;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

/// The default location of the properties (Java
/// `MainConstants.DEFAULT_PROPERTY_PLACEHOLDER_LOCATION`).
pub const DEFAULT_PROPERTY_PLACEHOLDER_LOCATION: &str =
    "classpath:application.properties;optional=true";

/// The location of the properties of a profile (`application-{profile}.properties`).
pub fn profile_property_placeholder_location(profile: &str) -> String {
    format!("classpath:application-{profile}.properties;optional=true")
}

/// A routes builder of the application.
pub type SharedRouteBuilder = Arc<dyn RouteBuilder + Send + Sync>;

/// The `camel.main.*` options (Java `MainConfigurationProperties`).
#[derive(Clone, Default)]
pub struct MainConfigurationProperties {
    name: Option<String>,
    profile: Option<String>,
    duration_max_seconds: i32,
    duration_max_messages: i32,
    duration_max_idle_seconds: i32,
    tracing: bool,
    tracing_standby: bool,
    message_history: bool,
    routes_builders: Vec<SharedRouteBuilder>,
}

/// Parses a duration in seconds: a plain number is in seconds, else a time with units
/// (`15s`, `2m`, `1h`, `500ms`) rounded up to whole seconds.
fn parse_duration_seconds(text: &str) -> CamelResult<i32> {
    let text = text.trim();
    if let Ok(n) = text.parse::<i32>() {
        return Ok(n);
    }
    let millis = to_milli_seconds(text)
        .map_err(|e| CamelError::illegal_argument(format!("Invalid duration {text}: {e}")))?;
    Ok(((millis + 999) / 1000) as i32)
}

fn parse_bool(key: &str, value: &str) -> CamelResult<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CamelError::illegal_argument(format!(
            "Invalid boolean value {value} of option {key}"
        ))),
    }
}

fn parse_int(key: &str, value: &str) -> CamelResult<i32> {
    value.trim().parse().map_err(|_| {
        CamelError::illegal_argument(format!("Invalid number value {value} of option {key}"))
    })
}

/// The option name without dashes, in lower case (Java matches the options ignoring case and
/// dashes: `duration-max-seconds` is `durationMaxSeconds`).
fn option_key(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '-')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl MainConfigurationProperties {
    /// The name of the CamelContext.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Sets the name of the CamelContext.
    pub fn with_name(&mut self, name: &str) -> &mut Self {
        self.name = Some(name.to_string());
        self
    }

    /// The profile (`dev`, `test`, `prod`, ...): the properties of
    /// `application-{profile}.properties` take precedence over `application.properties`.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// Sets the profile.
    pub fn with_profile(&mut self, profile: &str) -> &mut Self {
        self.profile = Some(profile.to_string());
        self
    }

    /// The seconds to run before stopping (0 or negative: run until stopped).
    pub fn duration_max_seconds(&self) -> i32 {
        self.duration_max_seconds
    }

    /// Sets the seconds to run before stopping.
    pub fn with_duration_max_seconds(&mut self, seconds: i32) -> &mut Self {
        self.duration_max_seconds = seconds;
        self
    }

    /// Sets the duration to run before stopping (Java `setDuration`, the alias of
    /// `durationMaxSeconds`): a plain number of seconds, or a time with units (`15s`, `2m`,
    /// `1h`, `500ms`, rounded up to whole seconds).
    ///
    /// # Errors
    ///
    /// An invalid duration.
    pub fn set_duration(&mut self, duration: &str) -> CamelResult<&mut Self> {
        self.duration_max_seconds = parse_duration_seconds(duration)?;
        Ok(self)
    }

    /// The number of messages to process before stopping (not acted upon yet).
    pub fn duration_max_messages(&self) -> i32 {
        self.duration_max_messages
    }

    /// Sets the number of messages to process before stopping.
    pub fn with_duration_max_messages(&mut self, messages: i32) -> &mut Self {
        self.duration_max_messages = messages;
        self
    }

    /// The idle seconds before stopping (not acted upon yet).
    pub fn duration_max_idle_seconds(&self) -> i32 {
        self.duration_max_idle_seconds
    }

    /// Sets the idle seconds before stopping.
    pub fn with_duration_max_idle_seconds(&mut self, seconds: i32) -> &mut Self {
        self.duration_max_idle_seconds = seconds;
        self
    }

    /// Whether tracing is enabled (default `false`).
    pub fn is_tracing(&self) -> bool {
        self.tracing
    }

    /// Sets whether tracing is enabled.
    pub fn with_tracing(&mut self, tracing: bool) -> &mut Self {
        self.tracing = tracing;
        self
    }

    /// Whether the tracer is set up in standby (enabled later).
    pub fn is_tracing_standby(&self) -> bool {
        self.tracing_standby
    }

    /// Sets whether the tracer is set up in standby.
    pub fn with_tracing_standby(&mut self, standby: bool) -> &mut Self {
        self.tracing_standby = standby;
        self
    }

    /// Whether message history is enabled.
    pub fn is_message_history(&self) -> bool {
        self.message_history
    }

    /// Sets whether message history is enabled.
    pub fn with_message_history(&mut self, enabled: bool) -> &mut Self {
        self.message_history = enabled;
        self
    }

    /// Adds a routes builder.
    pub fn add_routes_builder(
        &mut self,
        builder: impl RouteBuilder + Send + Sync + 'static,
    ) -> &mut Self {
        self.routes_builders.push(Arc::new(builder));
        self
    }

    /// The routes builders.
    pub fn routes_builders(&self) -> &[SharedRouteBuilder] {
        &self.routes_builders
    }

    /// Sets an option by its name (without the `camel.main.` prefix, ignoring case and
    /// dashes).
    ///
    /// # Returns
    ///
    /// Whether the option is known.
    ///
    /// # Errors
    ///
    /// An invalid value.
    pub fn set_option(&mut self, key: &str, value: &str) -> CamelResult<bool> {
        match option_key(key).as_str() {
            "name" => self.name = Some(value.to_string()),
            "profile" => self.profile = Some(value.to_string()),
            "duration" => self.duration_max_seconds = parse_duration_seconds(value)?,
            "durationmaxseconds" => self.duration_max_seconds = parse_int(key, value)?,
            "durationmaxmessages" => self.duration_max_messages = parse_int(key, value)?,
            "durationmaxidleseconds" => self.duration_max_idle_seconds = parse_int(key, value)?,
            "tracing" => self.tracing = parse_bool(key, value)?,
            "tracingstandby" => self.tracing_standby = parse_bool(key, value)?,
            "messagehistory" => self.message_history = parse_bool(key, value)?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// The events of the lifecycle of [`Main`] (Java `MainListener`); every method does nothing
/// by default.
pub trait MainListener: Send + Sync {
    /// Before the CamelContext is created.
    fn before_initialize(&self, _main: &Main) {}
    /// After the properties are set up, before the options are applied and the routes added.
    fn before_configure(&self, _main: &Main) {}
    /// After the routes are added.
    fn after_configure(&self, _main: &Main) {}
    /// Before the CamelContext starts.
    fn before_start(&self, _main: &Main) {}
    /// After the CamelContext started.
    fn after_start(&self, _main: &Main) {}
    /// Before the CamelContext stops.
    fn before_stop(&self, _main: &Main) {}
    /// After the CamelContext stopped.
    fn after_stop(&self, _main: &Main) {}
}

/// A standalone Camel application (Java `Main`).
pub struct Main {
    configuration: MainConfigurationProperties,
    default_property_placeholder_location: String,
    property_placeholder_locations: Option<String>,
    initial_properties: Vec<(String, String)>,
    override_properties: Vec<(String, String)>,
    beans: Vec<(String, Value)>,
    listeners: Vec<Arc<dyn MainListener>>,
    context: Option<Arc<DefaultCamelContext>>,
    started: bool,
    completed: Arc<Notify>,
}

impl Default for Main {
    fn default() -> Self {
        Self::new()
    }
}

impl Main {
    /// Creates the application.
    pub fn new() -> Self {
        Main {
            configuration: MainConfigurationProperties::default(),
            default_property_placeholder_location: DEFAULT_PROPERTY_PLACEHOLDER_LOCATION
                .to_string(),
            property_placeholder_locations: None,
            initial_properties: Vec::new(),
            override_properties: Vec::new(),
            beans: Vec::new(),
            listeners: Vec::new(),
            context: None,
            started: false,
            completed: Arc::new(Notify::new()),
        }
    }

    /// The `camel.main` configuration, to set the options and add the routes.
    pub fn configure(&mut self) -> &mut MainConfigurationProperties {
        &mut self.configuration
    }

    /// The `camel.main` configuration.
    pub fn configuration(&self) -> &MainConfigurationProperties {
        &self.configuration
    }

    /// Enables tracing.
    pub fn enable_trace(&mut self) {
        self.configuration.tracing = true;
    }

    /// Sets up tracing in standby.
    pub fn enable_trace_standby(&mut self) {
        self.configuration.tracing_standby = true;
    }

    /// Sets the default locations of the properties, used when no locations are set
    /// (comma separated; ending with `false` disables them).
    pub fn set_default_property_placeholder_location(&mut self, location: &str) {
        self.default_property_placeholder_location = location.to_string();
    }

    /// Sets the locations of the properties (comma separated), instead of the defaults.
    pub fn set_property_placeholder_locations(&mut self, locations: &str) {
        self.property_placeholder_locations = Some(locations.to_string());
    }

    /// Adds an initial property (the properties of the locations take precedence).
    pub fn add_initial_property(&mut self, key: &str, value: &str) {
        self.initial_properties
            .push((key.to_string(), value.to_string()));
    }

    /// Sets the initial properties.
    pub fn set_initial_properties<K: Into<String>, V: Into<String>>(
        &mut self,
        properties: impl IntoIterator<Item = (K, V)>,
    ) {
        self.initial_properties = properties
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
    }

    /// Adds an override property (taking precedence over all the other properties).
    pub fn add_override_property(&mut self, key: &str, value: &str) {
        self.override_properties
            .push((key.to_string(), value.to_string()));
    }

    /// Sets the override properties.
    pub fn set_override_properties<K: Into<String>, V: Into<String>>(
        &mut self,
        properties: impl IntoIterator<Item = (K, V)>,
    ) {
        self.override_properties = properties
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
    }

    /// Adds a property (Java `addProperty`: an initial property).
    pub fn add_property(&mut self, key: &str, value: &str) {
        self.add_initial_property(key, value);
    }

    /// Binds a bean in the registry (when starting, or right away if started).
    pub fn bind(&mut self, name: &str, bean: impl Into<Value>) {
        let bean = bean.into();
        if let Some(c) = &self.context {
            let _ = c.simple_registry().bind(name, bean.clone());
        }
        self.beans.push((name.to_string(), bean));
    }

    /// Adds a listener of the lifecycle.
    pub fn add_main_listener(&mut self, listener: impl MainListener + 'static) {
        self.listeners.push(Arc::new(listener));
    }

    /// The CamelContext (once started; kept after stopping).
    pub fn camel_context(&self) -> Option<&Arc<DefaultCamelContext>> {
        self.context.as_ref()
    }

    /// A producer template of the CamelContext (once started).
    pub fn camel_template(&self) -> Option<ProducerTemplate> {
        self.context.as_ref().map(|c| c.create_producer_template())
    }

    /// Parses the command line arguments (Java `MainCommandLineSupport`): `-d`/`-duration`,
    /// `-dm`/`-durationMaxMessages`, `-di`/`-durationIdle`, `-t`/`-trace`,
    /// `-ts`/`-traceStandby`, `-pl`/`-propertiesLocation`, `-p`/`-profile`.
    ///
    /// # Errors
    ///
    /// An unknown option or a missing or invalid value.
    pub fn parse_arguments<S: AsRef<str>>(&mut self, args: &[S]) -> CamelResult<()> {
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_ref();
            let name = arg.trim_start_matches('-');
            let mut value = || -> CamelResult<String> {
                i += 1;
                args.get(i).map(|v| v.as_ref().to_string()).ok_or_else(|| {
                    CamelError::illegal_argument(format!("Expected a value after {arg}"))
                })
            };
            match name {
                "d" | "duration" => {
                    let v = value()?;
                    self.configuration.set_duration(&v)?;
                }
                "dm" | "durationMaxMessages" => {
                    let v = value()?;
                    self.configuration.duration_max_messages = parse_int(arg, &v)?;
                }
                "di" | "durationIdle" => {
                    let v = value()?;
                    self.configuration.duration_max_idle_seconds = parse_int(arg, &v)?;
                }
                "t" | "trace" => self.configuration.tracing = true,
                "ts" | "traceStandby" => self.configuration.tracing_standby = true,
                "pl" | "propertiesLocation" => {
                    let v = value()?;
                    self.property_placeholder_locations = Some(v);
                }
                "p" | "profile" => {
                    let v = value()?;
                    self.configuration.profile = Some(v);
                }
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option: {arg}"
                    )))
                }
            }
            i += 1;
        }
        Ok(())
    }

    fn fire(&self, event: impl Fn(&dyn MainListener, &Main)) {
        for l in &self.listeners {
            event(l.as_ref(), self);
        }
    }

    /// The locations of the properties (Java `configurePropertiesService`): the given
    /// locations, else the profile properties and the default locations.
    fn locations(&self) -> Vec<String> {
        let locations = match &self.property_placeholder_locations {
            Some(l) => l.clone(),
            None => {
                let profile = std::env::var("CAMEL_MAIN_PROFILE")
                    .ok()
                    .or_else(|| self.configuration.profile.clone());
                let mut l = self.default_property_placeholder_location.clone();
                if let Some(p) = profile {
                    let loc = profile_property_placeholder_location(&p);
                    if !l.contains(&loc) {
                        l = format!("{loc},{l}");
                    }
                }
                l
            }
        };
        let locations = locations.trim();
        if locations.is_empty() || locations.ends_with("false") {
            return Vec::new();
        }
        locations
            .split(',')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Applies the `camel.main.*` and `camel.context.*` options of the properties.
    fn configure_from_properties(&mut self, context: &DefaultCamelContext) -> CamelResult<()> {
        let properties = context.properties().load_properties();
        let mut config = self.configuration.clone();
        let mut context_name = None;
        for (key, value) in &properties {
            if let Some(option) = key.strip_prefix("camel.main.") {
                let value = context
                    .resolve_property_placeholders(value)?
                    .unwrap_or_default();
                if !config.set_option(option, &value)? {
                    log::warn!("Unknown or not yet supported option: {key}={value}");
                }
            } else if let Some(option) = key.strip_prefix("camel.context.") {
                if option_key(option) == "name" {
                    context_name = context.resolve_property_placeholders(value)?;
                }
            }
        }
        if let Some(name) = context_name.or_else(|| config.name.clone()) {
            context.set_name(&name);
        }
        context.set_message_history(config.message_history);
        self.configuration = config;
        Ok(())
    }

    /// Creates, configures and starts the CamelContext.
    ///
    /// # Errors
    ///
    /// A properties location, an option or a route that cannot be loaded, or the context
    /// failing to start.
    pub async fn start(&mut self) -> CamelResult<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        self.fire(|l, m| l.before_initialize(m));
        let context = camel_core::default_context();
        let pc = context.properties();
        pc.set_initial_properties(self.initial_properties.clone());
        pc.set_override_properties(self.override_properties.clone());
        let locations = self.locations();
        let refs: Vec<&str> = locations.iter().map(String::as_str).collect();
        pc.set_locations(&refs)?;
        for (name, bean) in &self.beans {
            context.simple_registry().bind(name, bean.clone())?;
        }
        self.context = Some(context.clone());
        self.fire(|l, m| l.before_configure(m));
        self.configure_from_properties(&context)?;
        for builder in self.configuration.routes_builders.clone() {
            context.add_route_definitions(builder.build()).await?;
        }
        self.fire(|l, m| l.after_configure(m));
        self.fire(|l, m| l.before_start(m));
        context.start().await?;
        self.fire(|l, m| l.after_start(m));
        Ok(())
    }

    /// Stops the CamelContext.
    ///
    /// # Errors
    ///
    /// The context failing to stop.
    pub async fn stop(&mut self) -> CamelResult<()> {
        if !self.started {
            return Ok(());
        }
        self.started = false;
        self.fire(|l, m| l.before_stop(m));
        let context = self.context.clone().expect("the context");
        let result = context.stop().await;
        self.fire(|l, m| l.after_stop(m));
        result
    }

    /// A handle completing [`run`](Self::run) (Java `completed()`), usable from another task.
    pub fn completion(&self) -> MainCompletion {
        MainCompletion(self.completed.clone())
    }

    /// Completes [`run`](Self::run).
    pub fn completed(&self) {
        self.completed.notify_one();
    }

    /// Starts, runs until completed, the duration has elapsed (`durationMaxSeconds`) or the
    /// process is interrupted (Ctrl-C), then stops.
    ///
    /// # Errors
    ///
    /// See [`start`](Self::start) and [`stop`](Self::stop).
    pub async fn run(&mut self) -> CamelResult<()> {
        self.start().await?;
        let seconds = self.configuration.duration_max_seconds;
        let duration = async move {
            if seconds > 0 {
                tokio::time::sleep(Duration::from_secs(seconds as u64)).await;
                log::info!("Duration max seconds triggering shutdown of the JVM");
            } else {
                std::future::pending::<()>().await;
            }
        };
        let completed = self.completed.clone();
        tokio::select! {
            _ = completed.notified() => {}
            _ = duration => {}
            _ = tokio::signal::ctrl_c() => log::info!("Received hang up - stopping the main instance."),
        }
        self.stop().await
    }

    /// Whether tracing is enabled (the configuration; the Rust context has no tracer yet).
    pub fn is_tracing(&self) -> bool {
        self.configuration.tracing
    }
}

/// Completes [`Main::run`] from another task (Java `Main.completed()`).
#[derive(Clone)]
pub struct MainCompletion(Arc<Notify>);

impl MainCompletion {
    /// Completes the run.
    pub fn complete(&self) {
        self.0.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(15, parse_duration_seconds("15s").unwrap());
        assert_eq!(1, parse_duration_seconds("500ms").unwrap());
        assert_eq!("durationmaxseconds", option_key("duration-max-seconds"));
    }
}
