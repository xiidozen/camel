//! The bean registry (Java `org.apache.camel.spi.Registry` and `BeanRepository`).
//!
//! Beans are stored as [`Value`]s: any Rust value is stored as a [`Value::Object`] and looked
//! up again by its type with [`lookup_bean`] / [`find_beans_by_type`]. Trait objects are
//! stored wrapped in an `Arc` (for example an `Arc<dyn Processor>`) and looked up by that type.

use crate::error::{CamelError, CamelResult};
use camel_util::value::Value;
use std::any::Any;
use std::sync::Arc;

/// A supplier of bean instances (for lazily created or prototype beans).
pub type BeanSupplier = Arc<dyn Fn() -> Value + Send + Sync>;

/// A repository of beans that can be looked up by name or type.
pub trait Registry: Send + Sync {
    /// Looks up the bean with the name.
    fn lookup_by_name(&self, name: &str) -> Option<Value>;

    /// The names of all beans, in binding order.
    fn names(&self) -> Vec<String>;

    /// Binds the bean with the id (replacing an existing binding).
    ///
    /// # Errors
    ///
    /// Returns an error if the registry is read-only.
    fn bind(&self, id: &str, bean: Value) -> CamelResult<()>;

    /// Binds a bean created lazily by the supplier on first lookup (a singleton).
    ///
    /// # Errors
    ///
    /// Returns an error if the registry is read-only.
    fn bind_supplier(&self, id: &str, supplier: BeanSupplier) -> CamelResult<()>;

    /// Binds a bean created by the supplier on every lookup (a prototype).
    ///
    /// # Errors
    ///
    /// Returns an error if the registry is read-only.
    fn bind_as_prototype(&self, id: &str, supplier: BeanSupplier) -> CamelResult<()>;

    /// Removes the binding.
    fn unbind(&self, id: &str);
}

/// Looks up the bean with the name and type.
///
/// # Returns
///
/// The bean, or `None` if there is no bean with the name or it is of another type.
pub fn lookup_bean<T: Any + Send + Sync>(registry: &dyn Registry, name: &str) -> Option<Arc<T>> {
    registry
        .lookup_by_name(name)
        .and_then(|v| v.as_object().and_then(|o| o.downcast_arc::<T>()))
}

/// Looks up the bean with the name and type, failing if there is none (Java
/// `CamelContextHelper.mandatoryLookup`).
///
/// # Errors
///
/// Returns a `NoSuchBeanException` if there is no such bean.
pub fn mandatory_lookup_bean<T: Any + Send + Sync>(
    registry: &dyn Registry,
    name: &str,
) -> CamelResult<Arc<T>> {
    lookup_bean(registry, name)
        .ok_or_else(|| CamelError::no_such_bean_of_type(Some(name), std::any::type_name::<T>()))
}

/// Finds all the beans of the type, with their names (Java `findByTypeWithName`).
pub fn find_beans_by_type<T: Any + Send + Sync>(registry: &dyn Registry) -> Vec<(String, Arc<T>)> {
    registry
        .names()
        .into_iter()
        .filter_map(|n| lookup_bean::<T>(registry, &n).map(|b| (n, b)))
        .collect()
}

/// Finds the single bean of the type (Java `mandatoryFindSingleByType`).
///
/// # Errors
///
/// Returns a `NoSuchBeanTypeException` if there is not exactly one such bean.
pub fn mandatory_find_single_by_type<T: Any + Send + Sync>(
    registry: &dyn Registry,
) -> CamelResult<Arc<T>> {
    let mut found = find_beans_by_type::<T>(registry);
    if found.len() == 1 {
        return Ok(found.pop().unwrap().1);
    }
    let type_name = std::any::type_name::<T>();
    let msg = if found.is_empty() {
        format!("No bean could be found in the registry of type: {type_name}")
    } else {
        format!(
            "Found {} beans of type: {type_name}. Only one bean expected.",
            found.len()
        )
    };
    Err(CamelError::new(crate::error::ErrorKind::NoSuchBeanType, msg).with_detail(type_name))
}
