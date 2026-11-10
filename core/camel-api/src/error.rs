//! The Camel errors (the Java exception classes of the `org.apache.camel` package).
//!
//! Java Camel has a hierarchy of exception classes (`CamelException`,
//! `RuntimeCamelException`, `CamelExchangeException`, `NoSuchEndpointException`, ...). The Rust
//! port has a single [`CamelError`] type with an [`ErrorKind`] for every Java class. The kinds
//! keep the Java hierarchy, so [`CamelError::is`] answers the Java `instanceof` question (an
//! `InvalidPayload` error *is* a `CamelExchange` error), and the constructors produce the same
//! messages as the Java constructors.

use camel_util::artifact_utils::component_hint;
use camel_util::uri_support::sanitize_uri;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// A shared, thread safe error (the cause of a [`CamelError`]).
pub type BoxError = Arc<dyn Error + Send + Sync + 'static>;

/// Boxes any error as a [`BoxError`] (to use it as the cause of a [`CamelError`]).
pub fn box_error<E: Error + Send + Sync + 'static>(error: E) -> BoxError {
    Arc::new(error)
}

macro_rules! kinds {
    ($($(#[$m:meta])* $kind:ident => $parent:expr),+ $(,)?) => {
        /// The kind of a [`CamelError`]: one per Java exception class.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ErrorKind {
            $($(#[$m])* $kind),+
        }

        impl ErrorKind {
            /// The parent kind (the Java superclass), or `None` for a root.
            pub fn parent(&self) -> Option<ErrorKind> {
                match self {
                    $(ErrorKind::$kind => $parent),+
                }
            }

            /// The Java exception class name (without package), such as `NoSuchEndpointException`.
            pub fn java_name(&self) -> &'static str {
                match self {
                    $(ErrorKind::$kind => concat!(stringify!($kind), "Exception")),+
                }
            }

            /// All the kinds.
            pub const ALL: &'static [ErrorKind] = &[$(ErrorKind::$kind),+];

            /// The kind of the Java exception class name, with or without package (such as
            /// `IllegalArgumentException` or `java.lang.IllegalArgumentException`).
            pub fn from_java_name(name: &str) -> Option<ErrorKind> {
                let simple = name.rsplit('.').next().unwrap_or(name);
                Self::ALL.iter().copied().find(|k| k.java_name() == simple)
            }
        }
    };
}

use ErrorKind as K;

kinds! {
    /// `java.lang.Exception`: any checked error.
    General => None,
    /// `java.lang.RuntimeException`: any unchecked error.
    Runtime => None,
    /// `java.lang.IllegalArgumentException`.
    IllegalArgument => Some(K::Runtime),
    /// `java.lang.IllegalStateException`.
    IllegalState => Some(K::Runtime),
    /// `java.lang.UnsupportedOperationException`.
    UnsupportedOperation => Some(K::Runtime),
    /// `java.io.IOException`.
    Io => Some(K::General),
    /// `java.util.concurrent.RejectedExecutionException`.
    RejectedExecution => Some(K::Runtime),
    /// `CamelException`: base of the checked Camel errors.
    Camel => Some(K::General),
    /// `RuntimeCamelException`: base of the unchecked Camel errors.
    RuntimeCamel => Some(K::Runtime),
    /// `CamelExchangeException`: an error related to an exchange.
    CamelExchange => Some(K::Camel),
    /// `CamelAuthorizationException`: the exchange is not authorized.
    CamelAuthorization => Some(K::CamelExchange),
    /// `CamelExecutionException`: wraps an error thrown while processing an exchange (by a
    /// producer template).
    CamelExecution => Some(K::RuntimeExchange),
    /// `CamelUnitOfWorkException`: the unit of work failed with several errors.
    CamelUnitOfWork => Some(K::CamelExchange),
    /// `AlreadyStoppedException`.
    AlreadyStopped => Some(K::RuntimeCamel),
    /// `ExchangeTimedOutException`: the reply was not received in time.
    ExchangeTimedOut => Some(K::CamelExchange),
    /// `ExpectedBodyTypeException`.
    ExpectedBodyType => Some(K::RuntimeCamel),
    /// `ExpressionEvaluationException`.
    ExpressionEvaluation => Some(K::RuntimeCamel),
    /// `ExpressionIllegalSyntaxException`.
    ExpressionIllegalSyntax => Some(K::RuntimeCamel),
    /// `FailedToCreateConsumerException`.
    FailedToCreateConsumer => Some(K::RuntimeCamel),
    /// `FailedToCreateProducerException`.
    FailedToCreateProducer => Some(K::RuntimeCamel),
    /// `FailedToCreateRouteException`.
    FailedToCreateRoute => Some(K::RuntimeCamel),
    /// `FailedToCreateRouteFromTemplateException`.
    FailedToCreateRouteFromTemplate => Some(K::RuntimeCamel),
    /// `FailedToStartComponentException`.
    FailedToStartComponent => Some(K::RuntimeCamel),
    /// `FailedToStartRouteException`.
    FailedToStartRoute => Some(K::RuntimeCamel),
    /// `InvalidPayloadException`: the body could not be converted to the requested type.
    InvalidPayload => Some(K::CamelExchange),
    /// `InvalidPayloadRuntimeException`.
    InvalidPayloadRuntime => Some(K::RuntimeExchange),
    /// `InvalidPropertyException`.
    InvalidProperty => Some(K::RuntimeCamel),
    /// `LoadPropertiesException`.
    LoadProperties => Some(K::Camel),
    /// `NoFactoryAvailableException`.
    NoFactoryAvailable => Some(K::Io),
    /// `NoSuchBeanException`.
    NoSuchBean => Some(K::RuntimeCamel),
    /// `NoSuchBeanTypeException`.
    NoSuchBeanType => Some(K::NoSuchBean),
    /// `NoSuchEndpointException`.
    NoSuchEndpoint => Some(K::RuntimeCamel),
    /// `NoSuchHeaderException`.
    NoSuchHeader => Some(K::CamelExchange),
    /// `NoSuchHeaderOrPropertyException`.
    NoSuchHeaderOrProperty => Some(K::CamelExchange),
    /// `NoSuchLanguageException`.
    NoSuchLanguage => Some(K::RuntimeCamel),
    /// `NoSuchPropertyException`.
    NoSuchProperty => Some(K::CamelExchange),
    /// `NoSuchServiceException`.
    NoSuchService => Some(K::RuntimeCamel),
    /// `NoSuchVariableException`.
    NoSuchVariable => Some(K::CamelExchange),
    /// `NoTypeConversionAvailableException`.
    NoTypeConversionAvailable => Some(K::Camel),
    /// `PropertyBindingException`.
    PropertyBinding => Some(K::RuntimeCamel),
    /// `ProxyInstantiationException`.
    ProxyInstantiation => Some(K::RuntimeCamel),
    /// `ResolveEndpointFailedException`.
    ResolveEndpointFailed => Some(K::RuntimeCamel),
    /// `RollbackExchangeException`: the exchange is marked for rollback.
    RollbackExchange => Some(K::CamelExchange),
    /// `RouteStoppingException`: the route is stopping and does not accept new exchanges.
    RouteStopping => Some(K::RejectedExecution),
    /// `RuntimeExchangeException`.
    RuntimeExchange => Some(K::RuntimeCamel),
    /// `RuntimeExpressionException`.
    RuntimeExpression => Some(K::RuntimeCamel),
    /// `RuntimeTransformException`.
    RuntimeTransform => Some(K::RuntimeCamel),
    /// `StreamCacheException`.
    StreamCache => Some(K::TypeConversion),
    /// `TypeConversionException`.
    TypeConversion => Some(K::RuntimeCamel),
    /// `TypeConverterExistsException`.
    TypeConverterExists => Some(K::RuntimeCamel),
    /// `TypeConverterLoaderException`.
    TypeConverterLoader => Some(K::RuntimeCamel),
    /// `ValidationException`.
    Validation => Some(K::CamelExchange),
    /// `VetoCamelContextStartException`.
    VetoCamelContextStart => Some(K::General),
}

impl ErrorKind {
    /// Whether this kind is `kind` or one of its subkinds (Java `instanceof`).
    pub fn is(&self, kind: ErrorKind) -> bool {
        let mut current = Some(*self);
        while let Some(k) = current {
            if k == kind {
                return true;
            }
            current = k.parent();
        }
        false
    }
}

/// A Camel error: a kind, a message, an optional cause and optional details.
///
/// # Examples
///
/// ```
/// use camel_api::error::{CamelError, ErrorKind};
/// let e = CamelError::no_such_endpoint("kafak:topic");
/// assert!(e.is(ErrorKind::RuntimeCamel));
/// assert_eq!(
///     "No endpoint could be found for: kafak:topic, please check the needed Camel component is added to the CamelContext (not a built-in Camel component; did you mean 'kafka'?).",
///     e.to_string()
/// );
/// ```
#[derive(Clone)]
pub struct CamelError {
    kind: ErrorKind,
    message: Option<String>,
    source: Option<BoxError>,
    exchange_id: Option<String>,
    detail: Option<String>,
    subtype: Option<&'static str>,
}

impl fmt::Debug for CamelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CamelError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("source", &self.source.as_ref().map(|s| s.to_string()))
            .finish()
    }
}

