//! Ordered properties and the Java `.properties` file format.
//!
//! Port of `org.apache.camel.util.OrderedProperties`, `CamelCaseOrderedProperties`,
//! `OrderedLocationProperties`, `BaseOrderedProperties` and `PropertiesHelper`.
//!
//! The Java classes extended `java.util.Properties` and relied on its `load` method for the
//! `.properties` format; [`parse_properties`] reproduces that format exactly (comments,
//! `=`/`:`/whitespace separators, line continuations and `\uXXXX` escapes).
//!
//! The Java classes were internally synchronized with a lock. In Rust the methods take
//! `&mut self`; wrap the properties in a `Mutex` to share them between threads.

use crate::string_helper::{camel_case_to_dash, dash_to_camel_case};
use crate::value::{Value, ValueMap};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::fmt;
use std::io::Read;

/// Error raised when a `.properties` text cannot be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertiesError(pub String);

impl fmt::Display for PropertiesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PropertiesError {}

/// Parses text in the Java `.properties` format into key/value pairs (in file order).
///
/// The format is the one of `java.util.Properties.load(Reader)`:
///
/// * a line whose first non-whitespace character is `#` or `!` is a comment
/// * the key ends at the first unescaped `=`, `:` or whitespace; whitespace around the
///   separator is ignored
/// * a line ending with an odd number of backslashes continues on the next line (whose leading
///   whitespace is skipped)
/// * `\t`, `\n`, `\r`, `\f` and `\uXXXX` escapes are decoded, any other escaped character
///   stands for itself
///
/// # Errors
///
/// Returns an error for a malformed `\uXXXX` escape.
///
/// # Examples
///
/// ```
/// use camel_util::properties::parse_properties;
/// let pairs = parse_properties("# comment\nhello = World\nmulti=a \\\n    b\nkey\\ with\\ spaces:x\nu=\\u00f8").unwrap();
/// assert_eq!(vec![
///     ("hello".to_string(), "World".to_string()),
///     ("multi".to_string(), "a b".to_string()),
///     ("key with spaces".to_string(), "x".to_string()),
///     ("u".to_string(), "ø".to_string()),
/// ], pairs);
/// ```
pub fn parse_properties(text: &str) -> Result<Vec<(String, String)>, PropertiesError> {
    let mut answer = Vec::new();
    for line in logical_lines(text) {
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        let mut key_len = 0;
        let mut value_start = len;
        let mut has_sep = false;
        let mut preceding_backslash = false;
        while key_len < len {
            let c = chars[key_len];
            if (c == '=' || c == ':') && !preceding_backslash {
                value_start = key_len + 1;
                has_sep = true;
                break;
            } else if (c == ' ' || c == '\t' || c == '\u{c}') && !preceding_backslash {
                value_start = key_len + 1;
                break;
            }
            preceding_backslash = c == '\\' && !preceding_backslash;
            key_len += 1;
        }
        while value_start < len {
            let c = chars[value_start];
            if c != ' ' && c != '\t' && c != '\u{c}' {
                if !has_sep && (c == '=' || c == ':') {
                    has_sep = true;
                } else {
                    break;
                }
            }
            value_start += 1;
        }
        let key = load_convert(&chars[..key_len])?;
        let value = load_convert(&chars[value_start.min(len)..])?;
        answer.push((key, value));
    }
    Ok(answer)
}

/// Splits the text into logical lines (joining continuations, skipping comments and blanks).
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut continuing = false;
    for natural in split_natural_lines(text) {
        let trimmed = natural.trim_start_matches([' ', '\t', '\u{c}']);
        if !continuing
            && (trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!'))
        {
            continue;
        }
        // count trailing backslashes
        let backslashes = trimmed.chars().rev().take_while(|&c| c == '\\').count();
        if backslashes % 2 == 1 {
            current.push_str(&trimmed[..trimmed.len() - 1]);
            continuing = true;
        } else {
            current.push_str(trimmed);
            out.push(std::mem::take(&mut current));
            continuing = false;
        }
    }
    if continuing {
        out.push(current);
    }
    out
}

