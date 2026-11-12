//! The bean function of the simple language (Java `BeanFunctionFactory`): `${bean:ref}`,
//! `${bean:ref.method}`, `${bean:ref::method}`, `${bean:ref?method=m&scope=s}`. The
//! expression is created by the `bean` language of the context of the exchange (the
//! `camel-bean` crate), as Java resolves the `bean` language.

use super::functions::if_starts_with_return_remainder;
use super::{ParseResult, ParserError};
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, SharedExpression};
use camel_util::value::Value;
use std::sync::{Arc, OnceLock};

/// The reference, method and scope of the text after `bean:` (Java
/// `BeanFunctionFactory.parseBeanRemainder`).
pub fn parse_bean_remainder(remainder: &str) -> (String, Option<String>, Option<String>) {
    let mut reference = remainder.to_string();
    let mut method = None;
    let mut scope = None;
    if remainder.contains("?method=") || remainder.contains("?scope=") {
        let (r, query) = remainder.split_once('?').unwrap_or((remainder, ""));
        reference = r.to_string();
        if let Ok(map) = camel_util::uri_support::parse_query(query) {
            method = map.get("method").map(|v| v.to_string());
            scope = map.get("scope").map(|v| v.to_string());
        }
    } else {
        // first check :: because of my.own.Bean::method, unless it is inside the parameters
        let double_colon = remainder.find("::");
        let parameters = remainder.find('(');
        if let Some(i) = double_colon.filter(|&i| i > 0 && parameters.map_or(true, |p| i < p)) {
            reference = remainder[..i].to_string();
            method = Some(remainder[i + 2..].to_string());
        } else if remainder.starts_with("type:") {
            // type:com.foo.MyClass.myMethod: the method is the last part when it starts with
            // a lower case letter, as Java method names do
            let before_params = match parameters {
                Some(p) if p > 0 => &remainder[..p],
                _ => remainder,
            };
            if let Some(i) = before_params.rfind('.') {
                if i > 0
                    && before_params[i + 1..]
                        .chars()
                        .next()
                        .is_some_and(char::is_lowercase)
                {
                    reference = remainder[..i].to_string();
                    method = Some(remainder[i + 1..].to_string());
                }
            }
        } else if let Some(i) = remainder.find('.').filter(|&i| i > 0) {
            reference = remainder[..i].to_string();
            method = Some(remainder[i + 1..].to_string());
        }
    }
    (reference.trim().to_string(), method, scope)
}

/// `${bean:...}`, or `None` when the function is not a bean function.
///
/// # Errors
///
/// A `type:` reference (Rust has no class loading).
pub fn bean_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let Some(remainder) = if_starts_with_return_remainder("bean:", function) else {
        return Ok(None);
    };
    let (reference, method, _scope) = parse_bean_remainder(remainder);
    if let Some(class) = reference.strip_prefix("type:") {
        return Err(ParserError::new(
            format!("Cannot create the bean of class {class}: the Rust bean language has no class loading, bind the bean in the registry instead"),
            index,
        ));
    }
    // the bean language text: the scope has no effect in Rust (see the bean language)
    let text = match &method {
        Some(m) => format!("{reference}::{m}"),
        None => reference,
    };
    Ok(Some(Arc::new(BeanFunction {
        text,
        description: format!("${{{function}}}"),
        expression: OnceLock::new(),
    })))
}

/// `${bean:...}`: the expression of the bean language, created on first use.
struct BeanFunction {
    text: String,
    description: String,
    expression: OnceLock<SharedExpression>,
}

impl BeanFunction {
    fn expression(&self, exchange: &Exchange) -> CamelResult<SharedExpression> {
        if let Some(e) = self.expression.get() {
            return Ok(e.clone());
        }
        let language = exchange.context().resolve_language("bean")?;
        let e = language.create_expression(&self.text)?;
        let _ = self.expression.set(e.clone());
        Ok(e)
    }
}

impl Expression for BeanFunction {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.expression(exchange)?.evaluate(exchange)
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.expression(exchange)?.evaluate_mut(exchange)
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::parse_bean_remainder;

    #[test]
    fn remainders() {
        let p = |t: &str| parse_bean_remainder(t);
        assert_eq!(("foo".into(), Some("bar".into()), None), p("foo.bar"));
        assert_eq!(("foo".into(), Some("bar".into()), None), p("foo::bar"));
        assert_eq!(
            ("foo".into(), Some("bar".into()), Some("Prototype".into())),
            p("foo?method=bar&scope=Prototype")
        );
        assert_eq!(
            (
                "type:com.foo.MyClass".into(),
                Some("hello('a.b')".into()),
                None
            ),
            p("type:com.foo.MyClass.hello('a.b')")
        );
        assert_eq!(
            ("type:com.foo.MyClass".into(), None, None),
            p("type:com.foo.MyClass")
        );
        assert_eq!(("foo".into(), None, None), p(" foo "));
    }
}
