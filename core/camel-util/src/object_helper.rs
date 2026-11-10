//! A number of useful helper methods for working with objects.
//!
//! Port of the portable part of `org.apache.camel.util.ObjectHelper`. The Java class also
//! contained reflection, annotation and class loading helpers (`loadClass`,
//! `isOverridingMethod`, `findMethodsWithAnnotation`, `getPropertyName`, ...). Those have no
//! Rust equivalent: Rust has no runtime reflection, and the port registers components,
//! languages and beans explicitly instead of discovering them on the class path.

use crate::system_properties;
use crate::value::Value;
use std::error::Error;

/// A helper method for comparing values for equality while handling nulls.
///
/// Byte arrays and lists are compared by content.
pub fn equal(a: &Value, b: &Value) -> bool {
    equal_ignore_case_opt(a, b, false)
}

/// A helper method for comparing values for equality while handling case insensitivity of
/// texts.
pub fn equal_ignore_case(a: &Value, b: &Value) -> bool {
    equal_ignore_case_opt(a, b, true)
}

/// A helper method for comparing values for equality while handling nulls.
///
/// # Arguments
///
/// * `a` - the first value
/// * `b` - the second value
/// * `ignore_case` - whether two texts are compared ignoring case
pub fn equal_ignore_case_opt(a: &Value, b: &Value, ignore_case: bool) -> bool {
    if ignore_case {
        if let (Value::Text(x), Value::Text(y)) = (a, b) {
            return x.to_lowercase() == y.to_lowercase();
        }
    }
    a == b
}

/// A helper method for comparing byte arrays for equality while handling nulls.
pub fn equal_byte_array(a: Option<&[u8]>, b: Option<&[u8]>) -> bool {
    a == b
}

/// Returns true if the given value is equal to any of the expected values.
pub fn is_equal_to_any(value: &Value, values: &[Value]) -> bool {
    values.iter().any(|v| equal(value, v))
}

/// Converts the value to a boolean.
///
/// Booleans are returned as is; texts and bytes are converted only if they are `true` or
/// `false` (ignoring case); integers are `true` when greater than zero.
///
/// # Returns
///
/// The boolean, or `None` if the value cannot be converted.
pub fn to_boolean(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Bytes(bytes) => {
            let s = String::from_utf8_lossy(bytes);
            is_boolean(&s).then(|| s.eq_ignore_ascii_case("true"))
        }
        Value::Text(s) => is_boolean(s).then(|| s.eq_ignore_ascii_case("true")),
        Value::Int(i) => Some(*i > 0),
        _ => None,
    }
}

/// Asserts whether the value is **not** `None`.
///
/// # Arguments
///
/// * `value` - the value to test
/// * `name` - the key that resolved the value
///
/// # Returns
///
/// The passed value as is.
///
/// # Errors
///
/// Returns `"<name> must be specified"` if the value is `None` (the Java version threw
/// `IllegalArgumentException`).
pub fn not_null<T>(value: Option<T>, name: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("{name} must be specified"))
}

/// Asserts whether the value is **not** `None`.
///
/// # Arguments
///
/// * `value` - the value to test
/// * `name` - the key that resolved the value
/// * `on` - additional description to indicate where this problem occurred
///
/// # Errors
///
/// Returns `"<name> must be specified on: <on>"` if the value is `None`.
pub fn not_null_on<T>(value: Option<T>, name: &str, on: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("{name} must be specified on: {on}"))
}

/// Asserts that the given value is neither `None` nor an empty string.
///
/// # Errors
///
/// Returns `"<name> must be specified and non-empty"` if the assertion fails.
pub fn not_null_or_empty<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("{name} must be specified and non-empty")),
    }
}

