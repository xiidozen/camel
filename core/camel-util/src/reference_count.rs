//! A reference counter with callbacks. Port of `org.apache.camel.util.ReferenceCount`.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};

type Callback = Box<dyn Fn() + Send + Sync>;

/// Error returned when retaining or releasing a [`ReferenceCount`] in an invalid state (the
/// Java version threw `IllegalStateException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalStateError(pub String);

impl fmt::Display for IllegalStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IllegalStateError {}

/// A thread safe reference counter that invokes `on_first` on the first retain and
/// `on_release` when the count drops back to zero.
///
/// # Examples
///
/// ```
/// use camel_util::reference_count::ReferenceCount;
/// use std::sync::{Arc, atomic::{AtomicI32, Ordering}};
///
/// let cnt = Arc::new(AtomicI32::new(0));
/// let (c1, c2) = (cnt.clone(), cnt.clone());
/// let r = ReferenceCount::on(move || { c1.fetch_add(1, Ordering::SeqCst); },
///                            move || { c2.fetch_sub(1, Ordering::SeqCst); });
/// r.retain().unwrap();
/// assert_eq!(1, cnt.load(Ordering::SeqCst));
/// r.release().unwrap();
/// assert_eq!(0, cnt.load(Ordering::SeqCst));
/// assert!(r.release().is_err());
/// ```
pub struct ReferenceCount {
    count: AtomicI64,
    on_first: Callback,
    on_release: Callback,
}

impl fmt::Debug for ReferenceCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReferenceCount")
            .field("count", &self.get())
            .finish()
    }
}

impl ReferenceCount {
    /// Creates a reference count invoking `on_first` on the first retain and `on_release` when
    /// the count reaches zero.
    pub fn on(
        on_first: impl Fn() + Send + Sync + 'static,
        on_release: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        ReferenceCount {
            count: AtomicI64::new(0),
            on_first: Box::new(on_first),
            on_release: Box::new(on_release),
        }
    }

    /// Creates a reference count invoking `on_release` when the count reaches zero.
    pub fn on_release(on_release: impl Fn() + Send + Sync + 'static) -> Self {
        Self::on(|| {}, on_release)
    }

    /// Returns the reference count.
    pub fn get(&self) -> i64 {
        self.count.load(Ordering::SeqCst)
    }

    /// Increases the reference count, invoking `on_first` on the first increment.
    ///
    /// # Errors
    ///
    /// Returns an error if the counter is in a released (negative) state.
    pub fn retain(&self) -> Result<(), IllegalStateError> {
        loop {
            let v = self.count.load(Ordering::SeqCst);
            if v < 0 {
                return Err(IllegalStateError("Released".into()));
            }
            if self
                .count
                .compare_exchange(v, v + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if v == 0 {
                    (self.on_first)();
                }
                return Ok(());
            }
        }
    }

    /// Decreases the reference count and invokes `on_release` if the reference count reaches
    /// `0`.
    ///
    /// # Errors
    ///
    /// Returns an error if the reference count is already released (zero).
    pub fn release(&self) -> Result<(), IllegalStateError> {
        loop {
            let v = self.count.load(Ordering::SeqCst);
            if v <= 0 {
                return Err(IllegalStateError("ReferenceCount already released".into()));
            }
            if self
                .count
                .compare_exchange(v, v - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                if v == 1 {
                    (self.on_release)();
                }
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicI32;
    use std::sync::Arc;

    #[test]
    fn test_reference_count() {
        let cnt = Arc::new(AtomicI32::new(0));
        let (c1, c2) = (cnt.clone(), cnt.clone());
        let r = ReferenceCount::on(
            move || {
                c1.fetch_add(1, Ordering::SeqCst);
            },
            move || {
                c2.fetch_sub(1, Ordering::SeqCst);
            },
        );
        r.retain().unwrap();
        assert_eq!(1, r.get());
        assert_eq!(1, cnt.load(Ordering::SeqCst));
        r.retain().unwrap();
        assert_eq!(2, r.get());
        assert_eq!(1, cnt.load(Ordering::SeqCst));
        r.release().unwrap();
        assert_eq!(1, r.get());
        assert_eq!(1, cnt.load(Ordering::SeqCst));
        r.release().unwrap();
        assert_eq!(0, r.get());
        assert_eq!(0, cnt.load(Ordering::SeqCst));
        assert_eq!(
            Err(IllegalStateError("ReferenceCount already released".into())),
            r.release()
        );
    }

    #[test]
    fn test_on_release_only() {
        let cnt = Arc::new(AtomicI32::new(0));
        let c = cnt.clone();
        let r = ReferenceCount::on_release(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        r.retain().unwrap();
        r.release().unwrap();
        assert_eq!(1, cnt.load(Ordering::SeqCst));
        assert!(format!("{r:?}").contains("count: 0"));
    }
}
