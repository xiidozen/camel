//! Conversion of values to Java type names (`String`, `Integer`, `byte[]`, ...), as used by
//! the simple language (`${bodyAs(Integer)}`) and `convertBodyTo`.

use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult};
use camel_util::value::Value;

/// Converts the value to the Java type name (`String`, `Integer`, `int`, `Long`, `Double`,
/// `Boolean`, `byte[]`, ... with or without `java.lang.`).
///
/// # Errors
///
/// Returns an error for an unknown type, or (when `mandatory`) a value that cannot be
/// converted.
pub fn convert_to_type(value: &Value, type_name: &str) -> CamelResult<Value> {
    fn conv<T: FromValue>(v: &Value) -> CamelResult<Option<T>> {
        T::from_value(v)
    }
    let t = type_name.trim().trim_start_matches("java.lang.");
    let v = match t {
        "String" | "CharSequence" => conv::<String>(value)?.map(Value::Text),
        "Integer" | "int" | "Long" | "long" | "Short" | "short" | "Byte" | "byte" | "Number" => {
            conv::<i64>(value)?.map(Value::Int)
        }
        "Double" | "double" | "Float" | "float" => conv::<f64>(value)?.map(Value::Float),
        "Boolean" | "boolean" => conv::<bool>(value)?.map(Value::Bool),
        "byte[]" => conv::<Vec<u8>>(value)?.map(Value::Bytes),
        "Object" => Some(value.clone()),
        "java.util.List" | "List" | "java.util.Collection" | "Collection" => {
            conv::<Vec<Value>>(value)?.map(Value::List)
        }
        "java.util.Map" | "Map" => match value {
            Value::Map(_) => Some(value.clone()),
            _ => None,
        },
        other => {
            return Err(CamelError::illegal_argument(format!(
                "Cannot find class: {other}"
            )))
        }
    };
    Ok(v.unwrap_or_default())
}
