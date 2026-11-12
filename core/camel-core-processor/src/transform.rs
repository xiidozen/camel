//! Message translators: set body, headers, properties and variables, remove headers, stop and
//! throw exception (Java `SetBodyProcessor`, `TransformProcessor`, `SetHeaderProcessor`,
//! `SetPropertyProcessor`, `SetVariableProcessor`, `RemoveHeaderProcessor`,
//! `RemoveHeadersProcessor`, `StopProcessor`, `ThrowExceptionProcessor`, `SetHeadersProcessor`,
//! `SetVariablesProcessor`, `RemovePropertyProcessor`, `RemovePropertiesProcessor`,
//! `RemoveVariableProcessor`, `ScriptProcessor`).

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedExpression};

fn ready(r: CamelResult<()>) -> BoxFuture<'static, CamelResult<()>> {
    Box::pin(std::future::ready(r))
}

/// Sets the body of the message to the value of the expression (the setBody and transform
/// EIPs).
pub struct SetBodyProcessor {
    expression: SharedExpression,
}

impl SetBodyProcessor {
    /// Creates the processor.
    pub fn new(expression: SharedExpression) -> Self {
        SetBodyProcessor { expression }
    }
}

impl Processor for SetBodyProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        ready(
            self.expression
                .evaluate_mut(exchange)
                .map(|v| exchange.message_mut().set_body(v)),
        )
    }
}

/// Sets a header to the value of the expression.
pub struct SetHeaderProcessor {
    name: String,
    expression: SharedExpression,
}

impl SetHeaderProcessor {
    /// Creates the processor.
    pub fn new(name: &str, expression: SharedExpression) -> Self {
        SetHeaderProcessor {
            name: name.to_string(),
            expression,
        }
    }
}

impl Processor for SetHeaderProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        ready(
            self.expression
                .evaluate_mut(exchange)
                .map(|v| exchange.message_mut().set_header(&self.name, v)),
        )
    }
}

/// Sets an exchange property to the value of the expression.
pub struct SetPropertyProcessor {
    name: String,
    expression: SharedExpression,
}

impl SetPropertyProcessor {
    /// Creates the processor.
    pub fn new(name: &str, expression: SharedExpression) -> Self {
        SetPropertyProcessor {
            name: name.to_string(),
            expression,
        }
    }
}

impl Processor for SetPropertyProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        ready(
            self.expression
                .evaluate_mut(exchange)
                .map(|v| exchange.set_property(&self.name, v)),
        )
    }
}

/// Sets a variable to the value of the expression.
pub struct SetVariableProcessor {
    name: String,
    expression: SharedExpression,
}

impl SetVariableProcessor {
    /// Creates the processor.
    pub fn new(name: &str, expression: SharedExpression) -> Self {
        SetVariableProcessor {
            name: name.to_string(),
            expression,
        }
    }
}

impl Processor for SetVariableProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        ready(
            self.expression
                .evaluate_mut(exchange)
                .map(|v| exchange.set_variable(&self.name, v)),
        )
    }
}

/// Removes a header.
pub struct RemoveHeaderProcessor {
    name: String,
}

impl RemoveHeaderProcessor {
    /// Creates the processor.
    pub fn new(name: &str) -> Self {
        RemoveHeaderProcessor {
            name: name.to_string(),
        }
    }
}

impl Processor for RemoveHeaderProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        exchange.message_mut().remove_header(&self.name);
        ready(Ok(()))
    }
}

/// Removes the headers matching the pattern, except those matching an exclude pattern.
pub struct RemoveHeadersProcessor {
    pattern: String,
    exclude_patterns: Vec<String>,
}

impl RemoveHeadersProcessor {
    /// Creates the processor.
    pub fn new(pattern: &str, exclude_patterns: &[&str]) -> Self {
        RemoveHeadersProcessor {
            pattern: pattern.to_string(),
            exclude_patterns: exclude_patterns.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Processor for RemoveHeadersProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let excludes: Vec<&str> = self.exclude_patterns.iter().map(String::as_str).collect();
        exchange
            .message_mut()
            .remove_headers_excluding(&self.pattern, &excludes);
        ready(Ok(()))
    }
}

/// Stops routing the exchange (the stop EIP).
pub struct StopProcessor;

impl Processor for StopProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        exchange.set_route_stop(true);
        ready(Ok(()))
    }
}

/// Fails the exchange with the given error (the throwException EIP).
pub struct ThrowExceptionProcessor {
    error: CamelError,
}

impl ThrowExceptionProcessor {
    /// Creates the processor.
    pub fn new(error: CamelError) -> Self {
        ThrowExceptionProcessor { error }
    }
}

