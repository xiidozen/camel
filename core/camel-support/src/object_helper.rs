//! Value comparison with type coercion (the `typeCoerce*` and `compare` helpers of Java
//! `org.apache.camel.support.ObjectHelper`), used by the predicates of the DSL and the
//! languages: `"123"` equals `123`, `"true"` equals `true`, and numbers of different types are
//! compared by value.
//!
//! The Java type converter is replaced by the [`FromValue`] conversions of [`Value`]. Java
//! `BigInteger` values (numbers too large for a `long`) are compared as `i128`.

use camel_api::convert::FromValue;
use camel_util::value::Value;
use std::cmp::Ordering;

/// Whether the text is an integer number: an optional leading `-` followed by digits.
///
/// # Examples
///
/// ```
/// use camel_support::object_helper::is_number;
/// assert!(is_number("-123"));
/// assert!(!is_number("12.3"));
/// assert!(!is_number("-"));
/// ```
pub fn is_number(text: &str) -> bool {
    let Some(start) = start_position(text) else {
        return false;
    };
    text[start..].chars().all(|c| c.is_ascii_digit())
}

fn start_position(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    if text.starts_with('-') {
        if text.len() == 1 {
            return None;
        }
        return Some(1);
    }
    Some(0)
}

/// Whether the text is a floating point number: an optional leading `-`, digits with at most
/// one dot, not starting or ending with the dot.
///
/// # Examples
///
/// ```
/// use camel_support::object_helper::is_floating_number;
/// assert!(is_floating_number("-1.5"));
/// assert!(is_floating_number("15"));
/// assert!(!is_floating_number("1."));
/// assert!(!is_floating_number("1.2.3"));
/// ```
pub fn is_floating_number(text: &str) -> bool {
    let Some(start) = start_position(text) else {
        return false;
    };
    let mut dots = false;
    let mut digits = false;
    let mut last = '\0';
    for ch in text[start..].chars() {
        last = ch;
        if ch.is_ascii_digit() {
            digits = true;
        } else if ch == '.' {
            if dots {
                return false;
            }
            dots = true;
        } else {
            return false;
        }
    }
    digits && !text[start..].starts_with('.') && last != '.'
}

fn to_long(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
}

