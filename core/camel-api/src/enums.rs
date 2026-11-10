//! The enumerations of the Camel API (`org.apache.camel.ExchangePattern`, `LoggingLevel`,
//! `ServiceStatus`, ...).
//!
//! Every enum parses from and displays as the Java constant name (Java `valueOf` /
//! `name()`), as these names are used in configuration and route definitions.

use std::fmt;
use std::str::FromStr;

/// Error for a text that is not the name of an enum constant (Java `IllegalArgumentException`
/// from `valueOf`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEnumValue {
    /// The enum type name.
    pub enum_name: &'static str,
    /// The text that was not recognized.
    pub value: String,
}

impl fmt::Display for UnknownEnumValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.enum_name == "ExchangePattern" {
            write!(f, "Unknown message exchange pattern: {}", self.value)
        } else {
            write!(f, "No enum constant {}.{}", self.enum_name, self.value)
        }
    }
}

impl std::error::Error for UnknownEnumValue {}

macro_rules! java_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// All the constants, in declaration order (Java `values()`).
            pub const VALUES: &'static [$name] = &[$($name::$variant),+];

            /// The constant name (Java `name()`).
            pub fn name(&self) -> &'static str {
                match self {
                    $($name::$variant => stringify!($variant)),+
                }
            }

            /// The position in declaration order (Java `ordinal()`).
            pub fn ordinal(&self) -> usize {
                *self as usize
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.name())
            }
        }

        impl FromStr for $name {
            type Err = UnknownEnumValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $(stringify!($variant) => Ok($name::$variant),)+
                    _ => Err(UnknownEnumValue { enum_name: stringify!($name), value: s.to_string() }),
                }
            }
        }
    };
}

java_enum! {
    /// The message exchange pattern of an exchange, indicating whether the caller expects a
    /// reply.
    ///
    /// `InOnly` is used for fire-and-forget messaging: the caller sends a message and does not
    /// wait for a response. `InOut` is used for request-reply messaging: the caller sends a
    /// message and blocks until a response is received.
    ///
    /// The pattern is set on the exchange and is checked by the Camel routing engine and
    /// endpoints to determine how to handle the interaction.
    ExchangePattern {
        /// Fire-and-forget: the caller sends a message and does not expect a reply.
        InOnly,
        /// Request-reply: the caller sends a message and expects a response.
        InOut,
    }
}

// the enum is declared by a macro, so `#[derive(Default)]` cannot be used
#[allow(clippy::derivable_impls)]
impl Default for ExchangePattern {
    fn default() -> Self {
        ExchangePattern::InOnly
    }
}

impl ExchangePattern {
    /// Returns true if there can be an IN message.
    pub fn is_in_capable(&self) -> bool {
        true
    }

    /// Returns true if there can be an OUT message.
    pub fn is_out_capable(&self) -> bool {
        *self != ExchangePattern::InOnly
    }

    /// Parses the pattern (Java `asEnum`).
    ///
    /// # Errors
    ///
    /// Returns `Unknown message exchange pattern: <value>` for an unknown value.
    pub fn as_enum(value: &str) -> Result<ExchangePattern, UnknownEnumValue> {
        value.parse()
    }
}

java_enum! {
    /// Logging severity levels used throughout Camel to control the verbosity of diagnostic
    /// output.
    ///
    /// Used wherever Camel accepts a configurable log level: the Log EIP, error-handler
    /// redelivery logging, tracing, the dead-letter channel, and many component-level options.
    /// [`LoggingLevel::OFF`] disables the specific log statement without changing the logger's
    /// overall threshold.
    #[allow(clippy::upper_case_acronyms)]
    LoggingLevel {
        /// Most detailed logging level.
        TRACE,
        /// Detailed information useful during development.
        DEBUG,
        /// Informational messages.
        INFO,
        /// Warning messages.
        WARN,
        /// Error messages.
        ERROR,
        /// Disables logging entirely.
        OFF,
    }
}

impl LoggingLevel {
    /// Is the given logging level equal or higher than the current level (`OFF` is never
    /// enabled).
    pub fn is_enabled(&self, level: LoggingLevel) -> bool {
        if *self == LoggingLevel::OFF || level == LoggingLevel::OFF {
            return false;
        }
        *self <= level
    }
}

java_enum! {
    /// The current lifecycle state of a service as it transitions between initializing,
    /// started, suspended and stopped.
    ServiceStatus {
        /// The service is being initialized.
        Initializing,
        /// The service has been initialized but not yet started.
        Initialized,
        /// The service is starting.
        Starting,
        /// The service is started and ready.
        Started,
        /// The service is stopping.
        Stopping,
        /// The service has been stopped.
        Stopped,
        /// The service is suspending.
        Suspending,
        /// The service has been suspended.
        Suspended,
    }
}

