//! Holder for a key and value. Port of `org.apache.camel.util.KeyValueHolder`.

use std::fmt;

/// Holder for a key and value.
///
/// Displays as `key -> value` like the Java `toString()`.
///
/// # Examples
///
/// ```
/// use camel_util::key_value_holder::KeyValueHolder;
/// let foo = KeyValueHolder::new("foo", 123);
/// assert_eq!("foo", *foo.key());
/// assert_eq!("foo -> 123", foo.to_string());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct KeyValueHolder<K, V> {
    key: K,
    value: V,
}

impl<K, V> KeyValueHolder<K, V> {
    /// Creates a new holder.
    pub fn new(key: K, value: V) -> Self {
        KeyValueHolder { key, value }
    }

    /// Returns the key.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Returns the value.
    pub fn value(&self) -> &V {
        &self.value
    }

    /// Consumes the holder, returning the key and value.
    pub fn into_parts(self) -> (K, V) {
        (self.key, self.value)
    }
}

impl<K: fmt::Display, V: fmt::Display> fmt::Display for KeyValueHolder<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.key, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash<T: Hash>(t: &T) -> u64 {
        let mut h = DefaultHasher::new();
        t.hash(&mut h);
        h.finish()
    }

    #[test]
    fn test_key_value_holder() {
        let foo = KeyValueHolder::new("foo", 123);
        assert_eq!("foo", *foo.key());
        assert_eq!(123, *foo.value());
    }

    #[test]
    fn test_equals_and_hash_code_on_equal_objects() {
        let foo1 = KeyValueHolder::new("foo", 123);
        let foo2 = KeyValueHolder::new("foo", 123);
        assert_eq!(foo2, foo1);
        assert_eq!(hash(&foo2), hash(&foo1));
    }

    #[test]
    fn test_equals_and_hash_code_on_unequal_objects() {
        let foo = KeyValueHolder::new("foo", 123);
        let bar = KeyValueHolder::new("bar", 678);
        assert_ne!(bar, foo);
        assert_ne!(hash(&bar), hash(&foo));
    }

    #[test]
    fn test_equals_and_hash_code_on_unequal_objects_with_same_keys() {
        let foo1 = KeyValueHolder::new("foo", 123);
        let foo2 = KeyValueHolder::new("foo", 678);
        assert_ne!(foo2, foo1);
        assert_ne!(hash(&foo2), hash(&foo1));
    }

    #[test]
    fn test_equals_and_hash_code_on_unequal_objects_with_same_values() {
        let foo = KeyValueHolder::new("foo", 123);
        let bar = KeyValueHolder::new("bar", 123);
        assert_ne!(bar, foo);
        assert_ne!(hash(&bar), hash(&foo));
    }

    #[test]
    fn test_to_string() {
        assert_eq!("foo -> 123", KeyValueHolder::new("foo", 123).to_string());
    }
}
