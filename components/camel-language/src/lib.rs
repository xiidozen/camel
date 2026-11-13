//! # camel-language
//!
//! The language component (Java `LanguageComponent`): `language:name:script` evaluates the
//! script in the language (such as `simple`, `constant` or `header`) on each exchange, by
//! default replacing the message body with the result. The script is given in the uri (url
//! encoded), loaded from a resource (`classpath:` or `file:`, on each exchange, or once with
//! the `resource:` prefix while `contentCache` is on), referred to in the registry (`#name`),
//! or sent in the `CamelLanguageScript` header with `allowTemplateFromHeader`. This crate is the
//! Rust port of the Java `components/camel-language` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `script` | | Sets the script to execute. |
//! | `transform` | `true` | Whether the result of the script should be used as message body. |
//! | `cacheScript` | `false` | Whether to cache the compiled script and reuse it. |
//! | `allowTemplateFromHeader` | `false` | Whether to allow to use the script from the `CamelLanguageScript` header. |
//! | `contentCache` | `true` | Sets whether to use the resource content cache or not (`resource:` scripts). |
//!
//! Not ported yet: `binary`, `resultType`, an `Expression` object in the header.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::LANGUAGE_SCRIPT;
use camel_api::processor::{BoxFuture, Processor, SharedExpression, SharedProcessor};
use camel_api::service::Service;
use camel_api::spi::language::Language;
use camel_support::component_support::get_and_remove_parameter;
use camel_support::resource_helper::{has_scheme, resolve_resource_as_string};
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::{Arc, Mutex, Weak};

/// The prefix of a script loaded once from a resource (Java `LanguageComponent.RESOURCE`).
pub const RESOURCE: &str = "resource:";

/// The language component.
pub struct LanguageComponent {
    content_cache: Mutex<bool>,
    allow_template_from_header: Mutex<bool>,
}

impl Default for LanguageComponent {
    fn default() -> Self {
        LanguageComponent {
            content_cache: Mutex::new(true),
            allow_template_from_header: Mutex::new(false),
        }
    }
}

impl LanguageComponent {
    /// Creates the component.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to use the resource content cache by default.
    pub fn set_content_cache(&self, cache: bool) {
        *self.content_cache.lock().unwrap() = cache;
    }

    /// Sets whether the endpoints allow the script from the header by default.
    pub fn set_allow_template_from_header(&self, allow: bool) {
        *self.allow_template_from_header.lock().unwrap() = allow;
    }
}

impl Service for LanguageComponent {}