fn split_natural_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn load_convert(chars: &[char]) -> Result<String, PropertiesError> {
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&next) = chars.get(i) else {
            break;
        };
        i += 1;
        match next {
            'u' => {
                let hex: String = chars
                    .get(i..i + 4)
                    .map(|h| h.iter().collect())
                    .unwrap_or_default();
                let code = u32::from_str_radix(&hex, 16)
                    .ok()
                    .filter(|_| hex.len() == 4)
                    .ok_or_else(|| PropertiesError("Malformed \\uxxxx encoding.".to_string()))?;
                i += 4;
                // combine UTF-16 surrogate pairs (😀)
                if (0xD800..0xDC00).contains(&code)
                    && chars.get(i) == Some(&'\\')
                    && chars.get(i + 1) == Some(&'u')
                {
                    let low: String = chars
                        .get(i + 2..i + 6)
                        .map(|h| h.iter().collect())
                        .unwrap_or_default();
                    if let Ok(low) = u32::from_str_radix(&low, 16) {
                        if (0xDC00..0xE000).contains(&low) {
                            let combined = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            out.push(char::from_u32(combined).unwrap_or('\u{fffd}'));
                            i += 6;
                            continue;
                        }
                    }
                }
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'n' => out.push('\n'),
            'f' => out.push('\u{c}'),
            other => out.push(other),
        }
    }
    Ok(out)
}

