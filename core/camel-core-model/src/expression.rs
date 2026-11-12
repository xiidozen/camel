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
    /// An expression with options (Java `SimpleExpression` `resultType`, `trim`, `pretty` and
    /// `nested`).
    Configured {
        /// The expression.
        definition: Box<ExpressionDefinition>,
        /// The options.
        options: ExpressionOptions,
    },
}

/// The options of an expression (applied in the Java order: nested, result type, trim,
/// pretty).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpressionOptions {
    /// The value is a text evaluated again as an expression of the language.
    pub nested: bool,
    /// The type the value is converted to (`boolean` makes it a predicate).
    pub result_type: Option<String>,
    /// The text of the value is trimmed.
    pub trim: bool,
    /// The JSON or XML text of the value is pretty printed.
    pub pretty: bool,
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
            ExpressionDefinition::Configured { definition, .. } => definition.language_name(),
            _ => None,
        }
    }

    /// The expression without its options.
    pub fn definition(&self) -> &ExpressionDefinition {
        match self {
            ExpressionDefinition::Configured { definition, .. } => definition.definition(),
            other => other,
        }
    }

    fn with_options(self, f: impl FnOnce(&mut ExpressionOptions)) -> Self {
        let (definition, mut options) = match self {
            ExpressionDefinition::Configured {
                definition,
                options,
            } => (definition, options),
            other => (Box::new(other), ExpressionOptions::default()),
        };
        f(&mut options);
        ExpressionDefinition::Configured {
            definition,
            options,
        }
    }

    /// Evaluates the value again as an expression of the language (Java `nested(true)`).
    pub fn nested(self, nested: bool) -> Self {
        self.with_options(|o| o.nested = nested)
    }

    /// Pretty prints a JSON or XML value (Java `.simple(text, true)`).
    pub fn pretty(self, pretty: bool) -> Self {
        self.with_options(|o| o.pretty = pretty)
    }

    /// Trims the text of the value.
    pub fn trim(self, trim: bool) -> Self {
        self.with_options(|o| o.trim = trim)
    }

    /// Converts the value to the type (Java `resultType`, such as `int` or `boolean`).
    pub fn result_type(self, type_name: &str) -> Self {
        self.with_options(|o| o.result_type = Some(type_name.to_string()))
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
            ExpressionDefinition::Configured { definition, .. } => definition.fmt(f),
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

impl From<camel_support::tokenizer::TokenizeExpression> for ExpressionDefinition {
    fn from(t: camel_support::tokenizer::TokenizeExpression) -> Self {
        ExpressionDefinition::Expression(t.into_expression())
    }
}

impl From<ValueBuilder> for ExpressionDefinition {
    fn from(v: ValueBuilder) -> Self {
        ExpressionDefinition::Expression(v.expression())
    }
}