impl Component for LanguageComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let (name, script) = match remaining.split_once(':') {
            Some((n, s)) => (n.to_string(), Some(s.to_string())),
            None => (remaining.to_string(), None),
        };
        if name.is_empty() {
            return Err(CamelError::illegal_argument(format!(
                "Illegal syntax. Name of language not given in uri: {uri}"
            )));
        }
        let language = context.resolve_language(&name)?;
        let (script, resource_uri) = match script {
            Some(s) => match s.strip_prefix(RESOURCE) {
                Some(r) => (None, Some(r.to_string())),
                None => (
                    Some(camel_util::net::url_decode(&s).map_err(CamelError::illegal_argument)?),
                    None,
                ),
            },
            None => (None, None),
        };
        let script = get_and_remove_parameter::<String>(parameters, "script")?.or(script);
        let bool_param = |parameters: &mut Parameters, name: &str, default: bool| {
            get_and_remove_parameter::<bool>(parameters, name).map(|v| v.unwrap_or(default))
        };
        let transform = bool_param(parameters, "transform", true)?;
        let cache_script = bool_param(parameters, "cacheScript", false)?;
        let allow_template_from_header = bool_param(
            parameters,
            "allowTemplateFromHeader",
            *self.allow_template_from_header.lock().unwrap(),
        )?;
        let content_cache = bool_param(
            parameters,
            "contentCache",
            *self.content_cache.lock().unwrap(),
        )?;
        let endpoint = LanguageEndpoint {
            uri: uri.to_string(),
            language_name: name,
            language,
            context: Arc::downgrade(context),
            script,
            resource_uri,
            transform,
            cache_script,
            allow_template_from_header,
            content_cache,
            expression: Mutex::new(None),
            resource_content: Mutex::new(None),
        };
        // a cached script is compiled up front (Java doInit)
        if endpoint.cache_script {
            if let Some(s) = &endpoint.script {
                if !(s.starts_with("file:") || s.starts_with("http:")) {
                    let s = endpoint.resolve_script(s)?;
                    let e = endpoint.create_expression(&s)?;
                    *endpoint.expression.lock().unwrap() = Some(e);
                }
            }
        }
        Ok(Arc::new(endpoint))
    }

    fn default_name(&self) -> Option<&str> {
        Some("language")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The language endpoint (Java `LanguageEndpoint`), producer only.
pub struct LanguageEndpoint {
    uri: String,
    language_name: String,
    language: Arc<dyn Language>,
    context: Weak<dyn CamelContext>,
    script: Option<String>,
    resource_uri: Option<String>,
    transform: bool,
    cache_script: bool,
    allow_template_from_header: bool,
    content_cache: bool,
    expression: Mutex<Option<SharedExpression>>,
    resource_content: Mutex<Option<String>>,
}

impl LanguageEndpoint {
    /// The name of the language.
    pub fn language_name(&self) -> &str {
        &self.language_name
    }

    /// The script, if given in the uri or the `script` option.
    pub fn script(&self) -> Option<&str> {
        self.script.as_deref()
    }

    /// The resource of the script (with the `resource:` prefix in the uri).
    pub fn resource_uri(&self) -> Option<&str> {
        self.resource_uri.as_deref()
    }

    /// The cached expression (with `cacheScript`).
    pub fn expression(&self) -> Option<SharedExpression> {
        self.expression.lock().unwrap().clone()
    }

    /// Clears the cached resource content and expression (Java `clearContentCache`).
    pub fn clear_content_cache(&self) {
        *self.resource_content.lock().unwrap() = None;
        *self.expression.lock().unwrap() = None;
    }

    fn create_expression(&self, script: &str) -> CamelResult<SharedExpression> {
        let e = self.language.create_expression(script)?;
        e.init(&self.camel_context())?;
        Ok(e)
    }

    /// The script of a resource or a bean reference, else the script itself (Java
    /// `resolveScript`).
    fn resolve_script(&self, script: &str) -> CamelResult<String> {
        if has_scheme(script) {
            resolve_resource_as_string(script)
        } else if let Some(name) = script.strip_prefix('#') {
            let name = name.strip_prefix("bean:").unwrap_or(name);
            self.camel_context()
                .registry()
                .and_then(|r| r.lookup_by_name(name))
                .map(|v| v.to_string())
                .ok_or_else(|| CamelError::no_such_bean_of_type(Some(name), "java.lang.String"))
        } else {
            Ok(script.to_string())
        }
    }

    /// The content of the `resource:` script, cached with `contentCache`.
    fn resource_content(&self, uri: &str) -> CamelResult<String> {
        if self.content_cache {
            if let Some(c) = self.resource_content.lock().unwrap().clone() {
                return Ok(c);
            }
        }
        let content = resolve_resource_as_string(uri)?;
        if self.content_cache {
            *self.resource_content.lock().unwrap() = Some(content.clone());
        }
        Ok(content)
    }

    /// The expression for the exchange (Java `LanguageProducer.process`).
    fn expression_for(&self, exchange: &Exchange) -> CamelResult<SharedExpression> {
        if self.allow_template_from_header {
            if let Some(s) = exchange.message().header(LANGUAGE_SCRIPT) {
                let s = self.resolve_script(&s.to_string())?;
                return self.create_expression(&s);
            }
        }
        if self.cache_script {
            if let Some(e) = self.expression() {
                return Ok(e);
            }
        }
        let script = match (&self.script, &self.resource_uri) {
            (None, None) => {
                return Err(CamelError::camel_exchange(
                    "No script to evaluate",
                    exchange,
                ));
            }
            (None, Some(r)) => self.resource_content(r)?,
            (Some(s), _) => self.resolve_script(s)?,
        };
        let e = self.create_expression(&script)?;
        if self.cache_script {
            *self.expression.lock().unwrap() = Some(e.clone());
        }
        Ok(e)
    }
}

impl Service for LanguageEndpoint {}

impl Endpoint for LanguageEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(LanguageProducer { endpoint: self }))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::runtime(format!(
            "Cannot consume to a LanguageEndpoint: {}",
            self.uri
        )))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The language producer: evaluates the script, the result becoming the body with
/// `transform`.
struct LanguageProducer {
    endpoint: Arc<LanguageEndpoint>,
}

impl Service for LanguageProducer {}

impl Processor for LanguageProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let r = (|| {
            let e = self.endpoint.expression_for(exchange)?;
            let result = e.evaluate_mut(exchange)?;
            log::debug!("Evaluated expression as: {result}");
            if self.endpoint.transform {
                exchange.message_mut().set_body(result);
            }
            Ok(())
        })();
        Box::pin(std::future::ready(r))
    }
}

impl Producer for LanguageProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
