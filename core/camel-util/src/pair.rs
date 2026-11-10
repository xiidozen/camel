//! A pair of values of the same type. Port of `org.apache.camel.util.Pair`.

use std::fmt;

/// Holds a pair of values of the same type (`left` and `right`).
///
/// Displays as `(left, right)` like the Java `toString()`.
///
/// # Examples
///
/// ```
/// use camel_util::pair::Pair;
/// let p = Pair::new(9, 21);
/// assert_eq!(9, *p.left());
/// assert_eq!("(9, 21)", p.to_string());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pair<T> {
    left: T,
    right: T,
}

impl<T> Pair<T> {
    /// Creates a new pair.
    pub fn new(left: T, right: T) -> Self {
        Pair { left, right }
    }

    /// Returns the left value.
    pub fn left(&self) -> &T {
        &self.left
    }

    /// Returns the right value.
    pub fn right(&self) -> &T {
        &self.right
    }
}

impl<T: fmt::Display> fmt::Display for Pair<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.left, self.right)
    }
}