/// Tests whether the value is `null`, a blank string or an empty collection/map.
///
/// # Arguments
///
/// * `value` - the value, if it is a text it will be tested for blank text as well
///
/// # Returns
///
/// `true` if empty.
///
/// # Examples
///
/// ```
/// use camel_util::{object_helper::is_empty, value::Value};
/// assert!(is_empty(&Value::Null));
/// assert!(is_empty(&Value::from(" ")));
/// assert!(!is_empty(&Value::from(" A ")));
/// assert!(is_empty(&Value::List(vec![])));
/// ```
pub fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Text(s) => is_empty_str(Some(s)),
        Value::List(l) => l.is_empty(),
        Value::Map(m) => m.is_empty(),
        _ => false,
    }
}

/// Tests whether the value is **not** `null`, a blank string or an empty collection/map.
pub fn is_not_empty(value: &Value) -> bool {
    !is_empty(value)
}

/// Tests whether the string is `None` or blank (only whitespace).
pub fn is_empty_str(value: Option<&str>) -> bool {
    value.map(|s| s.trim().is_empty()).unwrap_or(true)
}

/// Tests whether the string is **not** `None` or blank.
pub fn is_not_empty_str(value: Option<&str>) -> bool {
    !is_empty_str(value)
}

/// Returns the first non null value.
pub fn first_not_null(values: &[Value]) -> Option<&Value> {
    values.iter().find(|v| !v.is_null())
}

/// Returns the value if it is not empty, otherwise the value from the supplier.
pub fn supply_if_empty(value: Value, supplier: impl FnOnce() -> Value) -> Value {
    if is_not_empty(&value) {
        value
    } else {
        supplier()
    }
}

/// Invokes the consumer with the value if the value is not empty.
pub fn if_not_empty(value: &Value, consumer: impl FnOnce(&Value)) {
    if is_not_empty(value) {
        consumer(value);
    }
}

/// Returns the predicate matching boolean on a list result set where if the first element is
/// a boolean its value is used, otherwise this method returns true if the collection is not
/// empty.
///
/// # Returns
///
/// `true` if the first element is a boolean and its value is true, or if the list is
/// non-empty and the first element is not a boolean.
pub fn matches(list: &[Value]) -> bool {
    match list.first() {
        None => false,
        Some(Value::Bool(b)) => *b,
        // lets assume non-empty results are true
        Some(_) => true,
    }
}

/// A helper method to access a system property (see [`crate::system_properties`]).
///
/// # Arguments
///
/// * `name` - the name of the system property required
/// * `default_value` - the default value to use if the property is not available
///
/// # Returns
///
/// The system property value or the default value if the property is not available.
pub fn get_system_property(name: &str, default_value: &str) -> String {
    system_properties::get_property(name).unwrap_or_else(|| default_value.to_string())
}

/// A helper method to access a boolean system property.
///
/// # Returns
///
/// `true` if the property (or the default value when not set) is `true` ignoring case
/// (Java `Boolean.parseBoolean`).
pub fn get_system_property_bool(name: &str, default_value: bool) -> bool {
    get_system_property(name, &default_value.to_string()).eq_ignore_ascii_case("true")
}

/// Returns the type name of the value, or `None` if the value is null (Java `className`).
pub fn class_name(value: &Value) -> Option<&'static str> {
    (!value.is_null()).then(|| value.type_name())
}

