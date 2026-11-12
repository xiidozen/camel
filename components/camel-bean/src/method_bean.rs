//! A bean made of closures, one per method: the Rust equivalent of a Java class with public
//! methods for the bean binding.

use camel_api::bean_binding::{method_not_found, shared_bean_value, BeanInvocation, ExchangeBean};
use camel_api::error::CamelResult;
use camel_util::value::Value;
use indexmap::IndexMap;
use std::fmt;
use std::sync::Arc;

type MethodFn = Arc<dyn Fn(&mut BeanInvocation<'_>) -> CamelResult<Value> + Send + Sync>;

/// A bean made of closures, one per method.
///
/// # Examples
///
/// ```
/// use camel_api::bean_binding::ExchangeBean;
/// use camel_bean::MethodBean;
/// use camel_util::value::Value;
///
/// let bean = MethodBean::new("MyFileNameGenerator")
///     .method("generateFilename", |_| Ok(Value::from("abc")))
///     .method("generateId", |_| Ok(Value::Int(123)));
/// assert_eq!(vec!["generateFilename", "generateId"], bean.method_names());
/// ```
#[derive(Clone)]
pub struct MethodBean {
    bean_type: String,
    methods: IndexMap<String, MethodFn>,
}

impl MethodBean {
    /// A bean of the type (the name used in messages, as the Java class name), without
    /// methods.
    pub fn new(bean_type: impl Into<String>) -> Self {
        MethodBean {
            bean_type: bean_type.into(),
            methods: IndexMap::new(),
        }
    }

    /// Adds the method.
    ///
    /// # Arguments
    ///
    /// * `name` - the name of the method
    /// * `f` - the method, called with the invocation (the exchange and the parameters)
    pub fn method<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: Fn(&mut BeanInvocation<'_>) -> CamelResult<Value> + Send + Sync + 'static,
    {
        self.methods.insert(name.into(), Arc::new(f));
        self
    }

    /// The bean as a value, to bind it in the registry.
    pub fn into_value(self) -> Value {
        shared_bean_value(Arc::new(self))
    }
}

impl fmt::Debug for MethodBean {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MethodBean")
            .field("bean_type", &self.bean_type)
            .field("methods", &self.methods.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ExchangeBean for MethodBean {
    fn bean_type(&self) -> &str {
        &self.bean_type
    }

    fn method_names(&self) -> Vec<String> {
        self.methods.keys().cloned().collect()
    }

    fn has_method(&self, name: &str) -> bool {
        self.methods.contains_key(name)
    }

    fn invoke(&self, invocation: &mut BeanInvocation<'_>) -> CamelResult<Value> {
        match self.methods.get(invocation.method()) {
            Some(f) => f(invocation),
            None => Err(method_not_found(
                &self.bean_type,
                &self.bean_type,
                invocation.method(),
            )),
        }
    }
}
