//! A number of helper methods for working with collections.
//!
//! Port of `org.apache.camel.util.CollectionHelper`. The Java `mapOf`, `immutableMapOf`,
//! `unmodifiableMap` and `propertiesOf` varargs builders are not ported: Rust builds maps
//! from arrays (`HashMap::from([(k, v), ...])`) and immutability is expressed by sharing a
//! `&` reference.

use crate::value::{Value, ValueMap};
use std::collections::HashSet;
use std::fmt::Display;
use std::hash::Hash;

/// Returns the size of the value if it can be determined to be a collection (list, map or
/// bytes).
///
/// # Returns
///
/// The size, or `None` if not a collection.
pub fn size(value: &Value) -> Option<usize> {
    value.size()
}

/// Sets the value of the entry in the map for the given key, though if the map already
/// contains a value for the given key then the value is appended to a list of values.
///
/// # Arguments
///
/// * `map` - the map to add the entry to
/// * `key` - the key in the map
/// * `value` - the value to put in the map
pub fn append_value(map: &mut ValueMap, key: &str, value: Value) {
    match map.get_mut(key) {
        Some(Value::List(list)) => list.push(value),
        Some(existing) if !existing.is_null() => {
            let old = std::mem::take(existing);
            *existing = Value::List(vec![old, value]);
        }
        _ => {
            map.insert(key.to_string(), value);
        }
    }
}

/// When trying to set the value for a map, if the key already exists the value is appended to
/// a list (also when the existing value is null). Otherwise, sets the entry to the given
/// value.
///
/// # Arguments
///
/// * `headers` - the map whose entry will be set or appended
/// * `key` - the key on the map
/// * `value` - the value to set or append within the map
pub fn append_entry(headers: &mut ValueMap, key: &str, value: Value) {
    match headers.get_mut(key) {
        Some(Value::List(list)) => list.push(value),
        Some(existing) => {
            let old = std::mem::take(existing);
            *existing = Value::List(vec![old, value]);
        }
        None => {
            headers.insert(key.to_string(), value);
        }
    }
}

/// Creates a set containing the given items.
pub fn create_set_containing<T: Eq + Hash, I: IntoIterator<Item = T>>(contents: I) -> HashSet<T> {
    contents.into_iter().collect()
}

/// Returns the items as a comma delimited string (no spaces), or an empty string for no items.
///
/// # Examples
///
/// ```
/// use camel_util::collection_helper::collection_as_comma_delimited_string;
/// assert_eq!("Claus,Willem,Jonathan", collection_as_comma_delimited_string(["Claus", "Willem", "Jonathan"]));
/// ```
pub fn collection_as_comma_delimited_string<I, T>(col: I) -> String
where
    I: IntoIterator<Item = T>,
    T: Display,
{
    col.into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Traverses the given map recursively and flattens the keys by combining them with the
/// optional separator.
///
/// # Arguments
///
/// * `map` - the map
/// * `separator` - optional separator to use in key name, for example a hyphen or dot
///
/// # Returns
///
/// The map with flattened keys.
///
/// # Examples
///
/// ```
/// use camel_util::{collection_helper::flatten_keys_in_map, value::{Value, ValueMap}};
/// let mut api = ValueMap::new();
/// api.insert("version".into(), "1.0.0".into());
/// let mut root = ValueMap::new();
/// root.insert("api".into(), Value::Map(api));
/// assert_eq!("1.0.0", flatten_keys_in_map(&root, Some("."))["api.version"]);
/// ```
pub fn flatten_keys_in_map(map: &ValueMap, separator: Option<&str>) -> ValueMap {
    let mut answer = ValueMap::new();
    do_flatten_keys_in_map(map, "", separator.unwrap_or(""), &mut answer);
    answer
}

fn do_flatten_keys_in_map(source: &ValueMap, prefix: &str, separator: &str, target: &mut ValueMap) {
    for (key, value) in source {
        let new_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}{separator}{key}")
        };
        match value {
            Value::Map(m) => do_flatten_keys_in_map(m, &new_key, separator, target),
            v => {
                target.insert(new_key, v.clone());
            }
        }
    }
}

/// Builds a new map that is the result of merging the given maps (later maps win).
pub fn merge_maps<'a, I: IntoIterator<Item = &'a ValueMap>>(maps: I) -> ValueMap {
    let mut answer = ValueMap::new();
    for m in maps {
        for (k, v) in m {
            answer.insert(k.clone(), v.clone());
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_as_comma_delimited_string() {
        assert_eq!(
            "Claus,Willem,Jonathan",
            collection_as_comma_delimited_string(["Claus", "Willem", "Jonathan"])
        );
        assert_eq!(
            "",
            collection_as_comma_delimited_string(Vec::<String>::new())
        );
        assert_eq!("Claus", collection_as_comma_delimited_string(["Claus"]));
    }

    #[test]
    fn test_size() {
        let mut map = ValueMap::new();
        map.insert("foo".into(), 123.into());
        map.insert("bar".into(), 456.into());
        assert_eq!(Some(2), size(&Value::Map(map)));
        assert_eq!(Some(2), size(&Value::from(vec!["Claus", "Willem"])));
        assert_eq!(None, size(&Value::from("x")));
    }

    #[test]
    fn test_append_value() {
        let mut map = ValueMap::new();
        append_value(&mut map, "foo", 123.into());
        assert_eq!(1, map.len());
        append_value(&mut map, "foo", 456.into());
        assert_eq!(1, map.len());
        append_value(&mut map, "bar", 789.into());
        assert_eq!(2, map.len());
        let values = map["foo"].as_list().unwrap();
        assert_eq!(2, values.len());
        assert_eq!(123, values[0]);
        assert_eq!(456, values[1]);
        assert_eq!(789, map["bar"]);
    }

    #[test]
    fn test_append_entry() {
        let mut map = ValueMap::new();
        map.insert("a".into(), Value::Null);
        append_entry(&mut map, "a", 1.into());
        assert_eq!("[null, 1]", map["a"].to_string());
        append_entry(&mut map, "b", 2.into());
        assert_eq!(2, map["b"]);
    }

    #[test]
    fn test_create_set_containing() {
        let set = create_set_containing(["foo", "bar", "baz"]);
        assert_eq!(3, set.len());
        assert!(set.contains("foo"));
        assert!(set.contains("bar"));
        assert!(set.contains("baz"));
    }

    #[test]
    fn test_flatten_keys_in_map() {
        let mut contact = ValueMap::new();
        contact.insert("organization".into(), "Apache Software Foundation".into());
        let mut api = ValueMap::new();
        api.insert("version".into(), "1.0.0".into());
        api.insert("title".into(), "My cool API".into());
        api.insert("contact".into(), Value::Map(contact));
        let mut root = ValueMap::new();
        root.insert("api".into(), Value::Map(api));
        root.insert("cors".into(), true.into());

        let flat = flatten_keys_in_map(&root, Some("."));
        assert_eq!(4, flat.len());
        assert_eq!(true, flat["cors"]);
        assert_eq!("1.0.0", flat["api.version"]);
        assert_eq!("My cool API", flat["api.title"]);
        assert_eq!(
            "Apache Software Foundation",
            flat["api.contact.organization"]
        );
    }

    #[test]
    fn test_merge_maps() {
        let mut a = ValueMap::new();
        a.insert("x".into(), 1.into());
        let mut b = ValueMap::new();
        b.insert("x".into(), 2.into());
        b.insert("y".into(), 3.into());
        let m = merge_maps([&a, &b]);
        assert_eq!(2, m["x"]);
        assert_eq!(3, m["y"]);
    }
}
