//! The simple language (Java `SimpleLanguage`).

use super::expression_parser::SimpleExpressionParser;
use super::functions::with_custom_functions;
use super::illegal_syntax;
use super::init_block;
use super::predicate_parser::SimplePredicateParser;
use camel_api::context::{context_plugin, CamelContext};
use camel_api::error::CamelResult;
use camel_api::processor::{SharedExpression, SharedPredicate};
use camel_api::spi::language::Language;
use camel_api::spi::simple_function::SimpleFunctionRegistry;
use camel_support::builder::named_expression;
use camel_support::resource_helper;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, Weak};

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
    context: RwLock<Option<Weak<dyn CamelContext>>>,
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
        with_custom_functions(self.custom_functions(), || {
            if init_block::has_init_block(expression) {
                return init_block::parse_expression(expression, self.allow_escape);
            }
            if let Some(e) = resource_expression(expression, self.allow_escape)? {
                return Ok(e);
            }
            SimpleExpressionParser::new(expression, self.allow_escape)
                .parse_expression()
                .map_err(|e| illegal_syntax(expression, &e))
        })
    }

    /// The custom functions of the context.
    fn custom_functions(&self) -> Option<Arc<SimpleFunctionRegistry>> {
        let context = self.context.read().unwrap().as_ref()?.upgrade()?;
        context_plugin::<SimpleFunctionRegistry>(context.as_ref())
    }

    /// Parses the predicate (without the cache).
    ///
    /// # Errors
    ///
    /// Returns a `SimpleIllegalSyntaxException` for an invalid predicate.
    pub fn parse_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        with_custom_functions(self.custom_functions(), || {
            if init_block::has_init_block(expression) {
                return init_block::parse_predicate(expression, self.allow_escape);
            }
            SimplePredicateParser::new(expression, self.allow_escape)
                .parse_predicate()
                .map_err(|e| illegal_syntax(expression, &e))
        })
    }
}

/// `resource:scheme:path`: the expression is the content of the resource (Java
/// `ScriptHelper.resolveOptionalExternalScript`); with functions in the uri the resource is
/// loaded when evaluating. Without a scheme the text is a plain expression.
fn resource_expression(
    expression: &str,
    allow_escape: bool,
) -> CamelResult<Option<SharedExpression>> {
    let Some(uri) = expression.strip_prefix("resource:") else {
        return Ok(None);
    };
    if !resource_helper::has_scheme(uri) {
        return Ok(None);
    }
    if uri.contains("${") {
        let uri_expression = SimpleExpressionParser::new(uri, allow_escape)
            .parse_expression()
            .map_err(|e| illegal_syntax(uri, &e))?;
        return Ok(Some(named_expression(
            format!("resource:{uri}"),
            move |e| {
                let uri = uri_expression.evaluate(e)?.to_string();
                let text = resource_helper::resolve_resource_as_string(&uri)?;
                SimpleLanguage::new().parse_expression(&text)?.evaluate(e)
            },
        )));
    }
    let text = resource_helper::resolve_resource_as_string(uri)?;
    SimpleExpressionParser::new(&text, allow_escape)
        .parse_expression()
        .map(Some)
        .map_err(|e| illegal_syntax(&text, &e))
}

impl Language for SimpleLanguage {
    fn set_camel_context(&self, context: Weak<dyn CamelContext>) {
        *self.context.write().unwrap() = Some(context);
    }

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