/// Turns the given values into a meaningful string such as `{foo, bar}`.
pub fn as_string(objects: Option<&[Value]>) -> String {
    match objects {
        None => "null".to_string(),
        Some(objects) => {
            let inner: Vec<String> = objects.iter().map(Value::to_string).collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Evaluates the value as a predicate which attempts to convert the value to a boolean,
/// otherwise `true` is returned if the value is not null.
///
/// Texts are `false` when blank or `false` (ignoring case), and `true` otherwise; lists and
/// maps are `true` when not empty.
///
/// # Examples
///
/// ```
/// use camel_util::{object_helper::evaluate_value_predicate, value::Value};
/// assert!(evaluate_value_predicate(&Value::from(123)));
/// assert!(!evaluate_value_predicate(&Value::from("FALSE")));
/// assert!(evaluate_value_predicate(&Value::from("foobar")));
/// assert!(!evaluate_value_predicate(&Value::from(" ")));
/// assert!(!evaluate_value_predicate(&Value::Null));
/// ```
pub fn evaluate_value_predicate(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Text(s) => {
            let s = s.trim();
            !(s.is_empty() || s.eq_ignore_ascii_case("false"))
        }
        Value::List(l) => !l.is_empty(),
        Value::Map(m) => !m.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

/// The maximum number of causes followed by [`create_exception_iterable`].
pub const MAX_CAUSE_DEPTH: usize = 1024;

/// Creates a list to walk the error from the bottom up (the last caused by going upwards to
/// the root error), following [`Error::source`].
///
/// # Arguments
///
/// * `error` - the error
///
/// # Returns
///
/// The errors, starting with the deepest cause.
pub fn create_exception_iterable<'a>(
    error: &'a (dyn Error + 'static),
) -> Vec<&'a (dyn Error + 'static)> {
    let mut list: Vec<&'a (dyn Error + 'static)> = Vec::new();
    let mut current: Option<&'a (dyn Error + 'static)> = Some(error);
    while let Some(e) = current {
        // The Java version stopped when a cause referred back to an exception in the chain.
        // A cycle cannot be built through `Error::source` with safe borrows, and comparing
        // addresses is unreliable in Rust (a zero-sized cause shares its wrapper's address),
        // so the walk is bounded by depth instead.
        if list.len() >= MAX_CAUSE_DEPTH {
            break;
        }
        list.push(e);
        current = e.source();
    }
    list.reverse();
    list
}

/// Retrieves the given error type from the error.
///
/// Is used to get the caused error that typically have been wrapped in some sort of Camel
/// wrapper error. The strategy is to look in the error hierarchy to find the first given
/// cause that matches the type, starting from the bottom (the real cause) and walking
/// upwards.
///
/// # Returns
///
/// The error found, or `None` if not found in the error hierarchy.
pub fn get_exception<'a, T: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a T> {
    create_exception_iterable(error)
        .into_iter()
        .find_map(|e| e.downcast_ref::<T>())
}

/// Is the given value a numeric NaN value.
pub fn is_nan(value: &Value) -> bool {
    matches!(value, Value::Float(f) if f.is_nan())
}

/// Turns the optional values into a list (empty for `None`).
pub fn as_list(objects: Option<&[Value]>) -> Vec<Value> {
    objects.map(<[Value]>::to_vec).unwrap_or_default()
}

/// Adds the value to the list at the given index, padding with nulls if the index is beyond
/// the end of the list.
pub fn add_list_by_index(list: &mut Vec<Value>, idx: usize, value: Value) {
    if idx < list.len() {
        list[idx] = value;
    } else {
        list.resize(idx, Value::Null);
        list.push(value);
    }
}