/// Escapes a key or value for writing in the `.properties` format (Java `saveConvert`).
///
/// # Arguments
///
/// * `text` - the text to escape
/// * `escape_space` - whether all spaces must be escaped (keys), otherwise only a leading
///   space (values)
pub fn save_convert(text: &str, escape_space: bool) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for (i, c) in text.chars().enumerate() {
        match c {
            ' ' if i == 0 || escape_space => out.push_str("\\ "),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// An ordered set of string properties: the key/values are stored in the order they are added
/// or loaded.
///
/// This is the Rust port of `org.apache.camel.util.OrderedProperties` (and its base class
/// `BaseOrderedProperties`), only intended as implementation detail for the Camel properties
/// component.
///
/// # Examples
///
/// ```
/// use camel_util::properties::OrderedProperties;
/// let mut prop = OrderedProperties::new();
/// prop.set_property("c", "CCC");
/// prop.set_property("a", "AAA");
/// assert_eq!(vec!["c", "a"], prop.keys().collect::<Vec<_>>());
/// prop.load("b = BBB").unwrap();
/// assert_eq!(Some("BBB"), prop.get_property("b"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderedProperties {
    map: IndexMap<String, String>,
}

impl OrderedProperties {
    /// Creates empty properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the properties as an ordered map.
    pub fn as_map(&self) -> &IndexMap<String, String> {
        &self.map
    }

    /// Sets the property, returning the previous value.
    pub fn set_property(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Option<String> {
        self.map.insert(key.into(), value.into())
    }

    /// Sets the property from any displayable key and value (the Java `put(Object, Object)`,
    /// which stored `toString()` of both).
    pub fn put(&mut self, key: impl fmt::Display, value: impl fmt::Display) -> Option<String> {
        self.set_property(key.to_string(), value.to_string())
    }

    /// Puts all the given entries.
    pub fn put_all<K: fmt::Display, V: fmt::Display>(
        &mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) {
        for (k, v) in entries {
            self.put(k, v);
        }
    }

    /// Returns the value of the property.
    pub fn get_property(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    /// Returns the value of the property, or the default value if not set.
    pub fn get_property_or<'a>(&'a self, key: &str, default_value: &'a str) -> &'a str {
        self.get_property(key).unwrap_or(default_value)
    }

    /// Returns the value of the property, or the default value if not set (Java
    /// `getOrDefault`).
    pub fn get_or_default<'a>(&'a self, key: &str, default_value: &'a str) -> &'a str {
        self.get_property_or(key, default_value)
    }

    /// Whether the property is set.
    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// Whether any property has the given value (also the legacy `Hashtable.contains`).
    pub fn contains_value(&self, value: &str) -> bool {
        self.map.values().any(|v| v == value)
    }

    /// Removes the property, keeping the order of the remaining properties.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.map.shift_remove(key)
    }

    /// Removes the property only if it is set to the given value.
    ///
    /// # Returns
    ///
    /// `true` if the property was removed.
    pub fn remove_if_value(&mut self, key: &str, value: &str) -> bool {
        if self.get_property(key) == Some(value) {
            self.remove(key);
            true
        } else {
            false
        }
    }

    /// Sets the property only if not already set.
    ///
    /// # Returns
    ///
    /// The existing value, or `None` if the property was set now.
    pub fn put_if_absent(&mut self, key: &str, value: &str) -> Option<String> {
        match self.get_property(key) {
            Some(v) => Some(v.to_string()),
            None => {
                self.set_property(key, value);
                None
            }
        }
    }

    /// Replaces the value of the property only if it is set.
    ///
    /// # Returns
    ///
    /// The previous value, or `None` if the property is not set.
    pub fn replace(&mut self, key: &str, value: &str) -> Option<String> {
        if self.contains_key(key) {
            self.set_property(key, value)
        } else {
            None
        }
    }

    /// Replaces the value of the property only if it is currently set to `old_value`.
    pub fn replace_if(&mut self, key: &str, old_value: &str, new_value: &str) -> bool {
        if self.get_property(key) == Some(old_value) {
            self.set_property(key, new_value);
            true
        } else {
            false
        }
    }

    /// Replaces every value with the result of the function.
    pub fn replace_all(&mut self, mut f: impl FnMut(&str, &str) -> String) {
        for (k, v) in self.map.iter_mut() {
            *v = f(k, v);
        }
    }

    /// Computes a new value from the key and the current value (if any); `None` removes the
    /// property.
    pub fn compute(
        &mut self,
        key: &str,
        f: impl FnOnce(&str, Option<&str>) -> Option<String>,
    ) -> Option<String> {
        let answer = f(key, self.get_property(key));
        match &answer {
            Some(v) => {
                self.set_property(key, v.clone());
            }
            None => {
                self.remove(key);
            }
        }
        answer
    }

    /// Computes the value if the property is not set.
    pub fn compute_if_absent(
        &mut self,
        key: &str,
        f: impl FnOnce(&str) -> Option<String>,
    ) -> Option<String> {
        if let Some(v) = self.get_property(key) {
            return Some(v.to_string());
        }
        let answer = f(key);
        if let Some(v) = &answer {
            self.set_property(key, v.clone());
        }
        answer
    }

    /// Computes a new value if the property is set; `None` removes the property.
    pub fn compute_if_present(
        &mut self,
        key: &str,
        f: impl FnOnce(&str, &str) -> Option<String>,
    ) -> Option<String> {
        let old = self.get_property(key)?.to_string();
        self.compute(key, |k, _| f(k, &old))
    }

    /// Merges the value with the current value using the function; `None` removes the property.
    pub fn merge(
        &mut self,
        key: &str,
        value: &str,
        f: impl FnOnce(&str, &str) -> Option<String>,
    ) -> Option<String> {
        let answer = match self.get_property(key) {
            None => Some(value.to_string()),
            Some(old) => f(old, value),
        };
        match &answer {
            Some(v) => {
                self.set_property(key, v.clone());
            }
            None => {
                self.remove(key);
            }
        }
        answer
    }

    /// Number of properties.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether there are no properties.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Removes all properties.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// The keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.map.keys().map(String::as_str)
    }

    /// The values in order.
    pub fn values(&self) -> impl Iterator<Item = &str> + '_ {
        self.map.values().map(String::as_str)
    }

    /// The entries in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Loads properties in the `.properties` format (see [`parse_properties`]), adding them in
    /// file order.
    ///
    /// # Errors
    ///
    /// Returns an error if the text is malformed.
    pub fn load(&mut self, text: &str) -> Result<(), PropertiesError> {
        for (k, v) in parse_properties(text)? {
            self.set_property(k, v);
        }
        Ok(())
    }

    /// Loads properties in the `.properties` format from a reader (UTF-8).
    ///
    /// # Errors
    ///
    /// Returns an error if reading fails or the text is malformed.
    pub fn load_reader(&mut self, mut reader: impl Read) -> Result<(), PropertiesError> {
        let mut text = String::new();
        reader
            .read_to_string(&mut text)
            .map_err(|e| PropertiesError(e.to_string()))?;
        self.load(&text)
    }

    /// Writes the properties in the `.properties` format (`key=value` lines, escaped).
    pub fn store(&self) -> String {
        self.iter()
            .map(|(k, v)| format!("{}={}\n", save_convert(k, true), save_convert(v, false)))
            .collect()
    }
}

