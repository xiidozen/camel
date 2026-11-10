//! A map that uses case insensitive keys, but preserves the original key cases.
//!
//! Port of `org.apache.camel.util.CaseInsensitiveMap`, the map used for message headers.

use crate::lang::{char_to_lower, char_to_upper};
use crate::value::Value;
use indexmap::{Equivalent, IndexMap};
use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, RwLock};

/// Folds a character for case-insensitive comparison: ASCII fast path, otherwise the Unicode
/// two-step fold `lower(upper(c))` used by Java's `String.equalsIgnoreCase`.
#[inline]
fn fold(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        char_to_lower(char_to_upper(c))
    }
}

fn hash_folded<H: Hasher>(s: &str, state: &mut H) {
    let mut n = 0usize;
    for c in s.chars() {
        state.write_u32(fold(c) as u32);
        n += 1;
    }
    state.write_usize(n);
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    let mut x = a.chars();
    let mut y = b.chars();
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(c1), Some(c2)) => {
                if c1 != c2 && fold(c1) != fold(c2) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// A map key that keeps its original case but hashes and compares case-insensitively.
#[derive(Clone)]
struct CiKey(Arc<str>);

impl Hash for CiKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_folded(&self.0, state)
    }
}

impl PartialEq for CiKey {
    fn eq(&self, other: &Self) -> bool {
        eq_ignore_case(&self.0, &other.0)
    }
}

impl Eq for CiKey {}

/// A borrowed lookup key (no allocation for `get`, `contains_key` and `remove`).
#[repr(transparent)]
struct CiStr(str);

impl CiStr {
    fn new(s: &str) -> &CiStr {
        // SAFETY: CiStr is repr(transparent) over str
        unsafe { &*(s as *const str as *const CiStr) }
    }
}

impl Hash for CiStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_folded(&self.0, state)
    }
}

impl Equivalent<CiKey> for CiStr {
    fn equivalent(&self, key: &CiKey) -> bool {
        eq_ignore_case(&self.0, &key.0)
    }
}

fn known_keys() -> &'static RwLock<Vec<Arc<str>>> {
    static KNOWN: OnceLock<RwLock<Vec<Arc<str>>>> = OnceLock::new();
    KNOWN.get_or_init(|| RwLock::new(Vec::new()))
}

/// Registers a set of well-known header key strings for deduplication.
///
/// When a key passed to [`CaseInsensitiveMap::insert`] is equal to one of these strings, the
/// shared canonical [`Arc<str>`] from this set is stored instead of a new allocation, reducing
/// memory when many map instances carry the same headers. A key that only differs in case is
/// stored as given, as the map preserves the original key cases.
///
/// This function is intended to be called once during framework startup; calling it again
/// replaces the registered keys.
pub fn register_known_keys<I, S>(keys: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut keys: Vec<Arc<str>> = keys.into_iter().map(|k| Arc::from(k.as_ref())).collect();
    keys.sort();
    keys.dedup();
    *known_keys().write().unwrap() = keys;
}

fn deduplicate_key(key: &str) -> Arc<str> {
    let known = known_keys().read().unwrap();
    match known.binary_search_by(|k| k.as_ref().cmp(key)) {
        // only an equal key is replaced, so the case of the caller's key is kept
        Ok(i) => known[i].clone(),
        Err(_) => Arc::from(key),
    }
}

/// A map that uses case insensitive keys, but preserves the original key cases.
///
/// Lookups (`get`, `contains_key`, `remove`) are O(1) and do not allocate. Entries are kept in
/// insertion order. When a key is inserted again with a different case, only the value is
/// replaced: the case of the first insertion is kept.
///
/// This map is **not** designed to be shared between threads without synchronization, as
/// concurrent access is not performed by the Camel routing engine.
///
/// # Examples
///
/// ```
/// use camel_util::case_insensitive_map::CaseInsensitiveMap;
/// use camel_util::value::Value;
///
/// let mut headers: CaseInsensitiveMap = CaseInsensitiveMap::new();
/// headers.insert("Content-Type", Value::from("text/plain"));
/// assert_eq!(Some(&Value::from("text/plain")), headers.get("content-type"));
/// headers.insert("CONTENT-TYPE", Value::from("application/json"));
/// assert_eq!(1, headers.len());
/// assert_eq!(vec!["Content-Type"], headers.keys().collect::<Vec<_>>());
/// ```
#[derive(Clone)]
pub struct CaseInsensitiveMap<V = Value> {
    inner: IndexMap<CiKey, V>,
}

