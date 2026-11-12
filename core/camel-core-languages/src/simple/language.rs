//! The simple language (Java `SimpleLanguage`).

use super::expression_parser::SimpleExpressionParser;
use super::illegal_syntax;
use super::init_block;
use super::predicate_parser::SimplePredicateParser;
use camel_api::error::CamelResult;
use camel_api::processor::{SharedExpression, SharedPredicate};
use camel_api::spi::language::Language;
use std::collections::HashMap;
use std::sync::Mutex;

/// The maximum number of cached expressions and predicates (Java
/// `CamelContextHelper.getMaximumSimpleCacheSize`, default 1000).
pub const MAXIMUM_CACHE_SIZE: usize = 1000;

/// The simple language: creates expressions from templates such as `Hello ${body}` and
/// predicates such as `${header.foo} == 'bar'`. Parsed expressions and predicates are cached.
///
/// # Examples
///
/// ```
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
/// use camel_api::spi::language::Language;
/// use camel_core_languages::simple::language::SimpleLanguage;
///
/// let simple = SimpleLanguage::new();
/// let mut e = Exchange::with_body(StandaloneContext::shared(), "World");
/// e.message_mut().set_header("lang", "en");
/// let exp = simple.create_expression("Hello ${body} (${header.lang})").unwrap();
/// assert_eq!("Hello World (en)", exp.evaluate(&e).unwrap().to_string());
/// let p = simple.create_predicate("${header.lang} == 'en' && ${body} contains 'orl'").unwrap();
/// assert!(p.matches(&e).unwrap());
/// ```
#[derive(Default)]
pub struct SimpleLanguage {
    allow_escape: bool,
    expressions: Mutex<HashMap<String, SharedExpression>>,
    predicates: Mutex<HashMap<String, SharedPredicate>>,
}

impl SimpleLanguage {
    /// The simple language (escapes such as `\n` allowed).
    pub fn new() -> Self {
        SimpleLanguage {
            allow_escape: true,
            ..Default::default()
        }
    }

    /// Parses the expression (without the cache).
    ///
    /// # Errors
    ///
    /// Returns a `SimpleIllegalSyntaxException` for an invalid expression.
    pub fn parse_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
        if init_block::has_init_block(expression) {
            return init_block::parse_expression(expression, self.allow_escape);
        }
        SimpleExpressionParser::new(expression, self.allow_escape)
            .parse_expression()
            .map_err(|e| illegal_syntax(expression, &e))
    }

    /// Parses the predicate (without the cache).
    ///
    /// # Errors
    ///
    /// Returns a `SimpleIllegalSyntaxException` for an invalid predicate.
    pub fn parse_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        if init_block::has_init_block(expression) {
            return init_block::parse_predicate(expression, self.allow_escape);
        }
        SimplePredicateParser::new(expression, self.allow_escape)
            .parse_predicate()
            .map_err(|e| illegal_syntax(expression, &e))
    }
}

impl Language for SimpleLanguage {
    fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        if let Some(p) = self.predicates.lock().unwrap().get(expression) {
            return Ok(p.clone());
        }
        let p = self.parse_predicate(expression)?;
        let mut cache = self.predicates.lock().unwrap();
        if cache.len() < MAXIMUM_CACHE_SIZE {
            cache.insert(expression.to_string(), p.clone());
        }
        Ok(p)
    }

    fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
        if let Some(e) = self.expressions.lock().unwrap().get(expression) {
            return Ok(e.clone());
        }
        let e = self.parse_expression(expression)?;
        let mut cache = self.expressions.lock().unwrap();
        if cache.len() < MAXIMUM_CACHE_SIZE {
            cache.insert(expression.to_string(), e.clone());
        }
        Ok(e)
    }
}
