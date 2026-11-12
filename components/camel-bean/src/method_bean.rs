//! A bean made of closures, one per method: the Rust equivalent of a Java class with public
//! methods for the bean binding.

use camel_api::bean_binding::{method_not_found, shared_bean_value, BeanInvocation, ExchangeBean};
use camel_api::error::CamelResult;
use camel_util::value::Value;
use indexmap::IndexMap;
use std::fmt;
use std::sync::Arc;

type MethodFn = Arc<dyn Fn(&mut BeanInvocation<'_>) -> CamelResult<Value> + Send + Sync>;

/// A method: the closure, and whether it returns no value (Java `void`).
#[derive(Clone)]
struct Method {
    f: MethodFn,
    void: bool,
}

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
///     .method("generateId", |_| Ok(Value::Int(123)))
///     .void_method("audit", |inv| {
///         inv.exchange_mut().set_property("audited", true);
///         Ok(())
///     });
/// assert_eq!(vec!["generateFilename", "generateId", "audit"], bean.method_names());
/// assert!(bean.is_void("audit"));
/// ```
#[derive(Clone)]
pub struct MethodBean {
    bean_type: String,
    methods: IndexMap<String, Method>,
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
    /// * `f` - the method, called with the invocation (the exchange and the parameters); its
    ///   result is the new message body when called by the bean EIP or component
    pub fn method<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: Fn(&mut BeanInvocation<'_>) -> CamelResult<Value> + Send + Sync + 'static,
    {
        self.methods.insert(
            name.into(),
            Method {
                f: Arc::new(f),
                void: false,
            },
        );
        self
    }

    /// Adds a method that returns no value (a Java `void` method): the bean EIP and component
    /// keep the message body.
    pub fn void_method<F>(mut self, name: impl Into<String>, f: F) -> Self
    where
        F: Fn(&mut BeanInvocation<'_>) -> CamelResult<()> + Send + Sync + 'static,
    {
        self.methods.insert(
            name.into(),
            Method {
                f: Arc::new(move |inv| f(inv).map(|_| Value::Null)),
                void: true,
            },
        );
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

    fn is_void(&self, method: &str) -> bool {
        self.methods.get(method).is_some_and(|m| m.void)
    }

    fn invoke(&self, invocation: &mut BeanInvocation<'_>) -> CamelResult<Value> {
        match self.methods.get(invocation.method()) {
            Some(m) => (m.f)(invocation),
            None => Err(method_not_found(
                &self.bean_type,
                &self.bean_type,
                invocation.method(),
            )),
        }
    }
}