impl fmt::Display for OrderedProperties {
    /// Formats like the Java `toString()`: `{k=v, k2=v2}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner: Vec<String> = self.iter().map(|(k, v)| format!("{k}={v}")).collect();
        write!(f, "{{{}}}", inner.join(", "))
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for OrderedProperties {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        OrderedProperties {
            map: iter
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
}

/// A camelCase ordered set of properties where the key/values are stored in the order they are
/// added or loaded.
///
/// The keys are stored in the original case, for example a key of
/// `camel.main.stream-caching-enabled` is stored as `camel.main.stream-caching-enabled`.
/// However the lookup of a value by key with the `get` methods supports camelCase or dash
/// style.
///
/// Only intended as implementation detail for Camel tooling such as camel-jbang. Port of
/// `org.apache.camel.util.CamelCaseOrderedProperties`; all other methods are those of
/// [`OrderedProperties`] (through `Deref`).
///
/// # Examples
///
/// ```
/// use camel_util::properties::CamelCaseOrderedProperties;
/// let mut prop = CamelCaseOrderedProperties::new();
/// prop.set_property("camel.main.stream-caching-enabled", "true");
/// assert_eq!(Some("true"), prop.get_property("camel.main.streamCachingEnabled"));
/// assert!(prop.contains_key("camel.main.streamCachingEnabled"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CamelCaseOrderedProperties {
    inner: OrderedProperties,
}

impl CamelCaseOrderedProperties {
    /// Creates empty properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the value of the property, trying the key as-is, then in camelCase and then in
    /// dash style.
    pub fn get_property(&self, key: &str) -> Option<&str> {
        self.inner
            .get_property(key)
            .or_else(|| self.inner.get_property(&dash_to_camel_case(key)))
            .or_else(|| self.inner.get_property(&camel_case_to_dash(key)))
    }

    /// Returns the value of the property (camelCase or dash style), or the default value.
    pub fn get_property_or<'a>(&'a self, key: &str, default_value: &'a str) -> &'a str {
        self.get_property(key).unwrap_or(default_value)
    }

    /// Whether the property is set, with the key as-is, in camelCase or in dash style.
    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(key)
            || self.inner.contains_key(&dash_to_camel_case(key))
            || self.inner.contains_key(&camel_case_to_dash(key))
    }
}

impl std::ops::Deref for CamelCaseOrderedProperties {
    type Target = OrderedProperties;

    fn deref(&self) -> &OrderedProperties {
        &self.inner
    }
}

impl std::ops::DerefMut for CamelCaseOrderedProperties {
    fn deref_mut(&mut self) -> &mut OrderedProperties {
        &mut self.inner
    }
}

