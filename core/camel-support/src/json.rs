//! JSON and [`Value`]s (Java: the Jackson mapping of maps, lists and simple values): JSON
//! objects are maps (in order), arrays lists, numbers integers or floats.

use camel_api::error::{CamelError, CamelResult};
use camel_util::value::{Value, ValueMap};

/// Whether the text is a JSON object or array, by its first and last characters (Java
/// `JsonPayloadHelper.isJsonText`: the text is not parsed).
///
/// # Examples
///
/// ```
/// use camel_support::json::is_json_text;
///
/// assert!(is_json_text("  {\"a\": 1}\n"));
/// assert!(is_json_text("[1]"));
/// assert!(!is_json_text("{not closed"));
/// assert!(!is_json_text("hello"));
/// ```
pub fn is_json_text(text: &str) -> bool {
    let t = text.trim();
    t.len() >= 2
        && ((t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']')))
}

/// Writes a body that already is JSON as it is (Java `JsonPayloadHelper.writeIfAlreadyJson`):
/// bytes, and a text that is a JSON object or array; `false` for the other bodies, which the
/// JSON data formats then serialize.
pub fn write_if_already_json(body: &Value, out: &mut Vec<u8>) -> bool {
    match body {
        Value::Bytes(b) => {
            out.extend_from_slice(b);
            true
        }
        Value::Text(t) if is_json_text(t) => {
            out.extend_from_slice(t.as_bytes());
            true
        }
        _ => false,
    }
}

/// The value of the JSON value.
pub fn from_json(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().unwrap_or_default()),
        },
        serde_json::Value::String(s) => Value::Text(s),
        serde_json::Value::Array(a) => Value::List(a.into_iter().map(from_json).collect()),
        serde_json::Value::Object(o) => Value::Map(
            o.into_iter()
                .map(|(k, v)| (k, from_json(v)))
                .collect::<ValueMap>(),
        ),
    }
}

/// The JSON value of the value: bytes as a base64 text (as Jackson writes `byte[]`), the
/// other objects as their text.
pub fn to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(t) => serde_json::Value::String(t.clone()),
        Value::Bytes(b) => serde_json::Value::String(base64_encode(b)),
        Value::List(l) => serde_json::Value::Array(l.iter().map(to_json).collect()),
        Value::Map(m) => {
            serde_json::Value::Object(m.iter().map(|(k, v)| (k.clone(), to_json(v))).collect())
        }
        other => serde_json::Value::String(other.to_string()),
    }
}

/// Parses the JSON text.
///
/// # Errors
///
/// The text is not valid JSON.
///
/// # Examples
///
/// ```
/// use camel_support::json::{parse, stringify};
///
/// let v = parse(r#"{"value":123,"list":[1,"a"]}"#).unwrap();
/// assert_eq!(r#"{"value":123,"list":[1,"a"]}"#, stringify(&v, false));
/// ```
pub fn parse(text: &str) -> CamelResult<Value> {
    serde_json::from_str::<serde_json::Value>(text)
        .map(from_json)
        .map_err(|e| CamelError::illegal_argument(format!("Invalid JSON: {e}")))
}

/// Parses the JSON bytes.
///
/// # Errors
///
/// The bytes are not valid JSON.
pub fn parse_bytes(bytes: &[u8]) -> CamelResult<Value> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .map(from_json)
        .map_err(|e| CamelError::illegal_argument(format!("Invalid JSON: {e}")))
}

/// The JSON text of the value, pretty printed (two spaces) if asked.
pub fn stringify(value: &Value, pretty: bool) -> String {
    let json = to_json(value);
    if pretty {
        serde_json::to_string_pretty(&json).unwrap_or_default()
    } else {
        json.to_string()
    }
}

fn base64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        let v = parse(r#"[{"value":123},1.5,true,null,"x"]"#).unwrap();
        assert_eq!(r#"[{"value":123},1.5,true,null,"x"]"#, stringify(&v, false));
        assert_eq!("\"AQID\"", stringify(&Value::Bytes(vec![1, 2, 3]), false));
        assert!(parse("{not json").is_err());
    }
}
