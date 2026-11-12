//! The processor calling a bean method with the exchange (Java `BeanProcessor` and
//! `AbstractBeanProcessor`), used by the bean EIP and the bean component.
//!
//! The result of the method becomes the message body, unless the method returns no value
//! ([`ExchangeBean::is_void`](camel_api::bean_binding::ExchangeBean::is_void)). A
//! [`SharedProcessor`] bound as the bean (see [`processor_value`]) is called as is (Java uses a
//! bean that is a `Processor` directly).

use camel_api::bean_binding::{
    exchange_bean, method_not_found, BeanInvocation, BeanProcessorFactory, BeanReference,
};
use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor, SharedProcessor};
use camel_core_languages::simple::ognl::{argument, invoke_ognl, method_and_args};
use camel_util::value::{ObjectRef, Value};
use std::sync::Arc;

/// The processor as a value, to bind it in the registry and call it with the bean EIP or
/// component.
pub fn processor_value(processor: SharedProcessor) -> Value {
    Value::Object(ObjectRef::new(processor))
}

/// Calls a bean method with the exchange, setting the result as the message body.
///
/// # Examples
///
/// ```
/// use camel_bean::{BeanProcessor, MethodBean};
/// use camel_core::prelude::*;
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// let context = camel_core::default_context();
/// let bean = MethodBean::new("Greeter").method("hello", |inv| {
///     Ok(Value::from(format!("Hello {}", inv.parameter(0))))
/// });
/// let processor = BeanProcessor::new(bean.into_value().into(), None);
/// let mut exchange = Exchange::new(context.as_shared());
/// exchange.message_mut().set_body("World");
/// processor.process(&mut exchange).await.unwrap();
/// assert_eq!(&Value::from("Hello World"), exchange.message().body());
/// # });
/// ```
pub struct BeanProcessor {
    bean: BeanReference,
    method: Option<String>,
}

impl BeanProcessor {
    /// A processor calling the method (a method call such as `hello('World')`; `None` for the
    /// only method of the bean) of the bean.
    pub fn new(bean: BeanReference, method: Option<&str>) -> Self {
        BeanProcessor {
            bean,
            method: method.map(str::to_string),
        }
    }

    /// The bean.
    pub fn bean_reference(&self) -> &BeanReference {
        &self.bean
    }

    /// The method, if any.
    pub fn method(&self) -> Option<&str> {
        self.method.as_deref()
    }

    /// Validates that the bean is in the registry and has the method.
    ///
    /// # Errors
    ///
    /// A `NoSuchBeanException`, or a `MethodNotFoundException`.
    pub fn validate(&self, context: &Arc<dyn CamelContext>) -> CamelResult<()> {
        let bean = self.bean(context)?;
        if let (Some(b), Some(m)) = (exchange_bean(&bean), &self.method) {
            let (name, _) = method_and_args(m);
            if !b.has_method(&name) {
                return Err(method_not_found(&bean.to_string(), b.bean_type(), &name));
            }
        }
        Ok(())
    }

    fn bean(&self, context: &Arc<dyn CamelContext>) -> CamelResult<Value> {
        match &self.bean {
            BeanReference::Bean(v) => Ok(v.clone()),
            BeanReference::Name(name) => context
                .registry()
                .and_then(|r| r.lookup_by_name(name))
                .ok_or_else(|| CamelError::no_such_bean(name)),
        }
    }
}

/// The error of a bean with several methods called without a method name (Java
/// `AbstractBeanProcessor`, which can also choose by the parameter types and `@Handler`).
fn no_method_invocation(bean: &Value, methods: &[String]) -> CamelError {
    let mut names = methods.to_vec();
    names.sort();
    names.dedup();
    let hint = if names.is_empty() {
        " (the bean has no public method to call)".to_string()
    } else {
        format!(
            " (the bean has the methods {}: name the one to call with method: <name> on the bean step)",
            names.join(", ")
        )
    };
    CamelError::illegal_state(format!(
        "No method invocation could be created, no matching method could be found on: {bean}{hint}"
    ))
}

/// Calls the method of the bean with the exchange: the result, `None` for a method without
/// result.
///
/// # Errors
///
/// A missing method, or the error of the method.
pub fn invoke_bean(
    bean: &Value,
    method: Option<&str>,
    exchange: &mut Exchange,
) -> CamelResult<Option<Value>> {
    let Some(b) = exchange_bean(bean) else {
        return match method {
            Some(m) => invoke_ognl(bean, m, exchange).map(Some),
            None => Err(no_method_invocation(bean, &[])),
        };
    };
    let (name, args) = match method {
        None => match b.method_names().as_slice() {
            [only] => (only.clone(), None),
            names => return Err(no_method_invocation(bean, names)),
        },
        Some(m) => {
            let (name, texts) = method_and_args(m);
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
            (name, args)
        }
    };
    if !b.has_method(&name) {
        return Err(method_not_found(&bean.to_string(), b.bean_type(), &name));
    }
    let void = b.is_void(&name);
    let result = b.invoke(&mut BeanInvocation::new(exchange, &name, args))?;
    Ok(if void { None } else { Some(result) })
}

impl Processor for BeanProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let bean = self.bean(exchange.context())?;
            if let Value::Object(o) = &bean {
                if let Some(p) = o.downcast_ref::<SharedProcessor>() {
                    let p = p.clone();
                    return p.process(exchange).await;
                }
            }
            if let Some(result) = invoke_bean(&bean, self.method.as_deref(), exchange)? {
                exchange.message_mut().set_body(result);
            }
            Ok(())
        })
    }
}

/// The [`BeanProcessorFactory`] of the bean EIP (Java `DefaultBeanProcessorFactory`).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultBeanProcessorFactory;

impl BeanProcessorFactory for DefaultBeanProcessorFactory {
    /// The processor, validated: the bean must be in the registry, and have the method (Java
    /// fails to create the route otherwise).
    fn create_bean_processor(
        &self,
        context: &Arc<dyn CamelContext>,
        bean: &BeanReference,
        method: Option<&str>,
    ) -> CamelResult<SharedProcessor> {
        let processor = BeanProcessor::new(bean.clone(), method);
        processor.validate(context)?;
        Ok(Arc::new(processor))
    }
}