fn to_boolean(text: &str) -> Option<bool> {
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn try_convert<T: FromValue>(value: &Value) -> Option<T> {
    T::from_value(value).ok().flatten()
}

/// Converts the value to the type (variant) of the target, if possible.
fn convert_like(target: &Value, value: &Value) -> Option<Value> {
    match target {
        Value::Text(_) => try_convert::<String>(value).map(Value::Text),
        Value::Int(_) => try_convert::<i64>(value).map(Value::Int),
        Value::Float(_) => try_convert::<f64>(value).map(Value::Float),
        Value::Bool(_) => try_convert::<bool>(value).map(Value::Bool),
        Value::Bytes(_) => try_convert::<Vec<u8>>(value).map(Value::Bytes),
        _ => None,
    }
}

fn same_variant(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn equal(a: &Value, b: &Value, ignore_case: bool) -> bool {
    camel_util::object_helper::equal_ignore_case_opt(a, b, ignore_case)
}

fn compare_doubles(a: f64, b: f64) -> Ordering {
    // Java Double.compareTo: total order (NaN is the largest, -0.0 < 0.0)
    a.total_cmp(&b)
}

/// Whether the values are equal, converting one to the type of the other when they differ
/// (Java `ObjectHelper.typeCoerceEquals`).
///
/// # Examples
///
/// ```
/// use camel_support::object_helper::type_coerce_equals;
/// use camel_util::value::Value;
/// assert!(type_coerce_equals(&Value::from("123"), &Value::from(123), false));
/// assert!(type_coerce_equals(&Value::from(true), &Value::from("TRUE"), false));
/// assert!(type_coerce_equals(&Value::from(2.5), &Value::from("2.5"), false));
/// assert!(type_coerce_equals(&Value::from("Hello"), &Value::from("hello"), true));
/// assert!(!type_coerce_equals(&Value::from("Hello"), &Value::Null, false));
/// ```
pub fn type_coerce_equals(left: &Value, right: &Value, ignore_case: bool) -> bool {
    if left.is_null() || right.is_null() {
        return left.is_null() && right.is_null();
    }
    match (left, right) {
        (Value::Text(l), Value::Text(r)) => {
            if is_number(l) && is_number(r) {
                return match (to_long(l), to_long(r)) {
                    (Some(a), Some(b)) => a == b,
                    _ => {
                        l.parse::<i128>()
                            .ok()
                            .zip(r.parse::<i128>().ok())
                            .is_some_and(|(a, b)| a == b)
                            || l == r
                    }
                };
            }
            if ignore_case {
                l.to_lowercase() == r.to_lowercase()
            } else {
                l == r
            }
        }
        (Value::Text(l), Value::Int(r)) if is_number(l) => to_long(l) == Some(*r),
        (Value::Text(l), Value::Float(r)) if is_floating_number(l) => l
            .parse::<f64>()
            .is_ok_and(|v| compare_doubles(v, *r) == Ordering::Equal),
        (Value::Text(l), Value::Bool(r)) => to_boolean(l) == Some(*r),
        (Value::Int(l), Value::Text(r)) if is_number(r) => to_long(r) == Some(*l),
        (Value::Int(l), Value::Int(r)) => l == r,
        (Value::Float(l), Value::Float(r)) => compare_doubles(*l, *r) == Ordering::Equal,
        (Value::Bool(l), Value::Text(r)) => to_boolean(r) == Some(*l),
        (Value::Int(l), Value::Float(r)) => (*l as f64) == *r,
        (Value::Float(l), Value::Int(r)) => *l == (*r as f64),
        _ => try_converters(left, right, ignore_case),
    }
}

fn try_converters(left: &Value, right: &Value, ignore_case: bool) -> bool {
    if equal(left, right, ignore_case) {
        return true;
    }
    if same_variant(left, right) {
        return false;
    }
    if let Some(v) = convert_like(right, left) {
        if equal(&v, right, ignore_case) {
            return true;
        }
    }
    convert_like(left, right).is_some_and(|v| equal(left, &v, ignore_case))
}

/// Whether the values are not equal (see [`type_coerce_equals`]).
pub fn type_coerce_not_equals(left: &Value, right: &Value) -> bool {
    !type_coerce_equals(left, right, false)
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Text(t) if is_number(t) || is_floating_number(t) => t.parse::<f64>().ok(),
        _ => None,
    }
}

/// Compares the values, converting one to the type of the other when they differ (Java
/// `ObjectHelper.typeCoerceCompare`).
///
/// # Examples
///
/// ```
/// use camel_support::object_helper::type_coerce_compare;
/// use camel_util::value::Value;
/// use std::cmp::Ordering;
/// assert_eq!(Ordering::Less, type_coerce_compare(&Value::from("9"), &Value::from("10")));
/// assert_eq!(Ordering::Greater, type_coerce_compare(&Value::from(20), &Value::from("3")));
/// assert_eq!(Ordering::Less, type_coerce_compare(&Value::from("abc"), &Value::from("abd")));
/// ```
pub fn type_coerce_compare(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Text(l), Value::Text(r)) => compare_string_string(l, r),
        (Value::Int(l), Value::Int(r)) => l.cmp(r),
        (Value::Float(l), Value::Float(r)) => compare_doubles(*l, *r),
        (Value::Text(l), Value::Int(r)) if is_number(l) => match to_long(l) {
            Some(v) => v.cmp(r),
            None => l
                .parse::<i128>()
                .map(|v| v.cmp(&(*r as i128)))
                .unwrap_or(Ordering::Greater),
        },
        (Value::Int(l), Value::Text(r)) if is_number(r) => match to_long(r) {
            Some(v) => l.cmp(&v),
            None => r
                .parse::<i128>()
                .map(|v| (*l as i128).cmp(&v))
                .unwrap_or(Ordering::Less),
        },
        (Value::Text(l), Value::Float(r)) if is_floating_number(l) => {
            compare_doubles(l.parse().unwrap_or(0.0), *r)
        }
        (Value::Text(l), Value::Bool(r)) if to_boolean(l).is_some() => {
            to_boolean(l).unwrap().cmp(r)
        }
        (Value::Bool(l), Value::Text(r)) if to_boolean(r).is_some() => {
            l.cmp(&to_boolean(r).unwrap())
        }
        _ => {
            if let (Some(a), Some(b)) = (as_number(left), as_number(right)) {
                if let (Value::Int(x), Value::Int(y)) = (left, right) {
                    return x.cmp(y);
                }
                return compare_doubles(a, b);
            }
            if let (Some(a), Some(b)) = (try_convert::<i64>(left), try_convert::<i64>(right)) {
                return a.cmp(&b);
            }
            if let (Some(a), Some(b)) = (try_convert::<f64>(left), try_convert::<f64>(right)) {
                return compare_doubles(a, b);
            }
            if matches!(right, Value::Text(_)) && !matches!(left, Value::Text(_)) {
                return type_coerce_compare(right, left).reverse();
            }
            if let Some(v) = convert_like(right, left) {
                return compare(&v, right, false);
            }
            if let Some(v) = convert_like(left, right) {
                return compare(left, &v, false);
            }
            compare(left, right, false)
        }
    }
}

fn compare_string_string(l: &str, r: &str) -> Ordering {
    if is_number(l) && is_number(r) {
        return match (to_long(l), to_long(r)) {
            (Some(a), Some(b)) => a.cmp(&b),
            _ => match (l.parse::<i128>(), r.parse::<i128>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                _ => l.cmp(r),
            },
        };
    }
    if is_floating_number(l) && is_floating_number(r) {
        if let (Ok(a), Ok(b)) = (l.parse::<f64>(), r.parse::<f64>()) {
            return compare_doubles(a, b);
        }
    }
    java_string_compare(l, r)
}

