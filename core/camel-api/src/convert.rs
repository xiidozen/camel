//! Conversion of dynamic [`Value`]s into Rust types.
//!
//! Java Camel converts message bodies and headers with `TypeConverter.convertTo(Class, Object)`.
//! In Rust the target type is a type parameter: anything implementing [`FromValue`] can be
//! requested, for example `message.body_as::<String>()`. The built-in conversions cover the
//! conversions of Camel's core type converters for text, numbers, booleans and bytes; the type
//! converter registry of the engine adds custom conversions on top of these.

use crate::error::{CamelError, CamelResult};
use camel_util::value::{java_double_to_string, Value};

/// A Rust type that can be converted from a [`Value`].
pub trait FromValue: Sized {
    /// The type name used in error messages (such as `String`, `Long`).
    fn type_name() -> &'static str;

    /// Converts the value.
    ///
    /// # Returns
    ///
    /// `Ok(None)` if the value is null, `Ok(Some(..))` if converted.
    ///
    /// # Errors
    ///
    /// Returns a `NoTypeConversionAvailableException` style error if the value cannot be
    /// converted.
    fn from_value(value: &Value) -> CamelResult<Option<Self>>;
}

fn no_conversion<T: FromValue>(value: &Value) -> CamelError {
    CamelError::no_type_conversion_available(
        (!value.is_null()).then(|| value.type_name()),
        T::type_name(),
    )
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Text(s) => Some(s.clone()),
        Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

impl FromValue for Value {
    fn type_name() -> &'static str {
        "Object"
    }

    fn from_value(value: &Value) -> CamelResult<Option<Self>> {
        Ok((!value.is_null()).then(|| value.clone()))
    }
}

impl FromValue for String {
    fn type_name() -> &'static str {
        "String"
    }

    /// Every value converts to its text form (Java `toString()`), bytes are decoded as UTF-8.
    fn from_value(value: &Value) -> CamelResult<Option<Self>> {
        Ok(match value {
            Value::Null => None,
            Value::Text(s) => Some(s.clone()),
            Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            Value::Float(f) => Some(java_double_to_string(*f)),
            v => Some(v.to_string()),
        })
    }
}

impl FromValue for bool {
    fn type_name() -> &'static str {
        "Boolean"
    }

    /// Booleans as is; texts `true`/`false` ignoring case.
    fn from_value(value: &Value) -> CamelResult<Option<Self>> {
        match value {
            Value::Null => Ok(None),
            Value::Bool(b) => Ok(Some(*b)),
            v => match text_of(v).map(|s| s.trim().to_ascii_lowercase()) {
                Some(s) if s == "true" => Ok(Some(true)),
                Some(s) if s == "false" => Ok(Some(false)),
                _ => Err(no_conversion::<Self>(v)),
            },
        }
    }
}

macro_rules! from_value_int {
    ($($t:ty => $name:expr),*) => {
        $(impl FromValue for $t {
            fn type_name() -> &'static str {
                $name
            }

            /// Integers (in range), floats without fraction and numeric texts convert.
            fn from_value(value: &Value) -> CamelResult<Option<Self>> {
                let err = || no_conversion::<Self>(value);
                match value {
                    Value::Null => Ok(None),
                    Value::Int(i) => <$t>::try_from(*i).map(Some).map_err(|_| err()),
                    Value::Float(f) if f.fract() == 0.0 && f.is_finite() => <$t>::try_from(*f as i64).map(Some).map_err(|_| err()),
                    Value::Bool(_) => Err(err()),
                    v => match text_of(v) {
                        Some(s) => s.trim().parse::<$t>().map(Some).map_err(|_| err()),
                        None => Err(err()),
                    },
                }
            }
        })*
    };
}

from_value_int!(i64 => "Long", i32 => "Integer", i16 => "Short", i8 => "Byte", u64 => "Long", u32 => "Integer", u16 => "Short", u8 => "Byte", usize => "Long", isize => "Long");