/// Ordered properties that also keep track from which location the properties are sourced
/// from, and default values.
///
/// This can be used to track all the various sources for configuration that a Camel
/// application uses (properties file, ENV variables, hardcoded in code, modeline, YAML DSL
/// etc.).
///
/// **Important:** use [`put_location`](Self::put_location) to store location information, and
/// [`put_location_default`](Self::put_location_default) to store default value information.
/// Port of `org.apache.camel.util.OrderedLocationProperties`; all other methods are those of
/// [`OrderedProperties`] (through `Deref`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderedLocationProperties {
    inner: OrderedProperties,
    locations: HashMap<String, String>,
    default_values: HashMap<String, String>,
}

impl OrderedLocationProperties {
    /// Creates empty properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts the property with the location it was sourced from.
    pub fn put_location(
        &mut self,
        location: &str,
        key: impl fmt::Display,
        value: impl fmt::Display,
    ) {
        let key = key.to_string();
        self.locations.insert(key.clone(), location.to_string());
        self.inner.set_property(key, value.to_string());
    }

    /// Puts the property with the location it was sourced from and its default value.
    pub fn put_location_default(
        &mut self,
        location: &str,
        key: impl fmt::Display,
        value: impl fmt::Display,
        default_value: Option<&str>,
    ) {
        let key = key.to_string();
        self.put_location(location, &key, value);
        if let Some(d) = default_value {
            self.default_values.insert(key, d.to_string());
        }
    }

    /// Puts all the properties of the other instance, with their locations and default values.
    pub fn put_all_from(&mut self, other: &OrderedLocationProperties) {
        for (k, v) in other.iter() {
            let location = other.get_location(k).unwrap_or_default().to_string();
            self.put_location_default(&location, k, v, other.get_default_value(k));
        }
    }

    /// Puts all the entries from the given location.
    pub fn put_all_location<K: fmt::Display, V: fmt::Display>(
        &mut self,
        location: &str,
        entries: impl IntoIterator<Item = (K, V)>,
    ) {
        for (k, v) in entries {
            self.put_location(location, k, v);
        }
    }

    /// Gets the location from where the property was resolved.
    ///
    /// # Returns
    ///
    /// The location, or `None` if not possible to determine the location.
    pub fn get_location(&self, key: &str) -> Option<&str> {
        self.locations.get(key).map(String::as_str)
    }

    /// Gets the default value of the property, if a default value exists.
    ///
    /// # Returns
    ///
    /// The default value, or `None` if not possible to determine the default value.
    pub fn get_default_value(&self, key: &str) -> Option<&str> {
        self.default_values.get(key).map(String::as_str)
    }

    /// Removes all properties, locations and default values.
    pub fn clear(&mut self) {
        self.locations.clear();
        self.default_values.clear();
        self.inner.clear();
    }

    /// Removes the property and its location and default value.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.locations.remove(key);
        self.default_values.remove(key);
        self.inner.remove(key)
    }
}

impl std::ops::Deref for OrderedLocationProperties {
    type Target = OrderedProperties;

    fn deref(&self) -> &OrderedProperties {
        &self.inner
    }
}

impl std::ops::DerefMut for OrderedLocationProperties {
    fn deref_mut(&mut self) -> &mut OrderedProperties {
        &mut self.inner
    }
}

// ---------------------------------------------------------------------------------------
// PropertiesHelper

/// Extracts (and removes) the properties whose name starts with the prefix; the prefix is
/// removed from the names of the extracted properties.
pub fn extract_properties(properties: &mut ValueMap, option_prefix: &str) -> ValueMap {
    extract_properties_remove(properties, option_prefix, true)
}

/// Extracts the properties whose name starts with the prefix; the prefix is removed from the
/// names of the extracted properties.
///
/// # Arguments
///
/// * `properties` - the properties
/// * `option_prefix` - the prefix
/// * `remove` - whether to remove the extracted properties from `properties`
pub fn extract_properties_remove(
    properties: &mut ValueMap,
    option_prefix: &str,
    remove: bool,
) -> ValueMap {
    let mut rc = ValueMap::new();
    properties.retain(|name, value| match name.strip_prefix(option_prefix) {
        Some(stripped) => {
            rc.insert(stripped.to_string(), value.clone());
            !remove
        }
        None => true,
    });
    rc
}