impl<V> Default for CaseInsensitiveMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: fmt::Debug> fmt::Debug for CaseInsensitiveMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V: fmt::Display> fmt::Display for CaseInsensitiveMap<V> {
    /// Formats like Java `AbstractMap.toString()`: `{k=v, k2=v2}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (i, (k, v)) in self.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{k}={v}")?;
        }
        f.write_str("}")
    }
}

impl<V: PartialEq> PartialEq for CaseInsensitiveMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl<V> CaseInsensitiveMap<V> {
    /// Creates an empty map.
    pub fn new() -> Self {
        CaseInsensitiveMap {
            inner: IndexMap::new(),
        }
    }

    /// Creates an empty map with room for at least `capacity` entries.
    pub fn with_capacity(capacity: usize) -> Self {
        CaseInsensitiveMap {
            inner: IndexMap::with_capacity(capacity),
        }
    }

    /// Returns the value for the key (looked up ignoring case).
    pub fn get(&self, key: &str) -> Option<&V> {
        self.inner.get(CiStr::new(key))
    }

    /// Returns the mutable value for the key (looked up ignoring case).
    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.inner.get_mut(CiStr::new(key))
    }

    /// Returns the stored key (with its original case) and value for the key.
    pub fn get_key_value(&self, key: &str) -> Option<(&str, &V)> {
        self.inner
            .get_key_value(CiStr::new(key))
            .map(|(k, v)| (k.0.as_ref(), v))
    }

    /// Whether the map contains the key (ignoring case).
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(CiStr::new(key))
    }

    /// Inserts the value. If the key already exists (ignoring case) only the value is replaced
    /// and the original key case is kept.
    ///
    /// # Returns
    ///
    /// The previous value, if any.
    pub fn insert(&mut self, key: impl AsRef<str>, value: V) -> Option<V> {
        let key = key.as_ref();
        if let Some(existing) = self.inner.get_mut(CiStr::new(key)) {
            return Some(std::mem::replace(existing, value));
        }
        self.inner.insert(CiKey(deduplicate_key(key)), value);
        None
    }

    /// Removes the key (ignoring case), keeping the order of the remaining entries.
    ///
    /// # Returns
    ///
    /// The removed value, if any.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.inner.shift_remove(CiStr::new(key))
    }

    /// Removes the key (ignoring case), returning the stored key and value.
    pub fn remove_entry(&mut self, key: &str) -> Option<(Arc<str>, V)> {
        self.inner
            .shift_remove_entry(CiStr::new(key))
            .map(|(k, v)| (k.0, v))
    }

    /// Removes all the given keys (ignoring case).
    ///
    /// # Returns
    ///
    /// `true` if any entry was removed (Java `keySet().removeAll`).
    pub fn remove_all<I, S>(&mut self, keys: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut changed = false;
        for k in keys {
            changed |= self.remove(k.as_ref()).is_some();
        }
        changed
    }

    /// Keeps only the given keys (ignoring case).
    ///
    /// # Returns
    ///
    /// `true` if any entry was removed (Java `keySet().retainAll`).
    pub fn retain_keys<I, S>(&mut self, keys: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let keep: CaseInsensitiveMap<()> = keys
            .into_iter()
            .map(|k| (k.as_ref().to_string(), ()))
            .collect();
        let before = self.len();
        self.inner.retain(|k, _| keep.contains_key(&k.0));
        before != self.len()
    }

    /// Retains only the entries for which the predicate returns `true` (the Java iterator
    /// `remove` and `removeIf`).
    pub fn retain(&mut self, mut f: impl FnMut(&str, &mut V) -> bool) {
        self.inner.retain(|k, v| f(&k.0, v));
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Iterates over the entries in insertion order (keys with their original case).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> + '_ {
        self.inner.iter().map(|(k, v)| (k.0.as_ref(), v))
    }

    /// Iterates over the entries with mutable values (the Java `Entry.setValue`).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut V)> + '_ {
        self.inner.iter_mut().map(|(k, v)| (k.0.as_ref(), v))
    }

    /// Iterates over the keys in insertion order (with their original case).
    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.inner.keys().map(|k| k.0.as_ref())
    }

    /// The shared key instances (to check deduplication of known keys).
    pub fn key_arcs(&self) -> impl Iterator<Item = &Arc<str>> + '_ {
        self.inner.keys().map(|k| &k.0)
    }

    /// Iterates over the values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.inner.values()
    }

    /// Iterates over the mutable values in insertion order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.inner.values_mut()
    }

    /// Inserts all the entries.
    pub fn extend_from<K: AsRef<str>, I: IntoIterator<Item = (K, V)>>(&mut self, entries: I) {
        for (k, v) in entries {
            self.insert(k, v);
        }
    }
}