impl ServiceStatus {
    /// Whether the service can be started (initialized, stopped or suspended).
    pub fn is_startable(&self) -> bool {
        matches!(self, Self::Initialized | Self::Stopped | Self::Suspended)
    }

    /// Whether the service can be stopped (started or suspended).
    pub fn is_stoppable(&self) -> bool {
        matches!(self, Self::Started | Self::Suspended)
    }

    /// Whether the service can be suspended (started).
    pub fn is_suspendable(&self) -> bool {
        *self == Self::Started
    }

    /// Whether the service is being initialized.
    pub fn is_initializing(&self) -> bool {
        *self == Self::Initializing
    }

    /// Whether the service is initialized.
    pub fn is_initialized(&self) -> bool {
        *self == Self::Initialized
    }

    /// Whether the service is starting.
    pub fn is_starting(&self) -> bool {
        *self == Self::Starting
    }

    /// Whether the service is started.
    pub fn is_started(&self) -> bool {
        *self == Self::Started
    }

    /// Whether the service is stopping.
    pub fn is_stopping(&self) -> bool {
        *self == Self::Stopping
    }

    /// Whether the service is stopped.
    pub fn is_stopped(&self) -> bool {
        *self == Self::Stopped
    }

    /// Whether the service is suspending.
    pub fn is_suspending(&self) -> bool {
        *self == Self::Suspending
    }

    /// Whether the service is suspended.
    pub fn is_suspended(&self) -> bool {
        *self == Self::Suspended
    }
}

java_enum! {
    /// Per-route policy controlling whether the route stops immediately or defers during
    /// graceful shutdown.
    ///
    /// Deferring is useful for shared internal routes (for example a `direct:` or `seda:` route
    /// used by many other routes) that must remain active until all dependent routes have
    /// stopped processing their in-flight exchanges.
    ShutdownRoute {
        /// Attempt to stop the route as soon as the shutdown sequence reaches it.
        Default,
        /// Keep the route running and stop it only after all `Default` routes have stopped.
        Defer,
    }
}

java_enum! {
    /// Per-consumer policy controlling how many pending tasks are processed before the consumer
    /// stops during graceful shutdown.
    ///
    /// Most consumers handle a single task at a time, so the distinction only matters for batch
    /// consumers (such as the file consumer) that can have multiple pending messages in a batch
    /// when shutdown is triggered.
    ShutdownRunningTask {
        /// The default: finish the current in-flight task and then stop.
        CompleteCurrentTaskOnly,
        /// Drain the entire current batch before stopping (batch consumers only).
        CompleteAllTasks,
    }
}

java_enum! {
    /// The kinds of options for waiting for (async) tasks to complete.
    WaitForTaskToComplete {
        /// Never wait for the async task to complete.
        Never,
        /// Wait only if the exchange pattern expects a reply (InOut).
        IfReplyExpected,
        /// Always wait for the async task to complete.
        Always,
    }
}

java_enum! {
    /// Defines the scope for bean lookups in the registry.
    ///
    /// *Since Camel 3.1*
    BeanScope {
        /// Reuses a single bean instance for all invocations (default).
        Singleton,
        /// Creates a new bean instance per exchange.
        Request,
        /// Creates a new bean instance per invocation.
        Prototype,
    }
}

java_enum! {
    /// Controls which Camel objects are registered for management and monitoring.
    ///
    /// *Since Camel 3.17*
    ManagementMBeansLevel {
        /// Only register the CamelContext.
        ContextOnly,
        /// Register the CamelContext and routes.
        RoutesOnly,
        /// Register the CamelContext, routes, and processors.
        Default,
    }
}

impl ManagementMBeansLevel {
    /// Whether routes are registered.
    pub fn is_routes(&self) -> bool {
        matches!(self, Self::Default | Self::RoutesOnly)
    }

    /// Whether processors are registered.
    pub fn is_processors(&self) -> bool {
        *self == Self::Default
    }
}

java_enum! {
    /// Controls the granularity of the performance statistics collected for a running
    /// CamelContext.
    ManagementStatisticsLevel {
        /// Extended statistics including additional performance metrics.
        Extended,
        /// Default statistics for context, routes, and processors.
        Default,
        /// Statistics for the context and routes only (no processors).
        RoutesOnly,
        /// No statistics collected.
        Off,
    }
}