/// Whether there are any properties with the given prefix (or any properties at all when the
/// prefix is empty).
pub fn has_properties(properties: &ValueMap, option_prefix: &str) -> bool {
    if option_prefix.is_empty() {
        !properties.is_empty()
    } else {
        properties.keys().any(|k| k.starts_with(option_prefix))
    }
}

/// Creates properties from key/value pairs.
pub fn as_properties<K: Into<String>, V: Into<String>>(
    pairs: impl IntoIterator<Item = (K, V)>,
) -> OrderedProperties {
    pairs.into_iter().collect()
}

/// Creates properties from a map, using the text form of each value.
pub fn as_properties_from_map(properties: &ValueMap) -> OrderedProperties {
    properties
        .iter()
        .map(|(k, v): (&String, &Value)| (k.clone(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPLICATION_PROPERTIES: &str = include_str!("../tests/resources/application.properties");

    // --- OrderedPropertiesTest ---

    #[test]
    fn test_ordered() {
        let mut prop = OrderedProperties::new();
        prop.set_property("c", "CCC");
        prop.set_property("d", "DDD");
        prop.set_property("e", "EEE");
        prop.set_property("b", "BBB");
        prop.set_property("a", "AAA");
        assert_eq!(5, prop.len());
        assert_eq!(
            vec!["c", "d", "e", "b", "a"],
            prop.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            vec!["CCC", "DDD", "EEE", "BBB", "AAA"],
            prop.values().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_ordered_load() {
        let mut prop = OrderedProperties::new();
        prop.load_reader(APPLICATION_PROPERTIES.as_bytes()).unwrap();
        assert_eq!(4, prop.len());
        assert_eq!(
            vec![
                "hello",
                "camel.component.seda.concurrent-consumers",
                "camel.component.seda.queueSize",
                "camel.component.direct.timeout"
            ],
            prop.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            vec!["World", "2", "500", "1234"],
            prop.values().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_contains_key() {
        let mut prop = OrderedProperties::new();
        prop.set_property("foo", "bar");
        assert!(prop.contains_key("foo"));
        assert!(!prop.contains_key("bar"));
    }

    #[test]
    fn test_contains_value() {
        let mut prop = OrderedProperties::new();
        prop.set_property("foo", "bar");
        assert!(prop.contains_value("bar"));
        assert!(!prop.contains_value("foo"));
    }

    #[test]
    fn test_contains_key_after_remove() {
        let mut prop = OrderedProperties::new();
        prop.set_property("foo", "bar");
        assert!(prop.contains_key("foo"));
        prop.remove("foo");
        assert!(!prop.contains_key("foo"));
    }

    #[test]
    fn test_map_methods() {
        let mut prop = OrderedLocationProperties::new();
        prop.put_location("app.properties", "b", "1");
        prop.put_location("app.properties", "a", "2");
        assert_eq!(vec!["b", "a"], prop.keys().collect::<Vec<_>>());
        assert_eq!("1", prop.get_or_default("b", "x"));
        assert_eq!("x", prop.get_or_default("c", "x"));
        assert_eq!(Some("1".to_string()), prop.put_if_absent("b", "3"));
        assert_eq!(None, prop.put_if_absent("c", "3"));
        assert_eq!(Some("3"), prop.get_property("c"));
        assert_eq!(
            Some("33".to_string()),
            prop.compute("c", |_, v| v.map(|v| format!("{v}3")))
        );
        assert_eq!(
            Some("4".to_string()),
            prop.compute_if_absent("d", |_| Some("4".into()))
        );
        assert_eq!(
            Some("44".to_string()),
            prop.merge("d", "4", |a, b| Some(format!("{a}{b}")))
        );
        assert_eq!(Some("2".to_string()), prop.replace("a", "22"));
        assert!(prop.replace_if("a", "22", "222"));
        assert_eq!(Some("222"), prop.get_property("a"));
        assert!(prop.remove_if_value("d", "44"));
        assert!(!prop.contains_key("d"));
        assert_eq!(vec!["1", "222", "33"], prop.values().collect::<Vec<_>>());

        let mut other = OrderedLocationProperties::new();
        other.put("b", "1");
        assert_ne!(prop, other);
        assert_eq!(prop, prop.clone());
    }

    // --- CamelCaseOrderedPropertiesTest ---

    #[test]
    fn test_camel_case_ordered() {
        let mut prop = CamelCaseOrderedProperties::new();
        prop.set_property("hello-world", "Hi Camel");
        prop.set_property("camel.main.stream-caching-enabled", "true");
        assert_eq!(2, prop.len());
        assert_eq!(
            vec!["hello-world", "camel.main.stream-caching-enabled"],
            prop.keys().collect::<Vec<_>>()
        );
        assert_eq!(vec!["Hi Camel", "true"], prop.values().collect::<Vec<_>>());
        assert_eq!(Some("Hi Camel"), prop.get_property("hello-world"));
        assert_eq!(Some("Hi Camel"), prop.get_property("helloWorld"));
        assert_eq!(
            Some("true"),
            prop.get_property("camel.main.stream-caching-enabled")
        );
        assert_eq!(
            Some("true"),
            prop.get_property("camel.main.streamCachingEnabled")
        );
        assert_eq!("Davs", prop.get_property_or("bye-world", "Davs"));
    }

    #[test]
    fn test_camel_case_ordered_load() {
        let mut prop = CamelCaseOrderedProperties::new();
        prop.load(APPLICATION_PROPERTIES).unwrap();
        assert_eq!(4, prop.len());
        assert_eq!(
            vec!["World", "2", "500", "1234"],
            prop.values().collect::<Vec<_>>()
        );
        let d = "MyDefault";
        assert_eq!("World", prop.get_property_or("hello", d));
        assert_eq!(
            "2",
            prop.get_property_or("camel.component.seda.concurrent-consumers", d)
        );
        assert_eq!(
            "2",
            prop.get_property_or("camel.component.seda.concurrentConsumers", d)
        );
        assert_eq!(
            "500",
            prop.get_property_or("camel.component.seda.queue-size", d)
        );
        assert_eq!(
            "500",
            prop.get_property_or("camel.component.seda.queueSize", d)
        );
        assert_eq!(
            "1234",
            prop.get_property_or("camel.component.direct.timeout", d)
        );
    }

    #[test]
    fn test_contains_key_camel_case_and_dash_fallback() {
        let mut prop = CamelCaseOrderedProperties::new();
        prop.set_property("camel.main.stream-caching-enabled", "true");
        prop.set_property("camel.component.seda.queueSize", "500");
        assert!(prop.contains_key("camel.main.stream-caching-enabled"));
        assert!(prop.contains_key("camel.main.streamCachingEnabled"));
        assert!(prop.contains_key("camel.component.seda.queueSize"));
        assert!(prop.contains_key("camel.component.seda.queue-size"));
        assert!(!prop.contains_key("camel.main.unknown"));
    }

    // --- PropertiesHelperTest ---

    #[test]
    fn test_extract_properties() {
        let mut params = ValueMap::new();
        params.insert("foo.name".into(), "Camel".into());
        params.insert("foo.age".into(), 5.into());
        params.insert("bar".into(), "yes".into());
        let foo = extract_properties_remove(&mut params, "foo.", true);
        assert_eq!(2, foo.len());
        assert_eq!("Camel", foo["name"]);
        assert_eq!(5, foo["age"]);
        assert_eq!(1, params.len());
        assert_eq!("yes", params["bar"]);
        assert!(has_properties(&params, ""));
        assert!(!has_properties(&params, "foo."));
        let kept = extract_properties_remove(&mut params, "b", false);
        assert_eq!(1, kept.len());
        assert_eq!(1, params.len());
        assert_eq!(
            Some("yes"),
            as_properties_from_map(&params).get_property("bar")
        );
        assert_eq!(
            Some("2"),
            as_properties([("a", "1"), ("b", "2")]).get_property("b")
        );
    }

    // --- .properties format (java.util.Properties.load semantics) ---

    #[test]
    fn test_format() {
        let text = "  # comment\n! other comment\n\nkey1=value1\nkey2 : value2\nkey3 value3\n\tkey4\t=\t value4 \nkey5\nkey6=\nmulti = one \\\n  two\\\n   three\nesc\\=key=a\\tb\\nc\\\\d\\u0041\nwin=a\r\nlast=x\\";
        let p = parse_properties(text).unwrap();
        let get = |k: &str| p.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(Some("value1"), get("key1"));
        assert_eq!(Some("value2"), get("key2"));
        assert_eq!(Some("value3"), get("key3"));
        assert_eq!(Some("value4 "), get("key4"));
        assert_eq!(Some(""), get("key5"));
        assert_eq!(Some(""), get("key6"));
        // as in Java, the leading whitespace of a continuation line is dropped
        assert_eq!(Some("one twothree"), get("multi"));
        assert_eq!(Some("a\tb\nc\\dA"), get("esc=key"));
        assert_eq!(Some("a"), get("win"));
        assert_eq!(Some("x"), get("last"));
        assert!(parse_properties("bad=\\u12").is_err());
        assert_eq!(
            Some("😀"),
            parse_properties("e=\\ud83d\\ude00")
                .unwrap()
                .first()
                .map(|(_, v)| v.as_str())
        );
        // comment marker inside a continuation is data
        assert_eq!("a#b", parse_properties("k=a\\\n#b").unwrap()[0].1);
    }

    #[test]
    fn test_store_round_trip() {
        let mut prop = OrderedProperties::new();
        prop.set_property("key with spaces", " leading space");
        prop.set_property("a=b", "c:d#e!f\\g\th\ni");
        let stored = prop.store();
        let mut loaded = OrderedProperties::new();
        loaded.load(&stored).unwrap();
        assert_eq!(prop, loaded);
        assert_eq!(
            "{key with spaces= leading space, a=b=c:d#e!f\\g\th\ni}",
            prop.to_string()
        );
    }

    #[test]
    fn test_location_properties() {
        let mut prop = OrderedLocationProperties::new();
        prop.put_location_default("env", "a", "1", Some("0"));
        prop.put_all_location("file", [("b", "2")]);
        assert_eq!(Some("env"), prop.get_location("a"));
        assert_eq!(Some("0"), prop.get_default_value("a"));
        assert_eq!(Some("file"), prop.get_location("b"));
        let mut copy = OrderedLocationProperties::new();
        copy.put_all_from(&prop);
        assert_eq!(prop, copy);
        assert_eq!(Some("1".to_string()), copy.remove("a"));
        assert_eq!(None, copy.get_location("a"));
        copy.clear();
        assert!(copy.is_empty());
        assert_eq!(None, copy.get_location("b"));
        let mut p = OrderedProperties::new();
        p.put_all([("x", 1), ("y", 2)]);
        p.replace_all(|k, v| format!("{k}{v}"));
        assert_eq!(Some("x1"), p.get_property("x"));
        assert_eq!(
            Some("y2!".to_string()),
            p.compute_if_present("y", |_, v| Some(format!("{v}!")))
        );
        assert_eq!(None, p.compute_if_present("z", |_, v| Some(v.to_string())));
        assert_eq!(None, p.merge("x", "?", |_, _| None));
        assert!(!p.contains_key("x"));
        assert!(!p.as_map().is_empty());
    }
}