impl fmt::Display for CamelError {
    /// The message (Java `getMessage()`); without a message the cause is shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.message, &self.source) {
            (Some(m), _) => f.write_str(m),
            (None, Some(s)) => write!(f, "{s}"),
            (None, None) => f.write_str(self.kind.java_name()),
        }
    }
}

impl Error for CamelError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn Error + 'static))
    }
}

impl PartialEq for CamelError {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.message == other.message
    }
}

fn cause_text(cause: &(dyn Error + 'static)) -> String {
    cause.to_string()
}

impl CamelError {
    /// Creates an error of the kind with the message.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        CamelError {
            kind,
            message: Some(message.into()),
            source: None,
            exchange_id: None,
            detail: None,
            subtype: None,
        }
    }

    /// Creates an error of the kind with the message and cause.
    pub fn with_cause(
        kind: ErrorKind,
        message: impl Into<String>,
        cause: impl Into<BoxError>,
    ) -> Self {
        CamelError {
            kind,
            message: Some(message.into()),
            source: Some(cause.into()),
            exchange_id: None,
            detail: None,
            subtype: None,
        }
    }

    /// Wraps the cause as an error of the kind (the Java constructors taking only a cause).
    pub fn wrap(kind: ErrorKind, cause: impl Into<BoxError>) -> Self {
        CamelError {
            kind,
            message: None,
            source: Some(cause.into()),
            exchange_id: None,
            detail: None,
            subtype: None,
        }
    }

    /// Wraps any error as a `RuntimeCamelException`, keeping a `CamelError` as is (Java
    /// `RuntimeCamelException.wrapRuntimeCamelException`).
    pub fn wrap_runtime(error: impl Into<BoxError>) -> Self {
        let error: BoxError = error.into();
        if let Some(e) = error.downcast_ref::<CamelError>() {
            if e.is(ErrorKind::Runtime) {
                return e.clone();
            }
        }
        CamelError::wrap(K::RuntimeCamel, error)
    }

    /// Sets the id of the exchange the error relates to.
    pub fn with_exchange_id(mut self, exchange_id: impl Into<String>) -> Self {
        self.exchange_id = Some(exchange_id.into());
        self
    }

    /// Sets a detail value (such as the endpoint uri, header or property name the error is
    /// about).
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets the Java class name of a component specific exception (such as
    /// `org.apache.camel.component.direct.DirectConsumerNotAvailableException`). Component
    /// exceptions use the [`ErrorKind`] of their Java superclass, plus this subtype.
    pub fn with_subtype(mut self, java_class_name: &'static str) -> Self {
        self.subtype = Some(java_class_name);
        self
    }

    /// The Java class name of a component specific exception, if set.
    pub fn subtype(&self) -> Option<&'static str> {
        self.subtype
    }

    /// Whether the error is the component specific exception (compared by the simple or the
    /// full Java class name).
    pub fn is_subtype(&self, name: &str) -> bool {
        self.subtype
            .is_some_and(|s| s == name || s.rsplit('.').next() == Some(name))
    }

    /// Wraps the error of a failed exchange as a `CamelExecutionException`, unless it already
    /// is one (Java `CamelExecutionException.wrapCamelExecutionException`).
    pub fn wrap_camel_execution(exchange: Option<&dyn ExchangeRef>, error: CamelError) -> Self {
        if error.kind == K::CamelExecution {
            return error;
        }
        let msg = match exchange {
            Some(e) => format!(
                "Exception occurred during execution on the exchange: {}",
                e.display()
            ),
            None => "Exception occurred during execution".to_string(),
        };
        let mut err = CamelError::with_cause(K::CamelExecution, msg, error);
        if let Some(e) = exchange {
            err = err.with_exchange_id(e.exchange_id());
        }
        err
    }

    /// The kind of the error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Whether the error is of the kind or a subkind (Java `instanceof`).
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind.is(kind)
    }

    /// The message (Java `getMessage()`), if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The cause (Java `getCause()`).
    pub fn cause(&self) -> Option<&BoxError> {
        self.source.as_ref()
    }

    /// The id of the exchange the error relates to (Java `CamelExchangeException.getExchange`).
    pub fn exchange_id(&self) -> Option<&str> {
        self.exchange_id.as_deref()
    }

    /// The detail value (the uri, header, property, variable or route id the error is about).
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Finds the first error of the kind in the cause chain, starting from the root cause and
    /// walking upwards (Java `ObjectHelper.getException`).
    pub fn find_cause(&self, kind: ErrorKind) -> Option<&CamelError> {
        camel_util::object_helper::create_exception_iterable(self)
            .into_iter()
            .filter_map(|e| e.downcast_ref::<CamelError>())
            .find(|e| e.is(kind))
    }

    // -------------------------------------------------------------------------------------
    // constructors with the Java messages

    /// A `RuntimeCamelException` with the message.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(K::RuntimeCamel, message)
    }

    /// An `IllegalArgumentException` with the message.
    pub fn illegal_argument(message: impl Into<String>) -> Self {
        Self::new(K::IllegalArgument, message)
    }

    /// An `IllegalStateException` with the message.
    pub fn illegal_state(message: impl Into<String>) -> Self {
        Self::new(K::IllegalState, message)
    }

    /// The Java `CamelExchangeException.createExceptionMessage`: `message. exchange. Caused by:
    /// [cause]`.
    pub fn exchange_message(
        message: Option<&str>,
        exchange: Option<&dyn fmt::Display>,
        cause: Option<&(dyn Error + 'static)>,
    ) -> String {
        let mut sb = String::new();
        if let Some(m) = message {
            sb.push_str(m);
        }
        if let Some(e) = exchange {
            if !sb.is_empty() {
                sb.push_str(". ");
            }
            sb.push_str(&e.to_string());
        }
        if let Some(c) = cause {
            if !sb.is_empty() {
                sb.push_str(". ");
            }
            sb.push_str(&format!(
                "Caused by: [{} - {}]",
                error_type_name(c),
                cause_text(c)
            ));
        }
        sb
    }

    /// A `CamelExchangeException` with the message for the exchange.
    pub fn camel_exchange(message: &str, exchange: &dyn ExchangeRef) -> Self {
        Self::new(
            K::CamelExchange,
            Self::exchange_message(Some(message), Some(&exchange.display()), None),
        )
        .with_exchange_id(exchange.exchange_id())
    }

    /// A `CamelExchangeException` with the message and cause for the exchange.
    pub fn camel_exchange_cause(
        message: &str,
        exchange: &dyn ExchangeRef,
        cause: impl Into<BoxError>,
    ) -> Self {
        let cause: BoxError = cause.into();
        let msg = Self::exchange_message(
            Some(message),
            Some(&exchange.display()),
            Some(cause.as_ref()),
        );
        Self::with_cause(K::CamelExchange, msg, cause).with_exchange_id(exchange.exchange_id())
    }

    /// A `RuntimeExchangeException`: `message on the exchange: Exchange[..]`.
    pub fn runtime_exchange(message: &str, exchange: Option<&dyn ExchangeRef>) -> Self {
        let msg = match exchange {
            Some(e) => format!("{message} on the exchange: {}", e.display()),
            None => message.to_string(),
        };
        let mut err = Self::new(K::RuntimeExchange, msg);
        if let Some(e) = exchange {
            err = err.with_exchange_id(e.exchange_id());
        }
        err
    }

    /// An `AlreadyStoppedException`.
    pub fn already_stopped() -> Self {
        Self::new(K::AlreadyStopped, "Already stopped")
    }

    /// An `ExchangeTimedOutException`.
    pub fn exchange_timed_out(
        exchange: &dyn ExchangeRef,
        timeout_millis: u64,
        message: Option<&str>,
    ) -> Self {
        let msg = match message {
            Some(m) => {
                format!("The OUT message was not received within: {timeout_millis} millis due {m}")
            }
            None => format!("The OUT message was not received within: {timeout_millis} millis"),
        };
        let mut e = Self::camel_exchange(&msg, exchange);
        e.kind = K::ExchangeTimedOut;
        e
    }

    /// An `ExpressionIllegalSyntaxException`.
    pub fn expression_illegal_syntax(expression: &str) -> Self {
        Self::new(
            K::ExpressionIllegalSyntax,
            format!("Illegal syntax: {expression}"),
        )
        .with_detail(expression)
    }

    /// An `ExpressionEvaluationException` wrapping the cause.
    pub fn expression_evaluation(cause: impl Into<BoxError>) -> Self {
        Self::wrap(K::ExpressionEvaluation, cause)
    }

    /// A `FailedToCreateConsumerException` for the endpoint.
    pub fn failed_to_create_consumer(endpoint: &str, cause: impl Into<BoxError>) -> Self {
        let cause: BoxError = cause.into();
        let msg = format!(
            "Failed to create Consumer for endpoint: {}. Reason: {}",
            sanitize_uri(endpoint),
            cause
        );
        Self::with_cause(K::FailedToCreateConsumer, msg, cause).with_detail(sanitize_uri(endpoint))
    }

    /// A `FailedToCreateProducerException` for the endpoint.
    pub fn failed_to_create_producer(endpoint: &str, cause: impl Into<BoxError>) -> Self {
        let cause: BoxError = cause.into();
        let msg = format!(
            "Failed to create Producer for endpoint: {}. Reason: {}",
            sanitize_uri(endpoint),
            cause
        );
        Self::with_cause(K::FailedToCreateProducer, msg, cause).with_detail(sanitize_uri(endpoint))
    }

    /// A `FailedToCreateRouteException` with only a reason.
    pub fn failed_to_create_route_because(cause: &str) -> Self {
        Self::new(
            K::FailedToCreateRoute,
            format!("Failed to create route because: {cause}"),
        )
    }

    /// A `FailedToCreateRouteException` for the route.
    ///
    /// # Arguments
    ///
    /// * `route_id` - the route id
    /// * `location` - the source location of the route, if known
    /// * `route` - the route (text form)
    /// * `cause` - the cause
    pub fn failed_to_create_route(
        route_id: &str,
        location: Option<&str>,
        route: &str,
        cause: impl Into<BoxError>,
    ) -> Self {
        let cause: BoxError = cause.into();
        let loc = location
            .map(|l| format!(" (source: {l})"))
            .unwrap_or_default();
        // cut the route after 60 chars, and sanitize uris so no passwords are shown
        let route = sanitize_uri(&camel_util::string_helper::limit_length_with_suffix(
            route,
            60,
            Some("..."),
        ));
        let msg = format!("Failed to create route: {route_id}{loc}: {route} because: {cause}");
        Self::with_cause(K::FailedToCreateRoute, msg, cause).with_detail(route_id)
    }

    /// A `FailedToStartRouteException`.
    pub fn failed_to_start_route(route_id: &str, message: &str) -> Self {
        Self::new(
            K::FailedToStartRoute,
            format!("Failed to start route: {route_id} because: {message}"),
        )
        .with_detail(route_id)
    }

    /// A `FailedToCreateRouteException` for the failing node of the route: `Failed to create
    /// route: id at: >>> node <<< in route: route... because: cause`.
    pub fn failed_to_create_route_at(
        route_id: &str,
        location: Option<&str>,
        route: &str,
        at: &str,
        cause: impl Into<BoxError>,
    ) -> Self {
        let cause: BoxError = cause.into();
        let loc = location
            .map(|l| format!(" (source: {l})"))
            .unwrap_or_default();
        let route = sanitize_uri(&camel_util::string_helper::limit_length_with_suffix(
            route,
            60,
            Some("..."),
        ));
        let msg = format!(
            "Failed to create route: {route_id}{loc} at: >>> {at} <<< in route: {route} because: {cause}"
        );
        Self::with_cause(K::FailedToCreateRoute, msg, cause).with_detail(route_id)
    }

    /// A `FailedToStartRouteException` with the cause.
    pub fn failed_to_start_route_cause(
        route_id: &str,
        message: &str,
        cause: impl Into<BoxError>,
    ) -> Self {
        Self::with_cause(
            K::FailedToStartRoute,
            format!("Failed to start route: {route_id} because: {message}"),
            cause,
        )
        .with_detail(route_id)
    }

    /// A `FailedToStartComponentException`.
    pub fn failed_to_start_component(
        component: &str,
        message: &str,
        cause: impl Into<BoxError>,
    ) -> Self {
        Self::with_cause(
            K::FailedToStartComponent,
            format!("Failed to start component {component} because of {message}"),
            cause,
        )
        .with_detail(component)
    }

    /// An `InvalidPayloadException`: the body is not available as the type.
    pub fn invalid_payload(
        exchange: &dyn ExchangeRef,
        type_name: &str,
        body_description: &str,
        message: &str,
    ) -> Self {
        let mut e = Self::camel_exchange(
            &format!("No body available of type: {type_name}{body_description} on: {message}"),
            exchange,
        );
        e.kind = K::InvalidPayload;
        e.detail = Some(type_name.to_string());
        e
    }

    /// A `NoSuchBeanException` for the name.
    pub fn no_such_bean(name: &str) -> Self {
        Self::new(
            K::NoSuchBean,
            format!("No bean could be found in the registry for: {name}"),
        )
        .with_detail(name)
    }

    /// A `NoSuchBeanException` for the name and type.
    pub fn no_such_bean_of_type(name: Option<&str>, type_name: &str) -> Self {
        let n = name.map(|n| format!(" for: {n}")).unwrap_or_default();
        let mut e = Self::new(
            K::NoSuchBean,
            format!("No bean could be found in the registry{n} of type: {type_name}"),
        );
        if let Some(n) = name {
            e = e.with_detail(n);
        }
        e
    }

    /// A `NoSuchEndpointException` for the uri (with a "did you mean" hint for the scheme).
    pub fn no_such_endpoint(uri: &str) -> Self {
        let sanitized = sanitize_uri(uri);
        let msg = format!(
            "No endpoint could be found for: {sanitized}, please check the needed Camel component is added to the CamelContext{}.",
            component_hint(&endpoint_scheme(uri))
        );
        Self::new(K::NoSuchEndpoint, msg).with_detail(sanitized)
    }

    /// A `NoSuchEndpointException` with a resolution instruction (appended after "please").
    pub fn no_such_endpoint_resolve(uri: &str, resolve_method: &str) -> Self {
        let sanitized = sanitize_uri(uri);
        Self::new(
            K::NoSuchEndpoint,
            format!("No endpoint could be found for: {sanitized}, please {resolve_method}"),
        )
        .with_detail(sanitized)
    }

    /// A `NoSuchLanguageException`.
    pub fn no_such_language(language: &str) -> Self {
        let msg = format!(
            "No language could be found for: {language}{}",
            camel_util::artifact_utils::language_hint(language)
        );
        Self::new(K::NoSuchLanguage, msg).with_detail(language)
    }

    /// A `NoTypeConversionAvailableException`.
    ///
    /// # Arguments
    ///
    /// * `value_type` - the type name of the value (`None` when the value is null)
    /// * `to_type` - the required type name
    pub fn no_type_conversion_available(value_type: Option<&str>, to_type: &str) -> Self {
        let msg = format!(
            "No type converter available to convert from type: {} to the required type: {to_type}{}",
            value_type.unwrap_or("null"),
            if value_type.is_none() { " (the value is null)" } else { "" }
        );
        Self::new(K::NoTypeConversionAvailable, msg)
    }

    /// A `TypeConversionException`.
    pub fn type_conversion(
        value_type: Option<&str>,
        to_type: &str,
        cause: impl Into<BoxError>,
    ) -> Self {
        let cause: BoxError = cause.into();
        let msg = format!(
            "Error during type conversion from type: {} to the required type: {to_type} due to {}: {}",
            value_type.unwrap_or("null"),
            error_type_name(cause.as_ref()),
            cause
        );
        Self::with_cause(K::TypeConversion, msg, cause)
    }

    /// A `ResolveEndpointFailedException` with the cause.
    pub fn resolve_endpoint_failed(uri: &str, cause: impl Into<BoxError>) -> Self {
        let cause: BoxError = cause.into();
        let sanitized = sanitize_uri(uri);
        let msg = format!("Failed to resolve endpoint: {sanitized} due to: {cause}");
        Self::with_cause(K::ResolveEndpointFailed, msg, cause).with_detail(sanitized)
    }

    /// A `ResolveEndpointFailedException` with a message.
    pub fn resolve_endpoint_failed_message(uri: &str, message: &str) -> Self {
        let sanitized = sanitize_uri(uri);
        Self::new(
            K::ResolveEndpointFailed,
            format!("Failed to resolve endpoint: {sanitized} due to: {message}"),
        )
        .with_detail(sanitized)
    }

    /// A `RollbackExchangeException` for the exchange.
    pub fn rollback_exchange(message: Option<&str>, exchange: &dyn ExchangeRef) -> Self {
        let mut e = Self::camel_exchange(message.unwrap_or("Intended rollback"), exchange);
        e.kind = K::RollbackExchange;
        e
    }

    /// A `RouteStoppingException`.
    pub fn route_stopping(message: &str) -> Self {
        Self::new(K::RouteStopping, message)
    }

    /// A `NoSuchHeaderException` for the header.
    ///
    /// # Arguments
    ///
    /// * `exchange` - the exchange
    /// * `header_name` - the header
    /// * `type_name` - the type the header was requested as, if any
    /// * `existing` - the existing value and its type name, when the header exists but could not
    ///   be converted
    pub fn no_such_header(
        exchange: &dyn ExchangeRef,
        header_name: &str,
        type_name: Option<&str>,
        existing: Option<(&str, &str)>,
    ) -> Self {
        let t = type_name
            .map(|t| format!(" of type: {t}"))
            .unwrap_or_default();
        let v = existing
            .map(|(v, t)| format!(" but has value: {v} of type: {t}"))
            .unwrap_or_default();
        let mut e = Self::camel_exchange(
            &format!("No '{header_name}' header available{t}{v}"),
            exchange,
        );
        e.kind = K::NoSuchHeader;
        e.detail = Some(header_name.to_string());
        e
    }

    /// A `NoSuchPropertyException` for the exchange property.
    pub fn no_such_property(
        exchange: &dyn ExchangeRef,
        property_name: &str,
        type_name: Option<&str>,
    ) -> Self {
        let t = type_name
            .map(|t| format!(" of type: {t}"))
            .unwrap_or_default();
        let mut e = Self::camel_exchange(
            &format!("No '{property_name}' exchange property available{t}"),
            exchange,
        );
        e.kind = K::NoSuchProperty;
        e.detail = Some(property_name.to_string());
        e
    }

    /// A `NoSuchVariableException` for the variable.
    pub fn no_such_variable(
        exchange: &dyn ExchangeRef,
        variable_name: &str,
        type_name: Option<&str>,
    ) -> Self {
        let msg = match type_name {
            Some(t) => format!("No '{variable_name}' variable available of type: {t}"),
            None => format!("No '{variable_name}' variable available"),
        };
        let mut e = Self::camel_exchange(&msg, exchange);
        e.kind = K::NoSuchVariable;
        e.detail = Some(variable_name.to_string());
        e
    }

    /// A `ValidationException` for the exchange.
    pub fn validation(exchange: &dyn ExchangeRef, message: &str) -> Self {
        let mut e = Self::camel_exchange(message, exchange);
        e.kind = K::Validation;
        e
    }

    /// A `PropertyBindingException`: error binding the property with the name.
    pub fn property_binding(
        target: &str,
        property_name: &str,
        value: &str,
        reason: Option<&str>,
    ) -> Self {
        let r = reason.map(|r| format!(": {r}")).unwrap_or_default();
        Self::new(
            K::PropertyBinding,
            format!("Error binding property ({property_name}={value}) with name: {property_name} on bean: {target} with value: {value}{r}"),
        )
        .with_detail(property_name)
    }
}

