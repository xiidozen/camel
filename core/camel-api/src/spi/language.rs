//! Languages (Java `org.apache.camel.spi.Language`): factories of expressions and predicates
//! from text, such as the simple language `${header.foo} == 'bar'`.

use crate::context::CamelContext;
use crate::error::CamelResult;
use crate::processor::{SharedExpression, SharedPredicate};
use std::sync::Weak;

/// A language: creates expressions and predicates from their text.
pub trait Language: Send + Sync {
    /// Creates a predicate.
    ///
    /// # Errors
    ///
    /// Returns an error (typically `ExpressionIllegalSyntaxException`) if the text is invalid.
    fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate>;

    /// Creates an expression.
    ///
    /// # Errors
    ///
    /// Returns an error (typically `ExpressionIllegalSyntaxException`) if the text is invalid.
    fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression>;

    /// Creates the expression with the options of the language (Java language properties,
    /// such as the `suppressExceptions` of jsonpath); by default the options are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression or an option is invalid.
    fn create_expression_with_options(
        &self,
        expression: &str,
        options: &[(String, String)],
    ) -> CamelResult<SharedExpression> {
        let _ = options;
        self.create_expression(expression)
    }

    /// Creates the predicate with the options of the language; by default the options are
    /// ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if the predicate or an option is invalid.
    fn create_predicate_with_options(
        &self,
        expression: &str,
        options: &[(String, String)],
    ) -> CamelResult<SharedPredicate> {
        let _ = options;
        self.create_predicate(expression)
    }

    /// Gives the language its context when the context creates it (Java `CamelContextAware`);
    /// the default ignores it.
    fn set_camel_context(&self, context: Weak<dyn CamelContext>) {
        let _ = context;
    }

    /// Whether expressions are evaluated once per text and can be cached (as singletons).
    fn is_singleton(&self) -> bool {
        true
    }
}
