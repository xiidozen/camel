//! # camel-core-languages
//!
//! The core languages of Camel (the Java `core/camel-core-languages` module):
//!
//! - [`simple`](simple::language::SimpleLanguage): templates and predicates such as
//!   `Hello ${body}` and `${header.foo} == 'bar'`;
//! - [`constant`](ConstantLanguage): a fixed value;
//! - [`header`](HeaderLanguage), [`exchangeProperty`](ExchangePropertyLanguage) and
//!   [`variable`](VariableLanguage): the value of a header, exchange property or variable.
//!
//! [`register_core_languages`] registers them on a context.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod simple;

use camel_api::error::CamelResult;
use camel_api::processor::{ExpressionPredicate, SharedExpression, SharedPredicate};
use camel_api::spi::language::Language;
use camel_support::builder::{
    constant_expression, exchange_property_expression, header_expression, variable_expression,
};
use std::sync::Arc;

/// The constant language (Java `ConstantLanguage`): the expression text is the value.
#[derive(Default)]
pub struct ConstantLanguage;

impl Language for ConstantLanguage {
    fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        Ok(Arc::new(ExpressionPredicate(
            self.create_expression(expression)?,
        )))
    }

    fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
        Ok(constant_expression(expression))
    }
}

macro_rules! name_language {
    ($(#[$m:meta])* $name:ident, $f:ident) => {
        $(#[$m])*
        #[derive(Default)]
        pub struct $name;

        impl Language for $name {
            fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
                Ok(Arc::new(ExpressionPredicate(self.create_expression(expression)?)))
            }

            fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
                Ok($f(expression.trim()))
            }
        }
    };
}

name_language!(
    /// The header language (Java `HeaderLanguage`): the value of the header named by the text.
    HeaderLanguage,
    header_expression
);
name_language!(
    /// The exchange property language (Java `ExchangePropertyLanguage`).
    ExchangePropertyLanguage,
    exchange_property_expression
);
name_language!(
    /// The variable language (Java `VariableLanguage`).
    VariableLanguage,
    variable_expression
);

/// A language factory: the language name and how to create it.
pub type LanguageEntry = (&'static str, fn() -> Arc<dyn Language>);

/// The core languages with their names.
pub fn core_languages() -> Vec<LanguageEntry> {
    vec![
        ("simple", || {
            Arc::new(simple::language::SimpleLanguage::new())
        }),
        ("constant", || Arc::new(ConstantLanguage)),
        ("header", || Arc::new(HeaderLanguage)),
        ("exchangeProperty", || Arc::new(ExchangePropertyLanguage)),
        ("variable", || Arc::new(VariableLanguage)),
    ]
}

/// Registers the core languages through the callback (for example
/// `DefaultCamelContext::register_language_factory`).
pub fn register_core_languages(mut register: impl FnMut(&'static str, fn() -> Arc<dyn Language>)) {
    for (name, f) in core_languages() {
        register(name, f);
    }
}