impl Processor for ThrowExceptionProcessor {
    fn process<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        ready(Err(self.error.clone()))
    }
}

/// Sets several headers, in order, each to the value of its expression (the setHeaders EIP);
/// an expression sees the headers set before it.
pub struct SetHeadersProcessor {
    headers: Vec<(String, SharedExpression)>,
}

impl SetHeadersProcessor {
    /// Creates the processor.
    pub fn new(headers: Vec<(String, SharedExpression)>) -> Self {
        SetHeadersProcessor { headers }
    }
}

impl Processor for SetHeadersProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        for (name, expression) in &self.headers {
            match expression.evaluate_mut(exchange) {
                Ok(v) => exchange.message_mut().set_header(name, v),
                Err(e) => return ready(Err(e)),
            }
        }
        ready(Ok(()))
    }
}

/// Sets several variables, in order (the setVariables EIP).
pub struct SetVariablesProcessor {
    variables: Vec<(String, SharedExpression)>,
}

impl SetVariablesProcessor {
    /// Creates the processor.
    pub fn new(variables: Vec<(String, SharedExpression)>) -> Self {
        SetVariablesProcessor { variables }
    }
}

impl Processor for SetVariablesProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        for (name, expression) in &self.variables {
            match expression.evaluate_mut(exchange) {
                Ok(v) => exchange.set_variable(name, v),
                Err(e) => return ready(Err(e)),
            }
        }
        ready(Ok(()))
    }
}

/// Removes an exchange property (the removeProperty EIP).
pub struct RemovePropertyProcessor {
    name: String,
}

impl RemovePropertyProcessor {
    /// Creates the processor.
    pub fn new(name: &str) -> Self {
        RemovePropertyProcessor {
            name: name.to_string(),
        }
    }
}

impl Processor for RemovePropertyProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        exchange.remove_property(&self.name);
        ready(Ok(()))
    }
}

/// Removes the exchange properties matching a pattern, except those matching an exclude
/// pattern (the removeProperties EIP).
pub struct RemovePropertiesProcessor {
    pattern: String,
    exclude_patterns: Vec<String>,
}

impl RemovePropertiesProcessor {
    /// Creates the processor.
    pub fn new(pattern: &str, exclude_patterns: &[&str]) -> Self {
        RemovePropertiesProcessor {
            pattern: pattern.to_string(),
            exclude_patterns: exclude_patterns.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Processor for RemovePropertiesProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let excludes: Vec<&str> = self.exclude_patterns.iter().map(String::as_str).collect();
        exchange.remove_properties(&self.pattern, &excludes);
        ready(Ok(()))
    }
}

/// Removes a variable, or all of them with `*` (the removeVariable EIP).
pub struct RemoveVariableProcessor {
    name: String,
}

impl RemoveVariableProcessor {
    /// Creates the processor.
    pub fn new(name: &str) -> Self {
        RemoveVariableProcessor {
            name: name.to_string(),
        }
    }
}

impl Processor for RemoveVariableProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        exchange.remove_variable(&self.name);
        ready(Ok(()))
    }
}

/// Evaluates an expression for its side effects, keeping the message (the script EIP).
pub struct ScriptProcessor {
    expression: SharedExpression,
}

impl ScriptProcessor {
    /// Creates the processor.
    pub fn new(expression: SharedExpression) -> Self {
        ScriptProcessor { expression }
    }
}

impl Processor for ScriptProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        ready(self.expression.evaluate_mut(exchange).map(|_| ()))
    }
}

/// The Java class name of `org.apache.camel.support.processor.PredicateValidationException`.
pub const PREDICATE_VALIDATION_EXCEPTION: &str =
    "org.apache.camel.support.processor.PredicateValidationException";

/// Fails the exchange with a `PredicateValidationException` when the predicate does not match
/// (the validate EIP, Java `PredicateValidatingProcessor`).
pub struct ValidateProcessor {
    predicate: camel_api::processor::SharedPredicate,
}

impl ValidateProcessor {
    /// Creates the processor.
    pub fn new(predicate: camel_api::processor::SharedPredicate) -> Self {
        ValidateProcessor { predicate }
    }
}

impl Processor for ValidateProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let result = self.predicate.matches(exchange).and_then(|matches| {
            log::debug!(
                "Validation {} for {exchange} with Predicate[{}]",
                if matches { "succeed" } else { "failed" },
                self.predicate.describe()
            );
            if matches {
                Ok(())
            } else {
                Err(CamelError::validation(
                    exchange,
                    &format!(
                        "Validation failed for Predicate[{}]",
                        self.predicate.describe()
                    ),
                )
                .with_subtype(PREDICATE_VALIDATION_EXCEPTION))
            }
        });
        ready(result)
    }
}
