//! Iterator adapters. Port of `org.apache.camel.util.FilterIterator` and
//! `org.apache.camel.util.SkipIterator`.
//!
//! Rust iterators already provide `filter` and `skip`; these adapters keep the extra Camel
//! semantics: `None` elements are skipped (Java `null`), the iterator can be closed early, and
//! the skip is lazy (done on the first access) as the splitter EIP expects.

/// A filtering iterator that skips `None` elements and the elements not accepted by the
/// predicate, and can be closed.
///
/// # Examples
///
/// ```
/// use camel_util::iterators::FilterIterator;
/// let it = FilterIterator::new(vec![Some(1), None, Some(2), Some(3)].into_iter(), |v: &i32| *v != 2);
/// assert_eq!(vec![1, 3], it.collect::<Vec<_>>());
/// ```
pub struct FilterIterator<I, F> {
    it: I,
    filter: F,
    closed: bool,
}

impl<T, I: Iterator<Item = Option<T>>, F: FnMut(&T) -> bool> FilterIterator<I, F> {
    /// Creates the iterator.
    pub fn new(it: I, filter: F) -> Self {
        FilterIterator {
            it,
            filter,
            closed: false,
        }
    }

    /// Closes the iterator: no more elements are returned.
    pub fn close(&mut self) {
        self.closed = true;
    }
}

impl<T, I: Iterator<Item = Option<T>>, F: FnMut(&T) -> bool> Iterator for FilterIterator<I, F> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        while !self.closed {
            match self.it.next() {
                None => return None,
                Some(Some(v)) if (self.filter)(&v) => return Some(v),
                Some(_) => {}
            }
        }
        None
    }
}

/// Skip based iterator which skips the given iterator a number of times (lazily, on the first
/// access), and can be closed. It closes itself when exhausted.
///
/// # Examples
///
/// ```
/// use camel_util::iterators::SkipIterator;
/// assert_eq!(vec![3, 4], SkipIterator::new(1..=4, 2).collect::<Vec<_>>());
/// ```
pub struct SkipIterator<I> {
    it: I,
    skip: usize,
    skipped: bool,
    closed: bool,
}

impl<I: Iterator> SkipIterator<I> {
    /// Creates a new skip iterator.
    ///
    /// # Arguments
    ///
    /// * `it` - the iterator
    /// * `skip` - number of elements to skip (Java required it to be non-negative, which the
    ///   `usize` type enforces)
    pub fn new(it: I, skip: usize) -> Self {
        SkipIterator {
            it,
            skip,
            skipped: false,
            closed: false,
        }
    }

    /// Closes the iterator: no more elements are returned.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Whether the iterator is closed (explicitly or because it was exhausted).
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

impl<I: Iterator> Iterator for SkipIterator<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        if self.closed {
            return None;
        }
        if !self.skipped {
            self.skipped = true;
            for _ in 0..self.skip {
                if self.it.next().is_none() {
                    break;
                }
            }
        }
        let answer = self.it.next();
        if answer.is_none() {
            // auto close
            self.closed = true;
        }
        answer
    }
}

/// Negates a predicate (Java `Predicates.negate`).
///
/// # Examples
///
/// ```
/// use camel_util::iterators::negate;
/// let count = ["A", "", "B"].iter().filter(|s| negate(|s: &&&str| s.is_empty())(s)).count();
/// assert_eq!(2, count);
/// ```
pub fn negate<T: ?Sized>(mut predicate: impl FnMut(&T) -> bool) -> impl FnMut(&T) -> bool {
    move |t| !predicate(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_iterator() {
        let mut it = FilterIterator::new(
            vec![Some("a"), None, Some("b"), Some("c")].into_iter(),
            |_: &&str| true,
        );
        assert_eq!(Some("a"), it.next());
        assert_eq!(Some("b"), it.next());
        it.close();
        assert_eq!(None, it.next());
    }

    #[test]
    fn skip_iterator() {
        let mut it = SkipIterator::new(vec![1, 2, 3].into_iter(), 1);
        assert_eq!(Some(2), it.next());
        assert_eq!(Some(3), it.next());
        assert!(!it.is_closed());
        assert_eq!(None, it.next());
        assert!(it.is_closed());
        let mut it = SkipIterator::new(vec![1, 2].into_iter(), 5);
        assert_eq!(None, it.next());
        let mut it = SkipIterator::new(vec![1, 2].into_iter(), 0);
        it.close();
        assert_eq!(None, it.next());
    }

    // port of PredicatesTest
    #[test]
    fn test_negate() {
        let is_empty = |s: &str| s.is_empty();
        assert!(!is_empty("something"));
        assert!(negate(is_empty)("something"));
        assert!(!negate(is_empty)(""));
    }

    #[test]
    fn negate_predicate() {
        let mut not_empty = negate(|s: &str| s.is_empty());
        assert!(not_empty("x"));
        assert!(!not_empty(""));
    }
}
