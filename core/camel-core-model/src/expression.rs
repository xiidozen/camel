//! Expressions in the model (Java `org.apache.camel.model.language.ExpressionDefinition`):
//! either a language expression given as text (such as `simple` `${body}`), as written in YAML
//! and XML routes, or an expression or predicate built in Rust code.

use camel_api::processor::{SharedExpression, SharedPredicate};
use camel_support::builder::ValueBuilder;
use std::fmt;

/// An expression (or predicate) of the model.
#[derive(Clone)]
pub enum ExpressionDefinition {
    /// An expression in a language, such as `simple` with `${header.foo} == 'bar'`.
    Language {
        /// The language name.
        language: String,
        /// The expression text.
        expression: String,
    },
    /// An expression built in Rust code.
    Expression(SharedExpression),
    /// A predicate built in Rust code.
    Predicate(SharedPredicate),
}

impl ExpressionDefinition {
    /// An expression in the language.
    pub fn language(language: &str, expression: &str) -> Self {
        ExpressionDefinition::Language {
            language: language.to_string(),
            expression: expression.to_string(),
        }
    }

    /// The language name, if this is a language expression.
    pub fn language_name(&self) -> Option<&str> {
        match self {
            ExpressionDefinition::Language { language, .. } => Some(language),
            _ => None,
        }
    }
}

impl fmt::Display for ExpressionDefinition {
    /// Java `ExpressionDefinition.toString()`: `simple{${body}}` for a language expression, or
    /// the text form of the Rust expression or predicate.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpressionDefinition::Language {
                language,
                expression,
            } => write!(f, "{language}{{{expression}}}"),
            ExpressionDefinition::Expression(e) => f.write_str(&e.describe()),
            ExpressionDefinition::Predicate(p) => f.write_str(&p.describe()),
        }
    }
}

impl fmt::Debug for ExpressionDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExpressionDefinition({self})")
    }
}

impl From<SharedExpression> for ExpressionDefinition {
    fn from(e: SharedExpression) -> Self {
        ExpressionDefinition::Expression(e)
    }
}

impl From<SharedPredicate> for ExpressionDefinition {
    fn from(p: SharedPredicate) -> Self {
        ExpressionDefinition::Predicate(p)
    }
}

impl From<ValueBuilder> for ExpressionDefinition {
    fn from(v: ValueBuilder) -> Self {
        ExpressionDefinition::Expression(v.expression())
    }
}
