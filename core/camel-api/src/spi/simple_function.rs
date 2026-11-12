//! The custom functions of the simple language (Java `SimpleFunction` and
//! `SimpleFunctionRegistry`): `${function(name)}`, `${function(name,exp)}`, or `${name}` and
//! `${name(exp)}` when no built-in function has the name.
//!
//! The registry is a plugin of the context
//! ([`context_plugin::<SimpleFunctionRegistry>`](crate::context::context_plugin)).

use crate::error::CamelResult;
use crate::exchange::Exchange;
use crate::processor::SharedExpression;
use camel_util::value::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// A custom function of the simple language.
pub trait SimpleFunction: Send + Sync {
    /// The name of the function.
    fn name(&self) -> &str;

    /// Applies the function to the input: the value of the argument, or the message body.
    ///
    /// # Errors
    ///
    /// The error of the function.
    fn apply(&self, exchange: &Exchange, input: Value) -> CamelResult<Value>;
}

/// A function of an expression: the expression is evaluated with the input as the message
/// body (such as a chain `${trim()} ~> ${uppercase()}`).
struct ExpressionFunction {
    name: String,
    expression: SharedExpression,
}

impl SimpleFunction for ExpressionFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn apply(&self, exchange: &Exchange, input: Value) -> CamelResult<Value> {
        let mut copy = exchange.clone();
        copy.message_mut().set_body(input);
        self.expression.evaluate_mut(&mut copy)
    }
}

/// The registry of the custom functions (Java `SimpleFunctionRegistry`).
#[derive(Default)]
pub struct SimpleFunctionRegistry {
    functions: RwLock<BTreeMap<String, Arc<dyn SimpleFunction>>>,
}

impl SimpleFunctionRegistry {
    /// Adds the function (replacing one of the same name).
    pub fn add_function(&self, function: Arc<dyn SimpleFunction>) {
        self.functions
            .write()
            .unwrap()
            .insert(function.name().to_string(), function);
    }

    /// Adds a function of the expression, evaluated with the input as the message body.
    pub fn add_expression_function(&self, name: &str, expression: SharedExpression) {
        self.add_function(Arc::new(ExpressionFunction {
            name: name.to_string(),
            expression,
        }));
    }

    /// Removes the function.
    pub fn remove_function(&self, name: &str) -> bool {
        self.functions.write().unwrap().remove(name).is_some()
    }

    /// The function of the name.
    pub fn function(&self, name: &str) -> Option<Arc<dyn SimpleFunction>> {
        self.functions.read().unwrap().get(name).cloned()
    }

    /// The number of custom functions (Java `customSize`).
    pub fn custom_size(&self) -> usize {
        self.functions.read().unwrap().len()
    }

    /// The names of the functions.
    pub fn names(&self) -> Vec<String> {
        self.functions.read().unwrap().keys().cloned().collect()
    }
}
