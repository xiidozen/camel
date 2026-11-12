//! The bean language (Java `BeanLanguage` and `BeanExpression`): `beanName.method`,
//! `beanName?method=method`, `beanName::method`, with parameters (`greeter.hello('World')`)
//! and OGNL on the result (`greeter.hello.length()`).
//!
//! Differences with Java:
//!
//! * `type:com.foo.MyBean` (create the bean from its class) is not available: Rust has no
//!   class loading, bind the bean in the registry instead.
//! * The bean is looked up in the registry on each evaluation; whether it is a singleton or a
//!   prototype is decided by how it is bound
//!   ([`Registry::bind_as_prototype`](camel_api::spi::registry::Registry::bind_as_prototype)),
//!   so the `scope` option is accepted and has no effect.
//! * Without a method name, the bean must have exactly one method: Java chooses the method by
//!   the parameter types, which Rust beans do not declare. A value that is not an
//!   [`ExchangeBean`](camel_api::bean_binding::ExchangeBean) is the result itself.
//! * An error of the bean method is returned as the error of the evaluation (Java sets it on
//!   the exchange).

use camel_api::bean_binding::{
    ambiguous_method_call, exchange_bean, method_not_found, BeanInvocation,
    METHOD_NOT_FOUND_EXCEPTION,
};
use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, ExpressionPredicate, SharedExpression, SharedPredicate};
use camel_api::spi::language::Language;
use camel_core_languages::simple::ognl::{argument, invoke_ognl, method_and_args};
use camel_util::ognl_helper;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the bean language.
pub const LANGUAGE: &str = "bean";

/// The Java class name of `org.apache.camel.ExpressionIllegalSyntaxException`.
const EXPRESSION_ILLEGAL_SYNTAX_EXCEPTION: &str =
    "org.apache.camel.ExpressionIllegalSyntaxException";

/// The bean language: [`BeanExpression`]s from their text.
///
/// # Examples
///
/// ```
/// use camel_bean::{BeanLanguage, MethodBean};
/// use camel_core::prelude::*;
///
/// let context = camel_core::default_context();
/// let foo = MethodBean::new("MyBean").method("cheese", |inv| {
///     Ok(inv.header("foo").cloned().unwrap_or_default())
/// });
/// context.registry().unwrap().bind("foo", foo.into_value()).unwrap();
/// let mut exchange = Exchange::new(context.as_shared());
/// exchange.message_mut().set_header("foo", "abc");
///
/// let bean = context.resolve_language("bean").unwrap();
/// for text in ["foo.cheese", "foo?method=cheese", "foo::cheese"] {
///     let exp = bean.create_expression(text).unwrap();
///     assert_eq!(Value::from("abc"), exp.evaluate(&exchange).unwrap());
/// }
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct BeanLanguage;

impl BeanLanguage {
    /// The bean language.
    pub fn new() -> Self {
        BeanLanguage
    }
}

impl Language for BeanLanguage {
    fn create_predicate(&self, expression: &str) -> CamelResult<SharedPredicate> {
        Ok(Arc::new(ExpressionPredicate(
            self.create_expression(expression)?,
        )))
    }

    fn create_expression(&self, expression: &str) -> CamelResult<SharedExpression> {
        Ok(Arc::new(BeanExpression::parse(expression)?))
    }
}

/// The bean name, method and scope of a bean language text (Java
/// `BeanLanguage.createBeanExpression`).
///
/// # Errors
///
/// An invalid query (`bean?method=...`).
pub fn parse_bean_text(expression: &str) -> CamelResult<(String, Option<String>, Option<String>)> {
    if expression.contains("?method=") || expression.contains("?scope=") {
        let (name, query) = expression.split_once('?').unwrap_or((expression, ""));
        let map = camel_util::uri_support::parse_query(query)
            .map_err(|e| CamelError::illegal_argument(e.to_string()))?;
        let get = |key: &str| map.get(key).and_then(|v| v.as_str()).map(str::to_string);
        return Ok((name.to_string(), get("method"), get("scope")));
    }
    // first check :: because of my.own.Bean::method, unless it is inside the parameters
    let double_colon = expression.find("::");
    let parameters = expression.find('(');
    if let Some(i) = double_colon.filter(|&i| i > 0 && parameters.map_or(true, |p| i < p)) {
        return Ok((
            expression[..i].to_string(),
            Some(expression[i + 2..].to_string()),
            None,
        ));
    }
    if let Some(i) = expression.find('.').filter(|&i| i > 0) {
        return Ok((
            expression[..i].to_string(),
            Some(expression[i + 1..].to_string()),
            None,
        ));
    }
    Ok((expression.to_string(), None, None))
}