impl ManagementStatisticsLevel {
    /// Whether the level is `Default` or `Extended`.
    pub fn is_default_or_extended(&self) -> bool {
        matches!(self, Self::Default | Self::Extended)
    }

    /// Whether the level is `Extended`.
    pub fn is_extended(&self) -> bool {
        *self == Self::Extended
    }
}

java_enum! {
    /// Controls how much information the CamelContext logs in its startup (and shutdown)
    /// summary.
    ///
    /// *Since Camel 3.8*
    StartupSummaryLevel {
        /// Most detailed startup summary with all routes and their endpoints.
        Verbose,
        /// Default startup summary with route overview.
        Default,
        /// Brief startup summary with route count.
        Brief,
        /// Single-line startup summary.
        Oneline,
        /// No startup summary logged.
        Off,
    }
}

java_enum! {
    /// Policy controlling what happens when the type converter registry receives a request to
    /// register a type converter whose from-to type pair is already registered. The default is
    /// `Ignore`.
    TypeConverterExists {
        /// Replace the existing type converter.
        Override,
        /// Keep the existing type converter and discard the new one.
        Ignore,
        /// Fail to signal a duplicate type converter.
        Fail,
    }
}

java_enum! {
    /// Lifecycle events of a CamelContext that can be timestamped by its clock.
    ///
    /// *Since Camel 4.4*
    #[allow(clippy::upper_case_acronyms)]
    ContextEvents {
        /// Boot event.
        BOOT,
        /// Start event.
        START,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_pattern() {
        assert_eq!(
            Ok(ExchangePattern::InOut),
            ExchangePattern::as_enum("InOut")
        );
        let e = ExchangePattern::as_enum("Foo").unwrap_err();
        assert_eq!("Unknown message exchange pattern: Foo", e.to_string());
        assert!(!ExchangePattern::InOnly.is_out_capable());
        assert!(ExchangePattern::InOut.is_out_capable());
        assert!(ExchangePattern::InOnly.is_in_capable());
        assert_eq!(ExchangePattern::InOnly, ExchangePattern::default());
    }

    #[test]
    fn logging_level() {
        assert!(LoggingLevel::INFO.is_enabled(LoggingLevel::WARN));
        assert!(LoggingLevel::INFO.is_enabled(LoggingLevel::INFO));
        assert!(!LoggingLevel::INFO.is_enabled(LoggingLevel::DEBUG));
        assert!(!LoggingLevel::OFF.is_enabled(LoggingLevel::ERROR));
        assert!(!LoggingLevel::TRACE.is_enabled(LoggingLevel::OFF));
        assert_eq!("WARN", LoggingLevel::WARN.to_string());
        assert_eq!(
            "No enum constant LoggingLevel.warn",
            "warn".parse::<LoggingLevel>().unwrap_err().to_string()
        );
    }

    #[test]
    fn service_status() {
        assert!(ServiceStatus::Stopped.is_startable());
        assert!(!ServiceStatus::Started.is_startable());
        assert!(ServiceStatus::Suspended.is_stoppable());
        assert!(ServiceStatus::Started.is_suspendable());
        assert!(ServiceStatus::Starting.is_starting());
        assert_eq!(8, ServiceStatus::VALUES.len());
        assert_eq!(3, ServiceStatus::Started.ordinal());
        for s in ServiceStatus::VALUES {
            assert_eq!(*s, s.name().parse().unwrap());
        }
    }

    #[test]
    fn management_levels() {
        assert!(ManagementMBeansLevel::RoutesOnly.is_routes());
        assert!(!ManagementMBeansLevel::RoutesOnly.is_processors());
        assert!(ManagementStatisticsLevel::Extended.is_default_or_extended());
        assert!(!ManagementStatisticsLevel::Off.is_default_or_extended());
        assert!(ManagementStatisticsLevel::Extended.is_extended());
        assert_eq!(Ok(StartupSummaryLevel::Oneline), "Oneline".parse());
        assert_eq!(Ok(TypeConverterExists::Fail), "Fail".parse());
        assert_eq!(Ok(BeanScope::Prototype), "Prototype".parse());
        assert_eq!(Ok(ShutdownRoute::Defer), "Defer".parse());
        assert_eq!(
            Ok(ShutdownRunningTask::CompleteAllTasks),
            "CompleteAllTasks".parse()
        );
        assert_eq!(
            Ok(WaitForTaskToComplete::IfReplyExpected),
            "IfReplyExpected".parse()
        );
        assert_eq!("BOOT", ContextEvents::BOOT.name());
    }
}