macro_rules! from_value_float {
    ($($t:ty => $name:expr),*) => {
        $(impl FromValue for $t {
            fn type_name() -> &'static str {
                $name
            }

            /// Numbers and numeric texts convert.
            fn from_value(value: &Value) -> CamelResult<Option<Self>> {
                match value {
                    Value::Null => Ok(None),
                    Value::Int(i) => Ok(Some(*i as $t)),
                    Value::Float(f) => Ok(Some(*f as $t)),
                    v => match text_of(v) {
                        Some(s) => s.trim().parse::<$t>().map(Some).map_err(|_| no_conversion::<Self>(v)),
                        None => Err(no_conversion::<Self>(v)),
                    },
                }
            }
        })*
    };
}

from_value_float!(f64 => "Double", f32 => "Float");

impl FromValue for Vec<u8> {
    fn type_name() -> &'static str {
        "byte[]"
    }

    /// Bytes as is; any other value as the UTF-8 bytes of its text form.
    fn from_value(value: &Value) -> CamelResult<Option<Self>> {
        Ok(match value {
            Value::Null => None,
            Value::Bytes(b) => Some(b.clone()),
            v => String::from_value(v)?.map(String::into_bytes),
        })
    }
}

impl FromValue for Vec<Value> {
    fn type_name() -> &'static str {
        "List"
    }

    /// Lists as is; any other value as a list of one element.
    fn from_value(value: &Value) -> CamelResult<Option<Self>> {
        Ok(match value {
            Value::Null => None,
            Value::List(l) => Some(l.clone()),
            v => Some(vec![v.clone()]),
        })
    }
}

impl FromValue for camel_util::value::ValueMap {
    fn type_name() -> &'static str {
        "Map"
    }

    fn from_value(value: &Value) -> CamelResult<Option<Self>> {
        match value {
            Value::Null => Ok(None),
            Value::Map(m) => Ok(Some(m.clone())),
            v => Err(no_conversion::<Self>(v)),
        }
    }
}

/// Converts the value to the type (convenience for [`FromValue::from_value`]).
///
/// # Errors
///
/// Returns an error if the value cannot be converted.
///
/// # Examples
///
/// ```
/// use camel_api::convert::convert_to;
/// use camel_util::value::Value;
/// assert_eq!(Some(123), convert_to::<i32>(&Value::from("123")).unwrap());
/// assert_eq!(Some("1.5".to_string()), convert_to::<String>(&Value::from(1.5)).unwrap());
/// assert_eq!(None, convert_to::<String>(&Value::Null).unwrap());
/// assert!(convert_to::<i32>(&Value::from("abc")).is_err());
/// ```
pub fn convert_to<T: FromValue>(value: &Value) -> CamelResult<Option<T>> {
    T::from_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(
            Some(true),
            convert_to::<bool>(&Value::from("TRUE")).unwrap()
        );
        assert!(convert_to::<bool>(&Value::from("yes")).is_err());
        assert_eq!(Some(5i64), convert_to::<i64>(&Value::from(5.0)).unwrap());
        assert!(convert_to::<i64>(&Value::from(5.5)).is_err());
        assert!(convert_to::<u8>(&Value::from(300)).is_err());
        assert_eq!(Some(2.5), convert_to::<f64>(&Value::from("2.5")).unwrap());
        assert_eq!(
            Some(b"abc".to_vec()),
            convert_to::<Vec<u8>>(&Value::from("abc")).unwrap()
        );
        assert_eq!(
            Some("abc".to_string()),
            convert_to::<String>(&Value::from(b"abc".to_vec())).unwrap()
        );
        assert_eq!(
            Some(vec![Value::from(1)]),
            convert_to::<Vec<Value>>(&Value::from(1)).unwrap()
        );
        assert!(convert_to::<camel_util::value::ValueMap>(&Value::from(1)).is_err());
        assert_eq!(None, convert_to::<Value>(&Value::Null).unwrap());
        let e = convert_to::<i32>(&Value::from("abc")).unwrap_err();
        assert_eq!("No type converter available to convert from type: String to the required type: Integer", e.to_string());
    }
}
