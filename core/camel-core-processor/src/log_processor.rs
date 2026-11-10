//! The log EIP (Java `org.apache.camel.processor.LogProcessor`): logs a message evaluated from
//! an expression.

use camel_api::enums::LoggingLevel;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedExpression};

/// Converts the Camel logging level to the `log` crate level (`None` for `OFF`).
pub fn to_log_level(level: LoggingLevel) -> Option<log::Level> {
    match level {
        LoggingLevel::TRACE => Some(log::Level::Trace),
        LoggingLevel::DEBUG => Some(log::Level::Debug),
        LoggingLevel::INFO => Some(log::Level::Info),
        LoggingLevel::WARN => Some(log::Level::Warn),
        LoggingLevel::ERROR => Some(log::Level::Error),
        LoggingLevel::OFF => None,
    }
}

/// Logs the value of the expression with the logger name (target) and level.
pub struct LogProcessor {
    expression: SharedExpression,
    log_name: String,
    level: LoggingLevel,
}

impl LogProcessor {
    /// Creates the processor.
    ///
    /// # Arguments
    ///
    /// * `expression` - the message to log
    /// * `log_name` - the logger name (the `log` crate target), usually the route id
    /// * `level` - the logging level
    pub fn new(expression: SharedExpression, log_name: &str, level: LoggingLevel) -> Self {
        LogProcessor {
            expression,
            log_name: log_name.to_string(),
            level,
        }
    }
}

impl Processor for LogProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let r = (|| {
            if let Some(level) = to_log_level(self.level) {
                if log::log_enabled!(target: "camel", level) || log::max_level() >= level {
                    let msg = self.expression.evaluate(exchange)?;
                    log::log!(target: &self.log_name, level, "{msg}");
                }
            }
            Ok(())
        })();
        Box::pin(std::future::ready(r))
    }
}
