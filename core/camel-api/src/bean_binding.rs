//! Bean binding: how the bean language, the bean component and the bean EIP call a method of a
//! bean with the exchange (Java `BeanInfo`, `MethodInfo` and the parameter binding of
//! `camel-bean`).
//!
//! Java finds the methods of a bean by reflection and binds each parameter from the exchange:
//! the explicit parameters of the call (`echo('a')`), else the message body for the first
//! parameter, an `Exchange` parameter to the exchange, `@Header("foo")` to a header, ...
//! Rust beans declare their methods by implementing [`ExchangeBean`], and each method reads
//! what it needs from the [`BeanInvocation`]: [`parameter`](BeanInvocation::parameter) is the
//! explicit parameter, else the body for the first one, as Java binds them.
//!
//! # Examples
//!
//! ```
//! use camel_api::bean_binding::{bean_value, exchange_bean, BeanInvocation, ExchangeBean};
//! use camel_api::error::CamelResult;
//! use camel_util::value::Value;
//!
//! struct Greeter;
//!
//! impl ExchangeBean for Greeter {
//!     fn bean_type(&self) -> &str {
//!         "Greeter"
//!     }
//!
//!     fn method_names(&self) -> Vec<String> {
//!         vec!["hello".to_string()]
//!     }
//!
//!     fn invoke(&self, invocation: &mut BeanInvocation<'_>) -> CamelResult<Value> {
//!         // the explicit parameter, else the message body
//!         let name = invocation.parameter_as::<String>(0)?.unwrap_or_default();
//!         Ok(Value::from(format!("Hello {name}")))
//!     }
//! }
//!
//! let value = bean_value(Greeter);
//! let bean = exchange_bean(&value).unwrap();
//! assert!(bean.has_method("hello"));
//! assert_eq!("Greeter", value.to_string());
//! ```

use crate::convert::FromValue;
use crate::error::{CamelError, CamelResult, ErrorKind};
use crate::exchange::Exchange;
use camel_util::value::{ObjectRef, Value};
use std::fmt;
use std::sync::Arc;

/// The Java class name of `org.apache.camel.component.bean.MethodNotFoundException`.
pub const METHOD_NOT_FOUND_EXCEPTION: &str =
    "org.apache.camel.component.bean.MethodNotFoundException";

/// The Java class name of `org.apache.camel.component.bean.AmbiguousMethodCallException`.
pub const AMBIGUOUS_METHOD_CALL_EXCEPTION: &str =
    "org.apache.camel.component.bean.AmbiguousMethodCallException";

/// A call of a bean method: the method name, the exchange and the explicit parameters (Java
/// `MethodInvocation` with the parameters bound by `MethodInfo`).
pub struct BeanInvocation<'a> {
    exchange: &'a mut Exchange,
    method: &'a str,
    args: Option<Vec<Value>>,
}

impl<'a> BeanInvocation<'a> {
    /// A call of the method on the exchange, with the explicit parameters of
    /// `method(a, b)` (`None` when the method is called without parentheses).
    pub fn new(exchange: &'a mut Exchange, method: &'a str, args: Option<Vec<Value>>) -> Self {
        BeanInvocation {
            exchange,
            method,
            args,
        }
    }

    /// The name of the method.
    pub fn method(&self) -> &str {
        self.method
    }

    /// The exchange (Java binds an `Exchange` parameter to it).
    pub fn exchange(&self) -> &Exchange {
        self.exchange
    }

    /// The exchange, to change it (the bean language calls the bean on a copy of the
    /// exchange and copies the headers and properties back, as Java).
    pub fn exchange_mut(&mut self) -> &mut Exchange {
        self.exchange
    }

    /// The explicit parameters, `None` when the method was called without parentheses.
    pub fn args(&self) -> Option<&[Value]> {
        self.args.as_deref()
    }

    /// The parameter at the index (Java parameter binding): the explicit parameter when the
    /// method was called with parameters, else the message body for the first parameter;
    /// `Null` otherwise.
    pub fn parameter(&self, index: usize) -> Value {
        match &self.args {
            Some(args) => args.get(index).cloned().unwrap_or(Value::Null),
            None if index == 0 => self.exchange.message().body().clone(),
            None => Value::Null,
        }
    }