/// Compares two strings like Java `String.compareTo` (by UTF-16 code units).
pub fn java_string_compare(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Compares the values without type coercion (Java `ObjectHelper.compare`): `Null` first,
/// values of the same type by their natural order, others by their type name.
pub fn compare(a: &Value, b: &Value, ignore_case: bool) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Text(x), Value::Text(y)) if ignore_case => {
            java_string_compare(&x.to_lowercase(), &y.to_lowercase())
        }
        (Value::Text(x), Value::Text(y)) => java_string_compare(x, y),
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => compare_doubles(*x, *y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Bytes(x), Value::Bytes(y)) => x.cmp(y),
        _ => a.type_name().cmp(b.type_name()),
    }
}

/// Whether the collection (a list, or a text containing the value as a sub text) contains the
/// value (Java `ObjectHelper.typeCoerceContains`).
///
/// # Examples
///
/// ```
/// use camel_support::object_helper::type_coerce_contains;
/// use camel_util::value::Value;
/// assert!(type_coerce_contains(&Value::from("Hello World"), &Value::from("World"), false));
/// assert!(type_coerce_contains(&Value::from("Hello World"), &Value::from("WORLD"), true));
/// assert!(type_coerce_contains(&Value::List(vec![Value::from(1), Value::from(2)]), &Value::from("2"), false));
/// assert!(type_coerce_contains(&Value::from("a,b,c"), &Value::from(Value::List(vec![])), false) == false);
/// ```
pub fn type_coerce_contains(collection: &Value, value: &Value, ignore_case: bool) -> bool {
    let collection = match collection {
        Value::Bytes(b) => Value::Text(String::from_utf8_lossy(b).into_owned()),
        other => other.clone(),
    };
    match &collection {
        Value::List(items) => {
            if !ignore_case && items.contains(value) {
                return true;
            }
            items
                .iter()
                .any(|item| type_coerce_equals(value, item, ignore_case))
        }
        Value::Text(text) => {
            if let Some(sub) = try_convert::<String>(value) {
                return if ignore_case {
                    text.to_lowercase().contains(&sub.to_lowercase())
                } else {
                    text.contains(&sub)
                };
            }
            text.split(',')
                .any(|part| type_coerce_equals(value, &Value::from(part), ignore_case))
        }
        Value::Map(m) => m
            .values()
            .any(|item| type_coerce_equals(value, item, ignore_case)),
        other => type_coerce_equals(value, other, ignore_case),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v<T: Into<Value>>(x: T) -> Value {
        x.into()
    }

    #[test]
    fn equals() {
        assert!(type_coerce_equals(&Value::Null, &Value::Null, false));
        assert!(type_coerce_equals(&v("00123"), &v(123), false));
        assert!(type_coerce_equals(&v(123), &v("123"), false));
        assert!(!type_coerce_equals(&v(123), &v("abc"), false));
        assert!(
            type_coerce_equals(&v("123"), &v("0123"), false),
            "numbers compared as numbers"
        );
        assert!(type_coerce_equals(
            &v("99999999999999999999"),
            &v("99999999999999999999"),
            false
        ));
        assert!(type_coerce_equals(&v(1), &v(1.0), false));
        assert!(type_coerce_equals(&v("false"), &v(false), false));
        assert!(!type_coerce_equals(&v("yes"), &v(true), false));
        assert!(type_coerce_equals(
            &Value::Bytes(b"abc".to_vec()),
            &v("abc"),
            false
        ));
        assert!(type_coerce_not_equals(&v("a"), &v("b")));
    }

    #[test]
    fn compares() {
        assert_eq!(Ordering::Less, type_coerce_compare(&v("1.5"), &v("2")));
        assert_eq!(Ordering::Equal, type_coerce_compare(&v(5), &v(5.0)));
        assert_eq!(Ordering::Less, type_coerce_compare(&v(false), &v("true")));
        assert_eq!(Ordering::Greater, type_coerce_compare(&v("b"), &v("a")));
        assert_eq!(Ordering::Less, compare(&Value::Null, &v(1), false));
        assert_eq!(Ordering::Equal, compare(&v("ABC"), &v("abc"), true));
        assert_eq!(Ordering::Less, java_string_compare("Z", "a"));
    }

    #[test]
    fn numbers() {
        assert!(is_number("0") && !is_number("") && !is_number("1a") && !is_number("+1"));
        assert!(is_floating_number("0.5") && !is_floating_number(".5") && !is_floating_number("-"));
    }

    #[test]
    fn contains() {
        assert!(type_coerce_contains(&v("a,b,c"), &v("b"), false));
        assert!(!type_coerce_contains(&v("abc"), &v("x"), false));
        assert!(type_coerce_contains(
            &Value::Bytes(b"Hello".to_vec()),
            &v("ell"),
            false
        ));
        assert!(type_coerce_contains(
            &Value::List(vec![v("A")]),
            &v("a"),
            true
        ));
    }
}