/// Where the bean of an expression comes from.
#[derive(Clone)]
enum BeanSource {
    /// Looked up in the registry by name.
    Name(String),
    /// The bean itself.
    Instance(Value),
}

/// Evaluates an expression by calling a method of a bean (Java `BeanExpression`).
#[derive(Clone)]
pub struct BeanExpression {
    source: BeanSource,
    method: Option<String>,
}

impl BeanExpression {
    /// An expression calling the method (a method call such as `hello('World')`, possibly
    /// followed by OGNL; `None` for the only method of the bean) of the bean with the name in
    /// the registry.
    pub fn new(bean_name: impl Into<String>, method: Option<&str>) -> Self {
        BeanExpression {
            source: BeanSource::Name(bean_name.into()),
            method: method.map(str::to_string),
        }
    }

    /// An expression calling the method of the bean.
    pub fn of_bean(bean: Value, method: Option<&str>) -> Self {
        BeanExpression {
            source: BeanSource::Instance(bean),
            method: method.map(str::to_string),
        }
    }

    /// The expression of a bean language text (see [`parse_bean_text`]).
    ///
    /// # Errors
    ///
    /// An invalid text, or a `type:` bean (no class loading in Rust).
    pub fn parse(expression: &str) -> CamelResult<Self> {
        let (name, method, _scope) = parse_bean_text(expression)?;
        let name = name.trim();
        if let Some(class) = name.strip_prefix("type:") {
            return Err(CamelError::illegal_argument(format!(
                "Cannot create the bean of class {class}: the Rust bean language has no class loading, bind the bean in the registry instead"
            )));
        }
        Ok(BeanExpression::new(name, method.as_deref()))
    }

    /// The name of the bean in the registry, if looked up by name.
    pub fn bean_name(&self) -> Option<&str> {
        match &self.source {
            BeanSource::Name(n) => Some(n),
            BeanSource::Instance(_) => None,
        }
    }

    /// The method, if any.
    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }

    fn bean(&self, context: &Arc<dyn CamelContext>) -> CamelResult<Value> {
        match &self.source {
            BeanSource::Instance(v) => Ok(v.clone()),
            BeanSource::Name(name) => context
                .registry()
                .and_then(|r| r.lookup_by_name(name))
                .ok_or_else(|| CamelError::no_such_bean(name)),
        }
    }

    /// Validates that the bean has the method (Java `validateHasMethod`): an invalid OGNL
    /// method is a `MethodNotFoundException` caused by an `ExpressionIllegalSyntaxException`.
    fn validate(&self, bean: &Value) -> CamelResult<()> {
        let Some(method) = &self.method else {
            return Ok(());
        };
        let bean_text = bean.to_string();
        if ognl_helper::is_invalid_valid_ognl_expression(method) {
            let cause = CamelError::new(
                ErrorKind::ExpressionIllegalSyntax,
                format!("Illegal syntax: {method}"),
            )
            .with_subtype(EXPRESSION_ILLEGAL_SYNTAX_EXCEPTION)
            .with_detail(method);
            return Err(CamelError::with_cause(
                ErrorKind::RuntimeExchange,
                format!("Method with name: {method} not found on bean: {bean_text}"),
                cause,
            )
            .with_subtype(METHOD_NOT_FOUND_EXCEPTION)
            .with_detail(method));
        }
        if let Some(b) = exchange_bean(bean) {
            let first = first_step(method)?;
            let (name, _) = method_and_args(&first);
            if !b.has_method(&name) {
                return Err(method_not_found(&bean_text, b.bean_type(), &name));
            }
        }
        Ok(())
    }

    /// Calls the bean on a copy of the exchange (Java avoids side effects on the exchange),
    /// returning the result and the copy.
    fn run(&self, exchange: &Exchange) -> CamelResult<(Value, Exchange)> {
        let bean = self.bean(exchange.context())?;
        self.validate(&bean)?;
        let mut copy = exchange.clone();
        // remove any existing exception in case of OGNL on the exception
        copy.set_exception(None);
        let result = self.call(&bean, &mut copy)?;
        Ok((result, copy))
    }

    fn call(&self, bean: &Value, exchange: &mut Exchange) -> CamelResult<Value> {
        let Some(method) = &self.method else {
            return match exchange_bean(bean) {
                Some(b) => {
                    let names = b.method_names();
                    match names.as_slice() {
                        [only] => b.invoke(&mut BeanInvocation::new(exchange, only, None)),
                        _ => Err(ambiguous_method_call(&names)),
                    }
                }
                None => Ok(bean.clone()),
            };
        };
        let steps = ognl_helper::split_ognl(method)
            .map_err(|m| CamelError::new(ErrorKind::ExpressionIllegalSyntax, m))?;
        let (first, rest) = match steps.split_first() {
            Some((first, rest)) => (first.clone(), rest.concat()),
            None => (method.clone(), String::new()),
        };
        let result = match exchange_bean(bean) {
            Some(b) => {
                let (name, texts) = method_and_args(&first);
                let args = match texts {
                    Some(texts) => {
                        let mut values = Vec::with_capacity(texts.len());
                        for t in &texts {
                            values.push(argument(t, exchange)?);
                        }
                        Some(values)
                    }
                    None => None,
                };
                if !b.has_method(&name) {
                    return Err(method_not_found(&bean.to_string(), b.bean_type(), &name));
                }
                b.invoke(&mut BeanInvocation::new(exchange, &name, args))?
            }
            None => invoke_ognl(bean, &first, exchange)?,
        };
        if rest.is_empty() {
            Ok(result)
        } else {
            invoke_ognl(&result, &rest, exchange)
        }
    }
}