    /// The [`parameter`](Self::parameter) converted to the type.
    ///
    /// # Errors
    ///
    /// The conversion error.
    pub fn parameter_as<T: FromValue>(&self, index: usize) -> CamelResult<Option<T>> {
        T::from_value(&self.parameter(index))
    }

    /// The value of the header (Java `@Header("name")`).
    pub fn header(&self, name: &str) -> Option<&Value> {
        self.exchange.message().header(name)
    }
}

/// A bean whose methods the bean language, component and EIP can call with an exchange (the
/// Rust side of the Java bean binding; see the [module documentation](self)).
pub trait ExchangeBean: Send + Sync + 'static {
    /// The type of the bean, used in messages (the Java class name).
    fn bean_type(&self) -> &str;

    /// The names of the methods of the bean.
    fn method_names(&self) -> Vec<String>;

    /// Whether the bean has the method.
    fn has_method(&self, name: &str) -> bool {
        self.method_names().iter().any(|m| m == name)
    }

    /// Calls the method of the invocation (one of [`method_names`](Self::method_names)).
    ///
    /// # Errors
    ///
    /// The error of the method.
    fn invoke(&self, invocation: &mut BeanInvocation<'_>) -> CamelResult<Value>;
}

/// The value holding an [`ExchangeBean`] (a concrete type, so it can be found again in a
/// type erased [`Value`]).
#[derive(Clone)]
struct BeanRef(Arc<dyn ExchangeBean>);

impl fmt::Display for BeanRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.bean_type())
    }
}

/// The bean as a value, for example to bind it in the registry.
pub fn bean_value(bean: impl ExchangeBean) -> Value {
    shared_bean_value(Arc::new(bean))
}

/// The shared bean as a value.
pub fn shared_bean_value(bean: Arc<dyn ExchangeBean>) -> Value {
    Value::Object(ObjectRef::new_display(BeanRef(bean)))
}

/// The bean held by the value, if it holds an [`ExchangeBean`].
pub fn exchange_bean(value: &Value) -> Option<Arc<dyn ExchangeBean>> {
    match value {
        Value::Object(o) => o.downcast_ref::<BeanRef>().map(|b| b.0.clone()),
        _ => None,
    }
}

/// A `MethodNotFoundException` (Java `Method with name: x not found on bean: y of type: z`).
pub fn method_not_found(bean: &str, bean_type: &str, method: &str) -> CamelError {
    CamelError::new(
        ErrorKind::RuntimeExchange,
        format!("Method with name: {method} not found on bean: {bean} of type: {bean_type}"),
    )
    .with_subtype(METHOD_NOT_FOUND_EXCEPTION)
    .with_detail(method)
}

/// An `AmbiguousMethodCallException`: no method name was given, and the bean has more than one
/// method (Java chooses by the parameter types, which Rust beans do not declare).
pub fn ambiguous_method_call(methods: &[String]) -> CamelError {
    CamelError::new(
        ErrorKind::RuntimeExchange,
        format!(
            "Ambiguous method invocations possible: [{}]",
            methods.join(", ")
        ),
    )
    .with_subtype(AMBIGUOUS_METHOD_CALL_EXCEPTION)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl ExchangeBean for Echo {
        fn bean_type(&self) -> &str {
            "Echo"
        }
        fn method_names(&self) -> Vec<String> {
            vec!["echo".into()]
        }
        fn invoke(&self, invocation: &mut BeanInvocation<'_>) -> CamelResult<Value> {
            Ok(invocation.parameter(0))
        }
    }

    #[test]
    fn bean_values() {
        let v = bean_value(Echo);
        assert!(exchange_bean(&v).is_some());
        assert!(exchange_bean(&Value::from("x")).is_none());
        let e = method_not_found("Echo", "Echo", "cake");
        assert!(e.is_subtype("MethodNotFoundException"));
        assert_eq!(Some("cake"), e.detail());
        assert!(ambiguous_method_call(&["a".into(), "b".into()])
            .to_string()
            .contains("[a, b]"));
    }
}