/// Checks whether the given string is a valid boolean value (either `true` or `false`)
/// ignoring its case.
pub fn is_boolean(s: &str) -> bool {
    s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ObjectRef;
    use std::fmt;

    #[test]
    fn test_equal() {
        let v = Value::from;
        assert!(equal(&Value::Null, &Value::Null));
        assert!(equal(&v(""), &v("")));
        assert!(equal(&v(" "), &v(" ")));
        assert!(equal(&v("Hello"), &v("Hello")));
        assert!(equal(&Value::from(123), &Value::from(123)));
        assert!(equal(&Value::from(true), &Value::from(true)));
        assert!(!equal(&Value::Null, &v("")));
        assert!(!equal(&v(""), &Value::Null));
        assert!(!equal(&v(" "), &v("    ")));
        assert!(!equal(&v("Hello"), &v("World")));
        assert!(!equal(&Value::from(true), &Value::from(false)));
        assert!(!equal(&Value::object(1u8), &Value::object(1u8)));
        assert!(equal(
            &Value::from(vec![40u8, 50, 60]),
            &Value::from(vec![40u8, 50, 60])
        ));
        assert!(!equal(
            &Value::from(vec![40u8, 50, 60]),
            &Value::from(vec![40u8, 50, 60, 70])
        ));
        assert!(equal_ignore_case(&v("HELLO"), &v("hello")));
        assert!(is_equal_to_any(&v("b"), &[v("a"), v("b")]));
    }

    #[test]
    fn test_equal_byte_array() {
        let e = equal_byte_array;
        assert!(e(Some(b"Hello"), Some(b"Hello")));
        assert!(!e(Some(b"Hello"), Some(b"World")));
        let thai = "Hello Thai Elephant \u{0E08}".as_bytes();
        assert!(e(Some(thai), Some(thai)));
        assert!(e(None, None));
        assert!(e(Some(&[]), Some(&[])));
        assert!(e(Some(&[40, 50, 60]), Some(&[40, 50, 60])));
        assert!(!e(Some(&[40, 50, 60]), Some(&[40, 50, 60, 70])));
        assert!(!e(Some(&[40, 50, 60, 70]), Some(&[40, 50, 60])));
        assert!(!e(Some(&[40, 50, 60]), Some(&[])));
        assert!(!e(Some(&[]), Some(&[40, 50, 60])));
        assert!(!e(Some(&[40, 50, 60]), None));
        assert!(!e(None, Some(&[40, 50, 60])));
    }

    #[test]
    fn test_is_empty() {
        assert!(is_empty(&Value::Null));
        assert!(is_empty(&Value::from("")));
        assert!(is_empty(&Value::from(" ")));
        assert!(!is_empty(&Value::from("A")));
        assert!(!is_empty(&Value::from(" A")));
        assert!(!is_empty(&Value::from(" A ")));
        assert!(!is_empty(&Value::object(())));
        assert!(is_empty_str(None));
        assert!(is_not_empty_str(Some("x")));
    }

    #[test]
    fn test_is_not_empty() {
        assert!(!is_not_empty(&Value::Null));
        assert!(!is_not_empty(&Value::from("")));
        assert!(!is_not_empty(&Value::from(" ")));
        assert!(is_not_empty(&Value::from("A")));
        assert!(is_not_empty(&Value::from(" A")));
        assert!(is_not_empty(&Value::from(" A ")));
        assert!(is_not_empty(&Value::object(())));
    }

    #[test]
    fn test_evaluate_as_predicate() {
        assert!(!evaluate_value_predicate(&Value::Null));
        assert!(evaluate_value_predicate(&Value::from(123)));
        for (s, expected) in [
            ("true", true),
            ("TRUE", true),
            ("false", false),
            ("FALSE", false),
            ("foobar", true),
            ("", false),
            (" ", false),
        ] {
            assert_eq!(expected, evaluate_value_predicate(&Value::from(s)), "{s}");
        }
        let mut list = Value::List(vec![]);
        assert!(!evaluate_value_predicate(&list));
        list.as_list_mut().unwrap().push("foo".into());
        assert!(evaluate_value_predicate(&list));
    }

    #[test]
    fn test_as_string() {
        assert_eq!(
            "{foo, bar}",
            as_string(Some(&[Value::from("foo"), Value::from("bar")]))
        );
        assert_eq!("null", as_string(None));
    }

    #[test]
    fn test_class_name() {
        assert_eq!(Some("Long"), class_name(&Value::from(5)));
        assert_eq!(None, class_name(&Value::Null));
    }

    #[test]
    fn test_get_system_property_default() {
        assert_eq!("foo", get_system_property("CamelFooDoesNotExist", "foo"));
    }

    #[test]
    fn test_get_system_property_boolean_default() {
        assert!(get_system_property_bool("CamelFooDoesNotExist", true));
    }

    #[test]
    fn test_matches() {
        assert!(matches(&[Value::from("foo"), Value::from("bar")]));
        assert!(!matches(&[Value::from(false), Value::from("bar")]));
        assert!(!matches(&[]));
    }

    #[test]
    fn test_to_boolean() {
        assert_eq!(Some(true), to_boolean(&Value::from(true)));
        assert_eq!(Some(true), to_boolean(&Value::from("true")));
        assert_eq!(Some(true), to_boolean(&Value::from(1)));
        assert_eq!(Some(false), to_boolean(&Value::from(0)));
        assert_eq!(Some(false), to_boolean(&Value::from(b"FALSE".to_vec())));
        assert_eq!(None, to_boolean(&Value::from("yes")));
        assert_eq!(
            None,
            to_boolean(&Value::object(std::time::SystemTime::now()))
        );
    }

    #[test]
    fn test_identity_hash_code() {
        let dummy = ObjectRef::new("Camel".to_string());
        assert_eq!(
            dummy.identity_hash_code(),
            dummy.clone().identity_hash_code()
        );
        let dummy_b = ObjectRef::new("Camel".to_string());
        assert_ne!(dummy.identity_hash_code(), dummy_b.identity_hash_code());
    }

    #[test]
    fn test_is_nan() {
        assert!(is_nan(&Value::from(f32::NAN)));
        assert!(is_nan(&Value::from(f64::NAN)));
        for v in [
            Value::Null,
            Value::from(""),
            Value::from("1.0"),
            Value::from(1),
            Value::from(1.5f32),
            Value::from(1.5),
            Value::from(false),
            Value::from(true),
        ] {
            assert!(!is_nan(&v), "{v}");
        }
    }

    #[test]
    fn test_not_null() {
        assert_eq!(Ok(3), not_null(Some(3), "expected"));
        assert_eq!(Ok(3), not_null_on(Some(3), "expected", "holder"));
        assert_eq!(
            Err("expected2 must be specified".to_string()),
            not_null(None::<i64>, "expected2")
        );
        assert_eq!(
            Err("expected2 must be specified on: holder".to_string()),
            not_null_on(None::<i64>, "expected2", "holder")
        );
        assert!(not_null_or_empty(Some(""), "x").is_err());
        assert_eq!(Ok("y"), not_null_or_empty(Some("y"), "x"));
    }

    #[test]
    fn test_as_list() {
        assert!(as_list(None).is_empty());
        assert!(as_list(Some(&[])).is_empty());
        let out = as_list(Some(&[Value::from("foo"), Value::from("bar")]));
        assert_eq!(2, out.len());
        assert_eq!("foo", out[0]);
        assert_eq!("bar", out[1]);
    }

    #[test]
    fn test_add_list_by_index() {
        let mut list = Vec::new();
        add_list_by_index(&mut list, 0, "aaa".into());
        add_list_by_index(&mut list, 2, "ccc".into());
        add_list_by_index(&mut list, 1, "bbb".into());
        assert_eq!(3, list.len());
        assert_eq!("aaa", list[0]);
        assert_eq!("bbb", list[1]);
        assert_eq!("ccc", list[2]);
        add_list_by_index(&mut list, 99, "zzz".into());
        assert_eq!(100, list.len());
        assert!(list[4].is_null());
        assert!(list[50].is_null());
        assert!(list[98].is_null());
        assert_eq!("zzz", list[99]);
    }

    #[test]
    fn test_misc() {
        assert_eq!(
            Some(&Value::from(1)),
            first_not_null(&[Value::Null, Value::from(1)])
        );
        assert_eq!(
            Value::from("x"),
            supply_if_empty(Value::from(" "), || "x".into())
        );
        let mut called = false;
        if_not_empty(&Value::from("a"), |_| called = true);
        assert!(called);
        assert!(is_boolean("TRUE"));
        assert!(!is_boolean("yes"));
    }

    #[derive(Debug)]
    struct Wrapper(Inner);
    #[derive(Debug)]
    struct Inner;
    impl fmt::Display for Wrapper {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("wrapper")
        }
    }
    impl fmt::Display for Inner {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("inner")
        }
    }
    impl Error for Wrapper {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }
    impl Error for Inner {}

    #[test]
    fn test_exception_iterable() {
        let e = Wrapper(Inner);
        let list = create_exception_iterable(&e);
        assert_eq!(
            vec!["inner", "wrapper"],
            list.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        assert!(get_exception::<Inner>(&e).is_some());
        assert!(get_exception::<Wrapper>(&e).is_some());
        assert!(get_exception::<std::fmt::Error>(&e).is_none());
    }
}
