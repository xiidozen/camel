//! Functional helpers. Port of the `org.apache.camel.util.function` package.
//!
//! The Java `Throwing*` functional interfaces (`ThrowingFunction`, `ThrowingConsumer`, ...),
//! `TriConsumer` and `VoidFunction` are not needed in Rust: closures returning a `Result` play
//! that role. `ThrowingHelper.wrapAs*` (turning checked exceptions into runtime exceptions) has
//! no equivalent either: use `?` or `expect`. `Predicates` is in [`crate::iterators::negate`].

use crate::object_helper::is_not_empty;
use crate::value::Value;
use std::sync::OnceLock;

/// A supplier which computes its value on the first call and returns that value on subsequent
/// calls (Java `Suppliers.memorize`). Thread safe.
///
/// # Examples
///
/// ```
/// use camel_util::function::Memorized;
/// use std::sync::atomic::{AtomicU32, Ordering};
/// let calls = AtomicU32::new(0);
/// let m = Memorized::new(|| { calls.fetch_add(1, Ordering::SeqCst); 42 });
/// assert_eq!(42, *m.get());
/// assert_eq!(42, *m.get());
/// assert_eq!(1, calls.load(Ordering::SeqCst));
/// ```
pub struct Memorized<T, F> {
    value: OnceLock<T>,
    supplier: F,
}

impl<T, F: Fn() -> T> Memorized<T, F> {
    /// Creates the memorizing supplier.
    pub fn new(supplier: F) -> Self {
        Memorized {
            value: OnceLock::new(),
            supplier,
        }
    }

    /// Returns the value, computing it on the first call.
    pub fn get(&self) -> &T {
        self.value.get_or_init(&self.supplier)
    }
}

/// A supplier which computes its value with a fallible supplier on the first successful call,
/// passing errors to the consumer (Java `Suppliers.memorize(ThrowingSupplier, Consumer)`).
pub struct MemorizedFallible<T, E, F, C> {
    value: OnceLock<T>,
    supplier: F,
    consumer: C,
    _e: std::marker::PhantomData<fn() -> E>,
}

impl<T, E, F: Fn() -> Result<T, E>, C: Fn(E)> MemorizedFallible<T, E, F, C> {
    /// Creates the memorizing supplier.
    pub fn new(supplier: F, consumer: C) -> Self {
        MemorizedFallible {
            value: OnceLock::new(),
            supplier,
            consumer,
            _e: std::marker::PhantomData,
        }
    }

    /// Returns the value, or `None` if the supplier failed (the error is given to the
    /// consumer, and the next call tries again).
    pub fn get(&self) -> Option<&T> {
        if let Some(v) = self.value.get() {
            return Some(v);
        }
        match (self.supplier)() {
            Ok(v) => Some(self.value.get_or_init(|| v)),
            Err(e) => {
                (self.consumer)(e);
                None
            }
        }
    }
}

/// Returns the first non-`None` value provided by the suppliers (Java
/// `Suppliers.firstNotNull`).
///
/// # Errors
///
/// Returns the error of the first supplier that fails.
pub fn first_not_null<T, E>(
    suppliers: &[&dyn Fn() -> Result<Option<T>, E>],
) -> Result<Option<T>, E> {
    for s in suppliers {
        if let Some(v) = s()? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// Returns the first value provided by the suppliers that matches the predicate (Java
/// `Suppliers.firstMatching`).
///
/// # Errors
///
/// Returns the error of the first supplier that fails.
pub fn first_matching<T, E>(
    predicate: impl Fn(&T) -> bool,
    suppliers: &[&dyn Fn() -> Result<T, E>],
) -> Result<Option<T>, E> {
    for s in suppliers {
        let v = s()?;
        if predicate(&v) {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// Binds the first argument of a two-argument function (Java `Bindings.bind`).
///
/// # Examples
///
/// ```
/// use camel_util::function::bind;
/// let add = |a: i32, b: i32| a + b;
/// let add5 = bind(5, add);
/// assert_eq!(7, add5(2));
/// ```
pub fn bind<T1: Clone, T2, R>(v1: T1, f: impl Fn(T1, T2) -> R) -> impl Fn(T2) -> R {
    move |v2| f(v1.clone(), v2)
}

/// Applies the function to the value if it is not empty (see
/// [`crate::object_helper::is_empty`]) (Java `ThrowingHelper.applyIfNotEmpty`).
///
/// # Errors
///
/// Returns the error of the function.
pub fn apply_if_not_empty<R, E>(
    value: &Value,
    f: impl FnOnce(&Value) -> Result<R, E>,
) -> Result<Option<R>, E> {
    if is_not_empty(value) {
        f(value).map(Some)
    } else {
        Ok(None)
    }
}

/// Applies the function to the value if it is not empty, otherwise returns the value of
/// `or_else`.
///
/// # Errors
///
/// Returns the error of the function.
pub fn apply_if_not_empty_or_else<R, E>(
    value: &Value,
    f: impl FnOnce(&Value) -> Result<R, E>,
    or_else: impl FnOnce() -> R,
) -> Result<R, E> {
    if is_not_empty(value) {
        f(value)
    } else {
        Ok(or_else())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn memorized() {
        let calls = Cell::new(0);
        let m = Memorized::new(|| {
            calls.set(calls.get() + 1);
            "x".to_string()
        });
        assert_eq!("x", m.get());
        assert_eq!("x", m.get());
        assert_eq!(1, calls.get());

        let attempts = Cell::new(0);
        let errors = Cell::new(0);
        let f = MemorizedFallible::new(
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() < 2 {
                    Err("boom")
                } else {
                    Ok(7)
                }
            },
            |_e| errors.set(errors.get() + 1),
        );
        assert_eq!(None, f.get());
        assert_eq!(Some(&7), f.get());
        assert_eq!(Some(&7), f.get());
        assert_eq!(2, attempts.get());
        assert_eq!(1, errors.get());
    }

    #[test]
    fn suppliers() {
        let none = || Ok::<Option<i32>, ()>(None);
        let two = || Ok::<Option<i32>, ()>(Some(2));
        assert_eq!(Ok(Some(2)), first_not_null(&[&none, &two]));
        assert_eq!(Ok(None), first_not_null(&[&none]));
        let one = || Ok::<i32, ()>(1);
        let three = || Ok::<i32, ()>(3);
        assert_eq!(Ok(Some(3)), first_matching(|v| *v > 2, &[&one, &three]));
        assert_eq!(Ok(None), first_matching(|v| *v > 5, &[&one, &three]));
        let fail = || Err::<i32, &str>("e");
        assert_eq!(Err("e"), first_matching(|_| true, &[&fail]));
    }

    #[test]
    fn apply() {
        assert_eq!(
            Ok::<_, ()>(Some(1)),
            apply_if_not_empty(&Value::from("a"), |_| Ok(1))
        );
        assert_eq!(
            Ok::<_, ()>(None::<i32>),
            apply_if_not_empty(&Value::from(" "), |_| Ok(1))
        );
        assert_eq!(
            Ok::<_, ()>(9),
            apply_if_not_empty_or_else(&Value::Null, |_| Ok(1), || 9)
        );
        assert_eq!(
            Ok::<_, ()>(1),
            apply_if_not_empty_or_else(&Value::from(1), |_| Ok(1), || 9)
        );
    }
}