impl<V: PartialEq> CaseInsensitiveMap<V> {
    /// Whether the map contains the given value.
    pub fn contains_value(&self, value: &V) -> bool {
        self.inner.values().any(|v| v == value)
    }

    /// Removes the entry only if the key (ignoring case) is mapped to the given value (Java
    /// `entrySet().remove(entry)`).
    ///
    /// # Returns
    ///
    /// `true` if the entry was removed.
    pub fn remove_entry_if_value(&mut self, key: &str, value: &V) -> bool {
        if self.get(key) == Some(value) {
            self.remove(key);
            true
        } else {
            false
        }
    }
}

impl<K: AsRef<str>, V> FromIterator<(K, V)> for CaseInsensitiveMap<V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut map = CaseInsensitiveMap::new();
        map.extend_from(iter);
        map
    }
}

impl<K: AsRef<str>, V> Extend<(K, V)> for CaseInsensitiveMap<V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        self.extend_from(iter)
    }
}

/// An owning iterator over the entries of a [`CaseInsensitiveMap`] (keys with their original
/// case).
pub struct IntoIter<V>(indexmap::map::IntoIter<CiKey, V>);

impl<V> Iterator for IntoIter<V> {
    type Item = (String, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(k, v)| (k.0.to_string(), v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<V> IntoIterator for CaseInsensitiveMap<V> {
    type Item = (String, V);
    type IntoIter = IntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.inner.into_iter())
    }
}

impl<'a, V> IntoIterator for &'a CaseInsensitiveMap<V> {
    type Item = (&'a str, &'a V);
    type IntoIter = Box<dyn Iterator<Item = (&'a str, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<V> std::ops::Index<&str> for CaseInsensitiveMap<V> {
    type Output = V;

    /// Returns the value for the key.
    ///
    /// # Panics
    ///
    /// Panics if the key is not present.
    fn index(&self, key: &str) -> &V {
        self.get(key)
            .unwrap_or_else(|| panic!("key not found: {key}"))
    }
}

// allow `HashMap<String, V>`-style copies that keep the original key case
impl<V: Clone> From<&CaseInsensitiveMap<V>> for std::collections::HashMap<String, V> {
    fn from(map: &CaseInsensitiveMap<V>) -> Self {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }
}

impl Borrow<str> for CiKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    type Map = CaseInsensitiveMap<Value>;

    fn v(s: &str) -> Value {
        Value::from(s)
    }

    /// The known keys are global; the tests that change them run one at a time.
    static KNOWN_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_lookup_case_agnostic() {
        let mut map = Map::new();
        assert!(map.get("foo").is_none());
        map.insert("foo", v("cheese"));
        for k in ["foo", "Foo", "FOO"] {
            assert_eq!(Some(&v("cheese")), map.get(k));
        }
    }

    #[test]
    fn test_lookup_case_agnostic_add_header() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        for k in ["foo", "Foo", "FOO"] {
            assert_eq!(Some(&v("cheese")), map.get(k));
        }
        assert!(map.get("unknown").is_none());
        map.insert("bar", v("beer"));
        for k in ["bar", "Bar", "BAR"] {
            assert_eq!(Some(&v("beer")), map.get(k));
        }
        assert!(map.get("unknown").is_none());
    }

