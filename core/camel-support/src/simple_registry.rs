//! An in-memory bean registry (Java `org.apache.camel.support.SimpleRegistry` /
//! `DefaultRegistry`).

use camel_api::error::CamelResult;
use camel_api::spi::registry::{BeanSupplier, Registry};
use camel_util::value::Value;
use indexmap::IndexMap;
use std::sync::{OnceLock, RwLock};

enum Entry {
    Bean(Value),
    Lazy(BeanSupplier, OnceLock<Value>),
    Prototype(BeanSupplier),
}

/// An in-memory, thread safe bean registry keeping the beans in binding order.
///
/// # Examples
///
/// ```
/// use camel_api::spi::registry::{lookup_bean, Registry};
/// use camel_support::simple_registry::SimpleRegistry;
/// use camel_util::value::Value;
///
/// struct Greeter(&'static str);
/// let registry = SimpleRegistry::new();
/// registry.bind("greeter", Value::object(Greeter("hi"))).unwrap();
/// assert_eq!("hi", lookup_bean::<Greeter>(&registry, "greeter").unwrap().0);
/// assert!(lookup_bean::<String>(&registry, "greeter").is_none());
/// ```
#[derive(Default)]
pub struct SimpleRegistry {
    beans: RwLock<IndexMap<String, Entry>>,
}

impl SimpleRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of bindings.
    pub fn len(&self) -> usize {
        self.beans.read().unwrap().len()
    }

    /// Whether there are no bindings.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Registry for SimpleRegistry {
    fn lookup_by_name(&self, name: &str) -> Option<Value> {
        let beans = self.beans.read().unwrap();
        match beans.get(name)? {
            Entry::Bean(v) => Some(v.clone()),
            Entry::Lazy(s, cell) => Some(cell.get_or_init(|| s()).clone()),
            Entry::Prototype(s) => Some(s()),
        }
    }

    fn names(&self) -> Vec<String> {
        self.beans.read().unwrap().keys().cloned().collect()
    }

    fn bind(&self, id: &str, bean: Value) -> CamelResult<()> {
        self.beans
            .write()
            .unwrap()
            .insert(id.to_string(), Entry::Bean(bean));
        Ok(())
    }

    fn bind_supplier(&self, id: &str, supplier: BeanSupplier) -> CamelResult<()> {
        self.beans
            .write()
            .unwrap()
            .insert(id.to_string(), Entry::Lazy(supplier, OnceLock::new()));
        Ok(())
    }

    fn bind_as_prototype(&self, id: &str, supplier: BeanSupplier) -> CamelResult<()> {
        self.beans
            .write()
            .unwrap()
            .insert(id.to_string(), Entry::Prototype(supplier));
        Ok(())
    }

    fn unbind(&self, id: &str) {
        self.beans.write().unwrap().shift_remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_api::error::ErrorKind;
    use camel_api::spi::registry::{
        find_beans_by_type, mandatory_find_single_by_type, mandatory_lookup_bean,
    };
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[derive(Debug, PartialEq)]
    struct Bean(u32);

    #[test]
    fn bind_and_lookup() {
        let r = SimpleRegistry::new();
        assert!(r.is_empty());
        r.bind("a", Value::object(Bean(1))).unwrap();
        r.bind("b", Value::object(Bean(2))).unwrap();
        r.bind("text", Value::from("hello")).unwrap();
        assert_eq!(3, r.len());
        assert_eq!(Some(Value::from("hello")), r.lookup_by_name("text"));
        assert_eq!(Bean(1), *mandatory_lookup_bean::<Bean>(&r, "a").unwrap());
        let err = mandatory_lookup_bean::<Bean>(&r, "zzz").unwrap_err();
        assert!(err.is(ErrorKind::NoSuchBean));
        let all = find_beans_by_type::<Bean>(&r);
        assert_eq!(
            vec!["a", "b"],
            all.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
        assert!(mandatory_find_single_by_type::<Bean>(&r)
            .unwrap_err()
            .is(ErrorKind::NoSuchBeanType));
        r.unbind("b");
        assert_eq!(Bean(1), *mandatory_find_single_by_type::<Bean>(&r).unwrap());
        assert_eq!(vec!["a", "text"], r.names());
    }

    #[test]
    fn suppliers() {
        let r = SimpleRegistry::new();
        let created = Arc::new(AtomicU32::new(0));
        let c = created.clone();
        r.bind_supplier(
            "lazy",
            Arc::new(move || Value::object(Bean(c.fetch_add(1, Ordering::SeqCst)))),
        )
        .unwrap();
        assert_eq!(0, created.load(Ordering::SeqCst));
        let a = mandatory_lookup_bean::<Bean>(&r, "lazy").unwrap();
        let b = mandatory_lookup_bean::<Bean>(&r, "lazy").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(1, created.load(Ordering::SeqCst));
        let c = created.clone();
        r.bind_as_prototype(
            "proto",
            Arc::new(move || Value::object(Bean(c.fetch_add(1, Ordering::SeqCst)))),
        )
        .unwrap();
        let x = mandatory_lookup_bean::<Bean>(&r, "proto").unwrap();
        let y = mandatory_lookup_bean::<Bean>(&r, "proto").unwrap();
        assert_ne!(*x, *y);
    }

    #[test]
    fn trait_objects() {
        use camel_api::processor::{processor_fn, Processor};
        let r = SimpleRegistry::new();
        let p: Arc<dyn Processor> = Arc::new(processor_fn(|_| Ok(())));
        r.bind("myProcessor", Value::object(p)).unwrap();
        assert!(mandatory_lookup_bean::<Arc<dyn Processor>>(&r, "myProcessor").is_ok());
    }
}