/// The scheme of an endpoint uri: the text before the first colon or question mark, or the
/// whole uri when it has neither.
fn endpoint_scheme(uri: &str) -> String {
    match uri.find([':', '?']) {
        Some(p) => uri[..p].to_string(),
        None => uri.to_string(),
    }
}

/// The type name of an error for messages (the Java class name for Camel errors).
pub fn error_type_name(e: &(dyn Error + 'static)) -> String {
    match e.downcast_ref::<CamelError>() {
        Some(c) => match c.subtype() {
            Some(s) => s.to_string(),
            None => format!("org.apache.camel.{}", c.kind().java_name()),
        },
        None => std::any::type_name_of_val(e).to_string(),
    }
}

/// What an error needs to know about an exchange (implemented by
/// [`crate::exchange::Exchange`]).
pub trait ExchangeRef {
    /// The exchange id.
    fn exchange_id(&self) -> String;
    /// The text form of the exchange, such as `Exchange[ID-...]`.
    fn display(&self) -> String;
}

impl From<CamelError> for BoxError {
    fn from(e: CamelError) -> Self {
        Arc::new(e)
    }
}

impl From<std::io::Error> for CamelError {
    fn from(e: std::io::Error) -> Self {
        CamelError::wrap(K::Io, Arc::new(e) as BoxError)
    }
}

impl From<String> for CamelError {
    /// A `RuntimeCamelException` with the message.
    fn from(message: String) -> Self {
        CamelError::runtime(message)
    }
}

impl From<&str> for CamelError {
    /// A `RuntimeCamelException` with the message.
    fn from(message: &str) -> Self {
        CamelError::runtime(message)
    }
}

/// The result type of fallible Camel operations.
pub type CamelResult<T> = Result<T, CamelError>;

#[cfg(test)]
mod tests {
    use super::*;

    struct Ex;
    impl ExchangeRef for Ex {
        fn exchange_id(&self) -> String {
            "ID-1".into()
        }
        fn display(&self) -> String {
            "Exchange[ID-1]".into()
        }
    }

    #[test]
    fn hierarchy() {
        assert!(ErrorKind::InvalidPayload.is(ErrorKind::CamelExchange));
        assert!(ErrorKind::InvalidPayload.is(ErrorKind::Camel));
        assert!(ErrorKind::InvalidPayload.is(ErrorKind::General));
        assert!(!ErrorKind::InvalidPayload.is(ErrorKind::Runtime));
        assert!(ErrorKind::NoSuchBeanType.is(ErrorKind::NoSuchBean));
        assert!(ErrorKind::RouteStopping.is(ErrorKind::RejectedExecution));
        assert!(ErrorKind::CamelExecution.is(ErrorKind::RuntimeExchange));
        let sub = CamelError::new(ErrorKind::CamelExchange, "x")
            .with_subtype("org.apache.camel.component.direct.DirectConsumerNotAvailableException");
        assert!(sub.is_subtype("DirectConsumerNotAvailableException"));
        assert!(!sub.is_subtype("Other"));
        assert_eq!(
            "org.apache.camel.component.direct.DirectConsumerNotAvailableException",
            error_type_name(&sub)
        );
        let wrapped = CamelError::wrap_camel_execution(None, sub);
        assert_eq!(
            Some("Exception occurred during execution"),
            wrapped.message()
        );
        assert!(wrapped.is(ErrorKind::CamelExecution));
        assert_eq!(
            Some("Exception occurred during execution"),
            CamelError::wrap_camel_execution(None, wrapped).message()
        );
        assert_eq!(
            "NoSuchEndpointException",
            ErrorKind::NoSuchEndpoint.java_name()
        );
    }

    #[test]
    fn messages() {
        assert_eq!("Already stopped", CamelError::already_stopped().to_string());
        assert_eq!(
            "The OUT message was not received within: 5000 millis. Exchange[ID-1]",
            CamelError::exchange_timed_out(&Ex, 5000, None).to_string()
        );
        let e = CamelError::exchange_timed_out(&Ex, 5000, Some("timeout"));
        assert_eq!(
            "The OUT message was not received within: 5000 millis due timeout. Exchange[ID-1]",
            e.to_string()
        );
        assert_eq!(Some("ID-1"), e.exchange_id());
        assert_eq!(
            "Illegal syntax: ${foo",
            CamelError::expression_illegal_syntax("${foo").to_string()
        );
        assert_eq!(
            "No endpoint could be found for: ftp://joe:xxxxxx@host, please check the needed Camel component is added to the CamelContext (the ftp component is in camel-ftp; add camel-ftp to the dependencies).",
            CamelError::no_such_endpoint("ftp://joe:secret@host").to_string()
        );
        assert_eq!(
            "No endpoint could be found for: foo:bar, please use the direct component",
            CamelError::no_such_endpoint_resolve("foo:bar", "use the direct component").to_string()
        );
        assert_eq!(
            "No type converter available to convert from type: null to the required type: Long (the value is null)",
            CamelError::no_type_conversion_available(None, "Long").to_string()
        );
        assert_eq!(
            "No bean could be found in the registry for: foo",
            CamelError::no_such_bean("foo").to_string()
        );
        assert_eq!(
            "No bean could be found in the registry of type: Processor",
            CamelError::no_such_bean_of_type(None, "Processor").to_string()
        );
        assert_eq!(
            "Intended rollback. Exchange[ID-1]",
            CamelError::rollback_exchange(None, &Ex).to_string()
        );
        assert_eq!(
            "No 'foo' header available of type: Long. Exchange[ID-1]",
            CamelError::no_such_header(&Ex, "foo", Some("Long"), None).to_string()
        );
        assert_eq!(
            "No 'foo' header available of type: Long but has value: abc of type: String. Exchange[ID-1]",
            CamelError::no_such_header(&Ex, "foo", Some("Long"), Some(("abc", "String"))).to_string()
        );
        assert_eq!(
            "No 'v' variable available. Exchange[ID-1]",
            CamelError::no_such_variable(&Ex, "v", None).to_string()
        );
        assert_eq!(
            "Failed to create route: r1 (source: a.yaml): From[ftp://joe:xxxxxx@h] because: boom",
            CamelError::failed_to_create_route(
                "r1",
                Some("a.yaml"),
                "From[ftp://joe:pw@h]",
                CamelError::runtime("boom")
            )
            .to_string()
        );
        assert_eq!(
            "Error during type conversion from type: String to the required type: Long due to org.apache.camel.RuntimeCamelException: bad",
            CamelError::type_conversion(Some("String"), "Long", CamelError::runtime("bad")).to_string()
        );
        assert_eq!(
            "x on the exchange: Exchange[ID-1]",
            CamelError::runtime_exchange("x", Some(&Ex)).to_string()
        );
        assert_eq!(
            "Failed to start route: r1 because: boom",
            CamelError::failed_to_start_route("r1", "boom").to_string()
        );
    }

    #[test]
    fn causes() {
        let root = CamelError::no_such_bean("foo");
        let mid = CamelError::camel_exchange_cause("failed", &Ex, root.clone());
        assert_eq!(
            "failed. Exchange[ID-1]. Caused by: [org.apache.camel.NoSuchBeanException - No bean could be found in the registry for: foo]",
            mid.to_string()
        );
        let top = CamelError::wrap_runtime(mid.clone());
        assert_eq!(ErrorKind::RuntimeCamel, top.kind());
        assert_eq!(Some(&root), top.find_cause(ErrorKind::NoSuchBean));
        assert!(top.find_cause(ErrorKind::Validation).is_none());
        // a runtime error is not wrapped again
        assert_eq!(root, CamelError::wrap_runtime(root.clone()));
        let io: CamelError = std::io::Error::other("disk").into();
        assert!(io.is(ErrorKind::Io));
        assert_eq!("disk", io.to_string());
        let s: CamelError = "plain".into();
        assert_eq!(ErrorKind::RuntimeCamel, s.kind());
    }
}