    #[test]
    fn test_lookup_case_agnostic_add_header_remove_header() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        map.insert("bar", v("beer"));
        assert_eq!(Some(&v("beer")), map.get("BAR"));
        map.remove("bar");
        assert!(map.get("bar").is_none());
        assert!(map.get("unknown").is_none());
    }

    #[test]
    fn test_set_with_different_case() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        map.insert("Foo", v("bar"));
        for k in ["FOO", "foo", "Foo"] {
            assert_eq!(Some(&v("bar")), map.get(k));
        }
    }

    #[test]
    fn test_remove_with_different_case() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        map.insert("Foo", v("bar"));
        map.remove("FOO");
        for k in ["foo", "Foo", "FOO"] {
            assert!(map.get(k).is_none());
        }
        assert!(map.is_empty());
    }

    #[test]
    fn test_put_all() {
        let mut map = Map::new();
        let mut other = Map::new();
        other.insert("Foo", v("cheese"));
        other.insert("bar", Value::from(123));
        map.extend(other);
        for k in ["FOO", "foo", "Foo"] {
            assert_eq!(Some(&v("cheese")), map.get(k));
        }
        for k in ["BAR", "bar", "BaR"] {
            assert_eq!(Some(&Value::from(123)), map.get(k));
        }
        // key case should be preserved
        let keys: HashMap<String, Value> = (&map).into();
        assert_eq!(Some(&v("cheese")), keys.get("Foo"));
        assert!(!keys.contains_key("foo"));
        assert!(!keys.contains_key("FOO"));
        assert_eq!(Some(&Value::from(123)), keys.get("bar"));
        assert!(!keys.contains_key("Bar"));
        assert!(!keys.contains_key("BAR"));
    }

    #[test]
    fn test_put_all_other() {
        let mut map = Map::new();
        let other: HashMap<String, Value> =
            HashMap::from([("Foo".into(), v("cheese")), ("bar".into(), 123.into())]);
        map.extend(other);
        assert_eq!(Some(&v("cheese")), map.get("FOO"));
        assert_eq!(Some(&Value::from(123)), map.get("BaR"));
    }

    #[test]
    fn test_put_all_empty() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        map.extend(HashMap::<String, Value>::new());
        assert_eq!(Some(&v("cheese")), map.get("FOO"));
        assert_eq!(1, map.len());
    }

    #[test]
    fn test_construct_from_other() {
        let other: HashMap<String, Value> =
            HashMap::from([("Foo".into(), v("cheese")), ("bar".into(), 123.into())]);
        let map: Map = other.into_iter().collect();
        assert_eq!(Some(&v("cheese")), map.get("foo"));
        assert_eq!(Some(&Value::from(123)), map.get("BAR"));
    }

    #[test]
    fn test_key_set() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("BAR", 123.into());
        map.insert("baZ", v("beer"));
        for k in [
            "Foo", "foo", "FOO", "BAR", "bar", "Bar", "baZ", "baz", "Baz", "BAZ",
        ] {
            assert!(map.contains_key(k), "{k}");
        }
    }

    #[test]
    fn test_key_set_remove_with_different_case() {
        let mut map = Map::new();
        map.insert("X-Trace", v("abc"));
        map.insert("Accept", v("text/plain"));
        assert!(map.remove("x-trace").is_some());
        assert!(!map.contains_key("X-Trace"));
        assert_eq!(vec!["Accept"], map.keys().collect::<Vec<_>>());
        assert!(map.remove("x-trace").is_none());
        assert!(map.remove("x-other").is_none());
        assert_eq!(1, map.len());
    }

    #[test]
    fn test_key_set_remove_all_with_different_case() {
        let mut map = Map::new();
        map.insert("X-Trace", v("abc"));
        map.insert("X-TENANT", v("t1"));
        map.insert("Accept", v("text/plain"));
        assert!(map.remove_all(["x-trace", "x-tenant"]));
        assert_eq!(vec!["Accept"], map.keys().collect::<Vec<_>>());
        assert!(!map.remove_all(["x-trace", "x-tenant"]));
        map.insert("X-Trace", v("abc"));
        map.insert("X-TENANT", v("t1"));
        assert!(map.remove_all(["x-trace", "x-tenant", "x-other", "x-more"]));
        assert_eq!(vec!["Accept"], map.keys().collect::<Vec<_>>());
        assert_eq!(Some(&v("text/plain")), map.get("accept"));
    }

    #[test]
    fn test_key_set_retain_all_with_different_case() {
        let mut map = Map::new();
        map.insert("X-Trace", v("abc"));
        map.insert("X-TENANT", v("t1"));
        map.insert("Accept", v("text/plain"));
        assert!(map.retain_keys(["accept", "x-tenant", "x-other"]));
        assert_eq!(vec!["X-TENANT", "Accept"], map.keys().collect::<Vec<_>>());
        assert!(!map.retain_keys(["ACCEPT", "X-Tenant"]));
        assert_eq!(2, map.len());
        assert!(map.retain_keys(Vec::<String>::new()));
        assert!(map.is_empty());
    }

    #[test]
    fn test_key_set_iterator_remove() {
        let mut map = Map::new();
        map.insert("X-Trace", v("abc"));
        map.insert("X-Tenant", v("t1"));
        map.insert("Accept", v("text/plain"));
        map.retain(|k, _| k != "X-Tenant");
        assert_eq!(vec!["X-Trace", "Accept"], map.keys().collect::<Vec<_>>());
        map.retain(|k, _| !k.starts_with("X-"));
        assert_eq!(vec!["Accept"], map.keys().collect::<Vec<_>>());
    }

    #[test]
    fn test_retain_keys_copy_to_another_map() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("BAR", 123.into());
        map.insert("baZ", v("beer"));
        let other: HashMap<String, Value> = (&map).into();
        assert!(other.contains_key("Foo"));
        assert!(!other.contains_key("foo"));
        assert!(!other.contains_key("FOO"));
        assert!(other.contains_key("BAR"));
        assert!(!other.contains_key("bar"));
        assert!(other.contains_key("baZ"));
        assert!(!other.contains_key("baz"));
    }

    #[test]
    fn test_values() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("BAR", v("123"));
        map.insert("baZ", v("Beer"));
        for val in map.values() {
            assert_eq!("String", val.type_name());
        }
        assert_eq!(3, map.values().count());
        assert!(map.contains_value(&v("cheese")));
        assert!(map.contains_value(&v("123")));
        assert!(map.contains_value(&v("Beer")));
    }

    #[test]
    fn test_romeks() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        assert_eq!(1, map.len());
        assert_eq!(Some(&v("cheese")), map.get("fOo"));
        for k in ["foo", "FOO", "FoO", "Foo", "fOO"] {
            assert!(map.contains_key(k));
        }
        map.insert("FOO", v("cake"));
        assert_eq!(1, map.len());
        assert!(map.contains_key("foo"));
        assert!(map.contains_key("FOO"));
        assert_eq!(Some(&v("cake")), map.get("fOo"));
    }

    #[test]
    fn test_romeks_using_regular_hash_map() {
        let mut map: HashMap<String, Value> = HashMap::new();
        map.insert("foo".into(), v("cheese"));
        assert!(!map.contains_key("fOo"));
        map.insert("FOO".into(), v("cake"));
        assert_eq!(2, map.len());
        assert_eq!(Some(&v("cheese")), map.get("foo"));
        assert_eq!(Some(&v("cake")), map.get("FOO"));
    }

    #[test]
    fn test_romeks_transferred_to_hash_map_afterwards() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("FOO", v("cake"));
        assert_eq!(1, map.len());
        let other: HashMap<String, Value> = (&map).into();
        assert!(!other.contains_key("foo"));
        assert!(!other.contains_key("FOO"));
        // the original key is preserved, which is the 1st key we put
        assert!(other.contains_key("Foo"));
        assert_eq!(1, other.len());
    }

    #[test]
    fn test_copy_to_another_map_preserve_key_case() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("BAR", v("cake"));
        let mut other: HashMap<String, Value> = HashMap::new();
        for (k, val) in &map {
            other.insert(k.to_string(), val.clone());
        }
        assert!(!other.contains_key("foo"));
        assert!(other.contains_key("Foo"));
        assert!(!other.contains_key("bar"));
        assert!(other.contains_key("BAR"));
        assert_eq!(2, other.len());
        let owned: HashMap<String, Value> = map.clone().into_iter().collect();
        assert_eq!(other, owned);
    }

    #[test]
    fn test_concurrent() {
        let map = Arc::new(Mutex::new(Map::new()));
        let handles: Vec<_> = (0..5)
            .map(|t| {
                let map = map.clone();
                std::thread::spawn(move || {
                    for i in (t..1000).step_by(5) {
                        let mut foo = Map::new();
                        foo.insert(format!("counter{i}"), Value::from(i));
                        foo.insert("foo", 123.into());
                        foo.insert("bar", 456.into());
                        foo.insert("cake", v("cheese"));
                        // copy foo to map as map is a shared resource
                        map.lock().unwrap().extend(foo);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let map = map.lock().unwrap();
        assert_eq!(1003, map.len());
        assert!(map.contains_key("counter0"));
        assert!(map.contains_key("counter500"));
        assert!(map.contains_key("counter999"));
        assert_eq!(Some(&Value::from(123)), map.get("FOO"));
        assert_eq!(Some(&Value::from(456)), map.get("Bar"));
        assert_eq!(Some(&v("cheese")), map.get("cAKe"));
    }

    #[test]
    fn test_insertion_order() {
        let mut map = Map::new();
        map.insert("Zebra", 1.into());
        map.insert("apple", 2.into());
        map.insert("Mango", 3.into());
        let entries: Vec<(&str, &Value)> = map.iter().collect();
        assert_eq!(
            vec![
                ("Zebra", &Value::from(1)),
                ("apple", &Value::from(2)),
                ("Mango", &Value::from(3))
            ],
            entries
        );
    }

    #[test]
    fn test_iterator_remove() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("Bar", v("cake"));
        map.insert("Baz", v("beer"));
        map.retain(|k, _| k != "Bar");
        assert_eq!(2, map.len());
        assert!(!map.contains_key("Bar"));
        assert!(map.contains_key("Foo"));
        assert!(map.contains_key("Baz"));
    }

    #[test]
    fn test_entry_set_remove() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("Bar", v("cake"));
        assert!(map.remove_entry_if_value("foo", &v("cheese")));
        assert_eq!(1, map.len());
        assert!(!map.contains_key("Foo"));
        assert!(!map.remove_entry_if_value("bar", &v("wrong")));
        assert_eq!(1, map.len());
    }

    #[test]
    fn test_entry_set_value() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        let (_, value) = map.iter_mut().next().unwrap();
        assert_eq!(&v("cheese"), value);
        let old = std::mem::replace(value, v("cake"));
        assert_eq!(v("cheese"), old);
        assert_eq!(Some(&v("cake")), map.get("Foo"));
    }

    #[test]
    fn test_remove_then_reput() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.remove("foo");
        assert!(map.is_empty());
        map.insert("FOO", v("cake"));
        assert_eq!(1, map.len());
        assert_eq!(Some(&v("cake")), map.get("foo"));
        // new key case should be used since old entry was removed
        let copy: HashMap<String, Value> = (&map).into();
        assert!(copy.contains_key("FOO"));
        assert!(!copy.contains_key("Foo"));
    }

    #[test]
    fn test_resize() {
        let mut map = Map::new();
        for i in 0..200 {
            map.insert(format!("key{i}"), Value::from(i));
        }
        assert_eq!(200, map.len());
        for i in 0..200 {
            assert!(map.contains_key(&format!("KEY{i}")));
            assert_eq!(Some(&Value::from(i)), map.get(&format!("key{i}")));
        }
        for i in 0..100 {
            map.remove(&format!("Key{i}"));
        }
        assert_eq!(100, map.len());
        for i in 100..200 {
            assert_eq!(Some(&Value::from(i)), map.get(&format!("KEY{i}")));
        }
    }

    #[test]
    fn test_contains_value() {
        let mut map = Map::new();
        map.insert("foo", v("cheese"));
        map.insert("bar", Value::Null);
        assert!(map.contains_value(&v("cheese")));
        assert!(map.contains_value(&Value::Null));
        assert!(!map.contains_value(&v("missing")));
    }

    #[test]
    fn test_null_value() {
        let mut map = Map::new();
        map.insert("foo", Value::Null);
        assert_eq!(1, map.len());
        assert!(map.contains_key("FOO"));
        assert_eq!(Some(&Value::Null), map.get("foo"));
        assert!(!map.contains_key("bar"));
    }

    #[test]
    fn test_clear_and_reuse() {
        let mut map = Map::new();
        map.insert("Foo", v("cheese"));
        map.insert("Bar", v("cake"));
        map.clear();
        assert_eq!(0, map.len());
        assert!(map.is_empty());
        assert!(!map.contains_key("Foo"));
        map.insert("Baz", v("beer"));
        assert_eq!(1, map.len());
        assert_eq!(Some(&v("beer")), map.get("BAZ"));
    }

    #[test]
    fn test_known_key_deduplication() {
        let _g = KNOWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        register_known_keys(["CamelCharsetName", "CamelExchangeId", "breadcrumbId"]);
        let canonical = |k: &str| {
            known_keys()
                .read()
                .unwrap()
                .iter()
                .find(|x| x.as_ref() == k)
                .unwrap()
                .clone()
        };

        let mut map = Map::new();
        let deserialized_key = String::from("CamelCharsetName");
        map.insert(&deserialized_key, v("UTF-8"));
        // the stored key should be the canonical shared instance
        assert!(Arc::ptr_eq(
            &canonical("CamelCharsetName"),
            map.key_arcs().next().unwrap()
        ));

        // a key with a different case is not replaced by the canonical key
        let mut map2 = Map::new();
        map2.insert("camelcharsetname", v("UTF-8"));
        assert_eq!(Some("camelcharsetname"), map2.keys().next());
        assert!(!Arc::ptr_eq(
            &canonical("CamelCharsetName"),
            map2.key_arcs().next().unwrap()
        ));
        assert_eq!(Some(&v("UTF-8")), map2.get("CamelCharsetName"));

        // non-registered key is stored as-is
        map.insert("CustomHeader", v("value"));
        assert_eq!(Some("CustomHeader"), map.keys().nth(1));
        register_known_keys(Vec::<String>::new());
    }

    #[test]
    fn test_known_key_keeps_key_case() {
        let _g = KNOWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        register_known_keys(["Content-Type", "CamelFileName"]);
        let mut map = Map::new();
        map.insert("content-type", v("text/plain"));
        map.insert("camelfilename", v("a.txt"));
        map.insert(String::from("Content-Length"), v("12"));
        assert_eq!(
            vec!["content-type", "camelfilename", "Content-Length"],
            map.keys().collect::<Vec<_>>()
        );
        assert_eq!(Some(&v("text/plain")), map.get("Content-Type"));
        assert_eq!(Some(&v("text/plain")), map.get("CONTENT-TYPE"));
        assert_eq!(Some(&v("a.txt")), map.get("CamelFileName"));
        // the first insert decides the case: a later insert with another case only replaces the value
        map.insert("Content-Type", v("application/json"));
        assert_eq!(
            1,
            map.keys()
                .filter(|k| k.eq_ignore_ascii_case("content-type"))
                .count()
        );
        assert_eq!(Some("content-type"), map.keys().next());
        assert_eq!(Some(&v("application/json")), map.get("content-type"));
        // an equal key still shares the canonical instance
        let mut map2 = Map::new();
        map2.insert(String::from("Content-Type"), v("text/plain"));
        let canonical = known_keys()
            .read()
            .unwrap()
            .iter()
            .find(|x| x.as_ref() == "Content-Type")
            .unwrap()
            .clone();
        assert!(Arc::ptr_eq(&canonical, map2.key_arcs().next().unwrap()));
        // also when copied from another map
        let copy: Map = map
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        assert_eq!(
            vec!["content-type", "camelfilename", "Content-Length"],
            copy.keys().collect::<Vec<_>>()
        );
        register_known_keys(Vec::<String>::new());
    }

    #[test]
    fn test_unicode_and_display() {
        let mut map: CaseInsensitiveMap<i32> = CaseInsensitiveMap::new();
        map.insert("Søren", 1);
        assert_eq!(Some(&1), map.get("SØREN"));
        map.insert("ÆBLE", 2);
        assert_eq!(Some(("ÆBLE", &2)), map.get_key_value("æble"));
        assert_eq!("{Søren=1, ÆBLE=2}", map.to_string());
        assert_eq!(2, map["æble"]);
        assert!(!map.contains_key("Sore"));
        let (k, val) = map.remove_entry("søren").unwrap();
        assert_eq!(("Søren", 1), (k.as_ref(), val));
        let a: CaseInsensitiveMap<i32> = [("A", 1)].into_iter().collect();
        let b: CaseInsensitiveMap<i32> = [("a", 1)].into_iter().collect();
        assert_eq!(a, b);
        assert!(format!("{a:?}").contains("\"A\": 1"));
        *map.get_mut("æble").unwrap() += 1;
        assert_eq!(Some(&3), map.values().next());
        for val in map.values_mut() {
            *val = 0;
        }
        assert_eq!(Some(&0), map.get("ÆBLE"));
        let mut w = CaseInsensitiveMap::with_capacity(4);
        w.insert("x", 1);
        assert_eq!(1, w.len());
    }
}
