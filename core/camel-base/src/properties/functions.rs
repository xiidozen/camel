//! The default functions of property placeholders (Java `EnvPropertiesFunction`,
//! `SysPropertiesFunction`, `ServicePropertiesFunction`, `ServiceHostPropertiesFunction`,
//! `ServicePortPropertiesFunction`, `BooleanPropertiesFunction`).

use camel_api::context::CamelContext;
use camel_api::exchange::Exchange;
use camel_api::spi::properties::PropertiesFunction;
use camel_util::io_helper::lookup_environment_variable;
use camel_util::system_properties;
use std::sync::{Arc, OnceLock, Weak};

/// The key and the default value of `key:default`.
fn key_and_default(remainder: &str) -> (&str, Option<&str>) {
    match remainder.split_once(':') {
        Some((k, d)) => (k, Some(d)),
        None => (remainder, None),
    }
}

/// `{{env:NAME:default}}`: the environment variable (also as upper case with `_` for `.` and
/// `-`).
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvPropertiesFunction;

impl PropertiesFunction for EnvPropertiesFunction {
    fn name(&self) -> &str {
        "env"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let (key, default) = key_and_default(remainder);
        lookup_environment_variable(key).or_else(|| default.map(str::to_string))
    }
}

/// `{{sys:name:default}}`: the system property (see `camel_util::system_properties`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SysPropertiesFunction;

impl PropertiesFunction for SysPropertiesFunction {
    fn name(&self) -> &str {
        "sys"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let (key, default) = key_and_default(remainder);
        system_properties::get_property(key).or_else(|| default.map(str::to_string))
    }
}

/// `{{service:name:default}}`: `host:port` of the service from the `NAME_SERVICE_HOST` and
/// `NAME_SERVICE_PORT` environment variables (as Kubernetes sets them).
#[derive(Debug, Default, Clone, Copy)]
pub struct ServicePropertiesFunction;

impl PropertiesFunction for ServicePropertiesFunction {
    fn name(&self) -> &str {
        "service"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let (key, default) = key_and_default(remainder);
        let host = lookup_environment_variable(&format!("{key}_SERVICE_HOST"));
        let port = lookup_environment_variable(&format!("{key}_SERVICE_PORT"));
        match (host, port) {
            (Some(h), Some(p)) => Some(format!("{h}:{p}")),
            _ => default.map(str::to_string),
        }
    }
}

/// `{{service.host:name:default}}`: the `NAME_SERVICE_HOST` environment variable.
#[derive(Debug, Default, Clone, Copy)]
pub struct ServiceHostPropertiesFunction;

impl PropertiesFunction for ServiceHostPropertiesFunction {
    fn name(&self) -> &str {
        "service.host"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let (key, default) = key_and_default(remainder);
        lookup_environment_variable(&format!("{key}_SERVICE_HOST"))
            .or_else(|| default.map(str::to_string))
    }
}

/// `{{service.port:name:default}}`: the `NAME_SERVICE_PORT` environment variable.
#[derive(Debug, Default, Clone, Copy)]
pub struct ServicePortPropertiesFunction;

impl PropertiesFunction for ServicePortPropertiesFunction {
    fn name(&self) -> &str {
        "service.port"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let (key, default) = key_and_default(remainder);
        lookup_environment_variable(&format!("{key}_SERVICE_PORT"))
            .or_else(|| default.map(str::to_string))
    }
}

/// `{{boolean:key operator value}}`: `true` or `false`, from the simple predicate
/// `${properties:key} operator value` (such as `{{boolean:myQueueSize > 5}}`); the context
/// is set when the component is added to it.
#[derive(Default)]
pub struct BooleanPropertiesFunction {
    context: OnceLock<Weak<dyn CamelContext>>,
}

impl BooleanPropertiesFunction {
    /// The function of the context.
    pub fn new(context: Weak<dyn CamelContext>) -> Self {
        let f = BooleanPropertiesFunction::default();
        let _ = f.context.set(context);
        f
    }
}

impl PropertiesFunction for BooleanPropertiesFunction {
    fn name(&self) -> &str {
        "boolean"
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let context: Arc<dyn CamelContext> = self.context.get()?.upgrade()?;
        let (key, condition) = match remainder.split_once(' ') {
            Some((k, c)) => (k, Some(c.trim())),
            None => (remainder, None),
        };
        let mut simple = format!("${{properties:{key}}}");
        if let Some(c) = condition {
            simple.push(' ');
            simple.push_str(c);
        }
        let language = context.resolve_language("simple").ok()?;
        let predicate = language.create_predicate(&simple).ok()?;
        let dummy = Exchange::new(context);
        let matches = predicate.matches(&dummy).ok()?;
        Some(matches.to_string())
    }
}
