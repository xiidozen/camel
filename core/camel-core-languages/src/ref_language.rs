//! The ref language (Java `RefLanguage`): the expression (or predicate) bean of the registry
//! with the name; a name with simple functions (such as `${header.ref}`) is evaluated on each
//! exchange.

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{
    expression_fn, expression_of, predicate_of, ExpressionPredicate, SharedExpression,
    SharedPredicate,
};
use camel_api::spi::language::Language;
use camel_util::value::Value;
use std::sync::{Arc, RwLock, Weak};

/// Whether the text has a simple function (Java `LanguageSupport.hasSimpleFunction`).
fn has_simple_function(text: &str) -> bool {
    text.contains("${") || text.contains("$simple{")
}

/// The ref language.
#[derive(Default)]
pub struct RefLanguage {
    context: RwLock<Option<Weak<dyn CamelContext>>>,
}

fn not_found(name: &str) -> CamelError {
    CamelError::illegal_argument(format!(
        "Cannot find expression or predicate in registry with ref: {name}"
    ))
}

/// The expression of the bean (an expression, or a predicate as a boolean expression).
fn lookup(context: &dyn CamelContext, name: &str) -> CamelResult<SharedExpression> {
    let value = context
        .registry()
        .and_then(|r| r.lookup_by_name(name))
        .ok_or_else(|| not_found(name))?;
    if let Some(e) = expression_of(&value) {
        return Ok(e);
    }
    if let Some(p) = predicate_of(&value) {
        return Ok(Arc::new(expression_fn(move |e: &Exchange| {
            Ok(Value::Bool(p.matches(e)?))
        })));
    }
    Err(not_found(name))
}

impl RefLanguage {
    fn context(&self) -> Option<Arc<dyn CamelContext>> {
        self.context.read().unwrap().as_ref()?.upgrade()
    }
}

impl Language for RefLanguage {
    fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        // a predicate bean as is
        if !has_simple_function(expression) {
            if let Some(c) = self.context() {
                if let Some(p) = c
                    .registry()
                    .and_then(|r| r.lookup_by_name(expression.trim()))
                    .as_ref()
                    .and_then(predicate_of)
                {
                    return Ok(p);
                }
            }
        }
        Ok(Arc::new(ExpressionPredicate(
            self.create_expression(expression)?,
        )))
    }

    fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
        let name = expression.trim().to_string();
        if has_simple_function(&name) {
            // the name is evaluated on each exchange (Java createDynamic)
            let simple = crate::simple::language::SimpleLanguage::new().parse_expression(&name)?;
            return Ok(Arc::new(expression_fn(move |e: &Exchange| {
                let r = simple.evaluate(e)?.to_string();
                lookup(e.context().as_ref(), &r)?.evaluate(e)
            })));
        }
        match self.context() {
            // the bean of the registry when known, else on first use
            Some(c) => lookup(c.as_ref(), &name),
            None => Ok(Arc::new(expression_fn(move |e: &Exchange| {
                lookup(e.context().as_ref(), &name)?.evaluate(e)
            }))),
        }
    }

    fn set_camel_context(&self, context: Weak<dyn CamelContext>) {
        *self.context.write().unwrap() = Some(context);
    }
}
