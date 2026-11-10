//! Languages (Java `org.apache.camel.spi.Language`): factories of expressions and predicates
//! from text, such as the simple language `${header.foo} == 'bar'`.

use crate::error::CamelResult;
use crate::processor::{SharedExpression, SharedPredicate};

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

    /// Whether expressions are evaluated once per text and can be cached (as singletons).
    fn is_singleton(&self) -> bool {
        true
    }
}