fn first_step(method: &str) -> CamelResult<String> {
    let steps = ognl_helper::split_ognl(method)
        .map_err(|m| CamelError::new(ErrorKind::ExpressionIllegalSyntax, m))?;
    Ok(steps
        .into_iter()
        .next()
        .unwrap_or_else(|| method.to_string()))
}

impl Expression for BeanExpression {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.run(exchange).map(|(v, _)| v)
    }

    /// Evaluates, then copies the properties and the headers of the copy of the exchange the
    /// bean was called with back to the exchange (Java `BeanExpression.invokeBean`).
    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        let (value, copy) = self.run(exchange)?;
        for (k, v) in copy.properties() {
            exchange.set_property(k, v.clone());
        }
        for (k, v) in copy.message().headers().iter() {
            exchange.message_mut().set_header(k, v.clone());
        }
        Ok(value)
    }

    /// Validates that the bean has the method, when the bean is in the registry (Java fails
    /// to create the route otherwise).
    fn init(&self, context: &Arc<dyn CamelContext>) -> CamelResult<()> {
        let bean = self.bean(context)?;
        self.validate(&bean)
    }

    fn describe(&self) -> String {
        let bean = match &self.source {
            BeanSource::Name(n) => n.clone(),
            BeanSource::Instance(v) => v.to_string(),
        };
        match &self.method {
            Some(m) => format!("BeanExpression[{bean} method:{m}]"),
            None => format!("BeanExpression[{bean}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_texts() {
        let p = |t: &str| parse_bean_text(t).unwrap();
        assert_eq!(("foo".into(), Some("cheese".into()), None), p("foo.cheese"));
        assert_eq!(
            ("foo".into(), Some("cheese".into()), None),
            p("foo?method=cheese")
        );
        assert_eq!(
            ("foo".into(), Some("echo('e::o')".into()), None),
            p("foo.echo('e::o')")
        );
        assert_eq!(
            (
                "my.company.MyClass".into(),
                Some("echo('a.b')".into()),
                None
            ),
            p("my.company.MyClass::echo('a.b')")
        );
        assert_eq!(
            ("foo".into(), Some("x".into()), Some("Request".into())),
            p("foo?method=x&scope=Request")
        );
        assert_eq!(("foo:x".into(), None, None), p("foo:x"));
        assert!(BeanExpression::parse("type:com.foo.MyBean").is_err());
    }
}
