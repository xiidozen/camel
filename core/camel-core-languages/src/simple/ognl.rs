//! Invoking methods and reading properties on values with an OGNL path, such as
//! `${body.getName()}`, `${header.cool.replaceAll("rocks", "is so cool")}` or
//! `${exchangeProperty.wicket[0].id}` (Java `BeanExpression.invokeOgnlMethod` of the bean
//! language, which the simple language uses for its OGNL functions).
//!
//! Java finds the methods by reflection. The Rust port knows the common methods of the
//! Java types behind each [`Value`] kind (`String`, `List`, `Map`, numbers, booleans, byte
//! arrays) and asks [`Bean`](camel_util::value::Bean) values (wrapped with
//! [`Value::bean`]) for their properties and methods.
//!
//! A step without parentheses (`${body.name}`) is a method without arguments, else a getter
//! (`getName()`/`isName()`), else the property of a bean, else the key of a map (Java
//! CAMEL-24916). A step with parentheses calls the method with its arguments: quoted texts,
//! numbers, `true`/`false`, `null` and simple expressions (`${header.foo}`).

use super::string_functions::Lazy;
use camel_api::convert::FromValue;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_util::ognl_helper;
use camel_util::string_helper::remove_leading_and_ending_quotes;
use camel_util::string_quote_helper::split_safe_quote_full;
use camel_util::value::{Value, ValueMap};

/// The Java class name of `org.apache.camel.language.bean.RuntimeBeanExpressionException`.
pub const RUNTIME_BEAN_EXPRESSION_EXCEPTION: &str =
    "org.apache.camel.language.bean.RuntimeBeanExpressionException";
/// The Java class name of `org.apache.camel.component.bean.MethodNotFoundException`.
pub const METHOD_NOT_FOUND_EXCEPTION: &str =
    "org.apache.camel.component.bean.MethodNotFoundException";
/// The Java class name of `java.lang.IndexOutOfBoundsException`.
pub const INDEX_OUT_OF_BOUNDS_EXCEPTION: &str = "java.lang.IndexOutOfBoundsException";

/// The Java class name of the value (Java `ObjectHelper.className`), used in error messages.
pub fn java_class_name(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "java.lang.Boolean".to_string(),
        Value::Int(_) => "java.lang.Integer".to_string(),
        Value::Float(_) => "java.lang.Double".to_string(),
        Value::Text(_) => "java.lang.String".to_string(),
        Value::Bytes(_) => "[B".to_string(),
        Value::List(_) => "java.util.ArrayList".to_string(),
        Value::Map(_) => "java.util.HashMap".to_string(),
        Value::Object(o) => o.type_name().to_string(),
    }
}

/// The Java canonical class name (`byte[]` for a byte array).
fn canonical_class_name(value: &Value) -> String {
    match value {
        Value::Bytes(_) => "byte[]".to_string(),
        v => java_class_name(v),
    }
}

/// The text of the value as Java `String.valueOf` (`[B@..` style for byte arrays).
fn java_to_string(value: &Value) -> String {
    match value {
        Value::Bytes(b) => format!("[B@{:x}", b.as_ptr() as usize as u32),
        v => v.to_string(),
    }
}

/// Invokes the OGNL path on the bean (Java `BeanExpression` with an OGNL method).
///
/// # Arguments
///
/// * `bean` - the value to start from (the body, a header, ...)
/// * `ognl` - the OGNL path, such as `.getName().length()` or `[0].id`
/// * `exchange` - the exchange, for the simple expressions in method arguments and keys
///
/// # Errors
///
/// A `RuntimeBeanExpressionException` (kind `RuntimeExpression`) wrapping the cause:
/// a `MethodNotFoundException` for an unknown method, an `IndexOutOfBoundsException` for an
/// index or key not found.
pub fn invoke_ognl(bean: &Value, ognl: &str, exchange: &Exchange) -> CamelResult<Value> {
    let bean_name = || canonical_class_name(bean);
    let wrap = |method: &str, e: CamelError| {
        if e.is_subtype(RUNTIME_BEAN_EXPRESSION_EXCEPTION)
            || e.is(ErrorKind::ExpressionIllegalSyntax)
        {
            return e;
        }
        CamelError::with_cause(
            ErrorKind::RuntimeExpression,
            format!(
                "Failed to invoke method: {method} on {} due to: {e}",
                bean_name()
            ),
            e,
        )
        .with_subtype(RUNTIME_BEAN_EXPRESSION_EXCEPTION)
    };
    ognl_helper::validate_method_name(ognl)
        .map_err(CamelError::illegal_argument)
        .map_err(|e| wrap(ognl, e))?;
    let methods = ognl_helper::split_ognl(ognl)
        .map_err(|m| CamelError::new(ErrorKind::ExpressionIllegalSyntax, m))?;
    let methods = if methods.is_empty() {
        vec![ognl.to_string()]
    } else {
        methods
    };
    let mut result = bean.clone();
    let mut path = String::new();
    for part in methods {
        let null_safe = ognl_helper::is_null_safe_operator(&part);
        let name = ognl_helper::remove_leading_operators(&part).to_string();
        if result.is_null() {
            return Err(CamelError::new(
                ErrorKind::RuntimeExpression,
                format!(
                    "Failed to invoke method: {ognl} on {} due last method returned null and therefore cannot continue to invoke method {name} on a null instance",
                    bean_name()
                ),
            )
            .with_subtype(RUNTIME_BEAN_EXPRESSION_EXCEPTION));
        }
        path.push_str(&part);
        let (method, key) = match ognl_helper::is_ognl_index(&name) {
            Some(kv) => (kv.key().clone(), kv.value().clone()),
            None => (Some(name.clone()), None),
        };
        let current = result.clone();
        if let Some(m) = &method {
            result = invoke(&current, m, exchange).map_err(|e| wrap(m, e))?;
        }
        if let Some(k) = key {
            let owner = result.clone();
            result = lookup_key(&owner, &k, exchange, null_safe, &path, &current)
                .map_err(|e| wrap(&name, e))?;
        }
        if result.is_null() && null_safe {
            return Ok(Value::Null);
        }
    }
    Ok(result)
}

/// Java `BeanExpression.lookupResult`: the key of a map, or the index (`0`, `last`,
/// `last-1`) of a list.
fn lookup_key(
    result: &Value,
    key: &str,
    exchange: &Exchange,
    null_safe: bool,
    path: &str,
    bean: &Value,
) -> CamelResult<Value> {
    let key = if super::tokenizer::has_function_start_token(key) {
        Lazy::new(key).evaluate(exchange)?.to_string()
    } else {
        key.to_string()
    };
    if key.trim().is_empty() {
        return Err(CamelError::illegal_argument(format!(
            "key must be specified in Simple language ognl path: {path}"
        )));
    }
    let key = remove_leading_and_ending_quotes(key.trim());
    if let Value::Map(m) = result {
        return Ok(m.get(&key).cloned().unwrap_or_default());
    }
    let num = key.parse::<i64>().ok();
    if key.starts_with("last") || num.is_some() {
        if let Value::List(list) = result {
            let mut index = num;
            if key.starts_with("last") {
                let mut n = list.len() as i64 - 1;
                if let Some((_, after)) = key.split_once('-') {
                    match after.trim().parse::<i64>() {
                        Ok(redux) => n -= redux,
                        Err(_) => {
                            return Err(CamelError::new(
                                ErrorKind::ExpressionIllegalSyntax,
                                format!("Illegal syntax: {key}"),
                            ))
                        }
                    }
                }
                index = Some(n);
            }
            if let Some(i) = index {
                if i >= 0 && (i as usize) < list.len() {
                    return Ok(list[i as usize].clone());
                }
            }
            if !null_safe {
                return Err(CamelError::new(
                    ErrorKind::RuntimeCamel,
                    format!(
                        "Index: {}, Size: {} out of bounds with List from bean: {}using OGNL path [{path}]",
                        index.unwrap_or_default(),
                        list.len(),
                        java_to_string(bean)
                    ),
                )
                .with_subtype(INDEX_OUT_OF_BOUNDS_EXCEPTION));
            }
        }
    }
    if null_safe {
        return Ok(Value::Null);
    }
    Err(CamelError::new(
        ErrorKind::RuntimeCamel,
        format!(
            "Key: {key} not found in bean: {} of type: {} using OGNL path [{path}]",
            java_to_string(bean),
            canonical_class_name(bean)
        ),
    )
    .with_subtype(INDEX_OUT_OF_BOUNDS_EXCEPTION))
}

/// Splits `name(args)` into the name and the argument texts (`None` without parentheses).
fn method_and_args(method: &str) -> (String, Option<Vec<String>>) {
    match method.find('(') {
        Some(i) if method.ends_with(')') => {
            let inner = &method[i + 1..method.len() - 1];
            let args = if inner.trim().is_empty() {
                Vec::new()
            } else {
                split_safe_quote_full(inner, ',', false, true, false)
            };
            (method[..i].trim().to_string(), Some(args))
        }
        _ => (method.trim().to_string(), None),
    }
}

/// The value of a method argument (Java bean parameter binding of literal values and simple
/// expressions).
fn argument(text: &str, exchange: &Exchange) -> CamelResult<Value> {
    let t = text.trim();
    if t.len() >= 2
        && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\'')))
    {
        return Ok(Value::from(&t[1..t.len() - 1]));
    }
    if super::tokenizer::has_function_start_token(t) {
        return Lazy::new(t).evaluate(exchange);
    }
    match t {
        "null" => return Ok(Value::Null),
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return Ok(Value::Int(i));
    }
    if let Ok(f) = t.parse::<f64>() {
        if t.contains('.') {
            return Ok(Value::Float(f));
        }
    }
    Ok(Value::from(t))
}

fn not_found(value: &Value, method: &str) -> CamelError {
    CamelError::new(
        ErrorKind::RuntimeExchange,
        format!(
            "Method with name: {method} not found on bean: {} of type: {}",
            java_to_string(value),
            java_class_name(value)
        ),
    )
    .with_subtype(METHOD_NOT_FOUND_EXCEPTION)
}

/// Invokes one OGNL step on the value.
fn invoke(value: &Value, step: &str, exchange: &Exchange) -> CamelResult<Value> {
    let (name, args) = method_and_args(step);
    let args = match args {
        Some(texts) => {
            let mut values = Vec::with_capacity(texts.len());
            for t in &texts {
                values.push(argument(t, exchange)?);
            }
            Some(values)
        }
        None => None,
    };
    let display = || match &args {
        Some(_) => format!(
            "{name}({})",
            &step[step.find('(').map_or(0, |i| i + 1)..step.len() - 1]
        ),
        None => name.clone(),
    };
    match &args {
        Some(a) if !a.is_empty() => {
            call(value, &name, a)?.ok_or_else(|| not_found(value, &display()))
        }
        _ => {
            // a method without arguments, a getter, a bean property, else a map key
            if let Some(v) = call(value, &name, &[])? {
                return Ok(v);
            }
            for prefix in ["get", "is"] {
                if let Some(p) = name.strip_prefix(prefix) {
                    if p.starts_with(|c: char| c.is_uppercase()) {
                        let property = decapitalize(p);
                        if let Some(v) = property_of(value, &property) {
                            return Ok(v);
                        }
                    }
                }
            }
            if let Some(v) = property_of(value, &name) {
                return Ok(v);
            }
            for prefix in ["get", "is"] {
                let getter = format!("{prefix}{}", capitalize(&name));
                if let Some(v) = call(value, &getter, &[])? {
                    return Ok(v);
                }
            }
            if args.is_none() {
                if let Value::Map(m) = value {
                    if let Some(v) = m.get(&name) {
                        return Ok(v.clone());
                    }
                }
            }
            Err(not_found(value, &display()))
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn decapitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The property of a bean, or of the built-in values (the `length` of arrays, the `class`).
fn property_of(value: &Value, name: &str) -> Option<Value> {
    match (value, name) {
        (Value::Object(o), _) if o.is_bean() => o.bean_property(name),
        (Value::Bytes(b), "length") => Some(Value::Int(b.len() as i64)),
        (Value::List(l), "length") => Some(Value::Int(l.len() as i64)),
        (_, "class") => Some(java_class(value)),
        _ => None,
    }
}

/// The Java class of the value, as a map with `name`, `simpleName` and `canonicalName`.
fn java_class(value: &Value) -> Value {
    let name = java_class_name(value);
    let simple = match value {
        Value::Bytes(_) => "byte[]".to_string(),
        _ => name.rsplit(['.', ':']).next().unwrap_or(&name).to_string(),
    };
    let mut m = ValueMap::new();
    m.insert("name".to_string(), Value::from(name.as_str()));
    m.insert("simpleName".to_string(), Value::from(simple));
    m.insert(
        "canonicalName".to_string(),
        Value::from(canonical_class_name(value)),
    );
    Value::Map(m)
}

fn text_arg(args: &[Value], i: usize) -> Option<String> {
    args.get(i).map(|v| v.to_string())
}

fn int_arg(args: &[Value], i: usize) -> CamelResult<Option<i64>> {
    match args.get(i) {
        Some(v) => i64::from_value(v),
        None => Ok(None),
    }
}

fn regex(pattern: &str) -> CamelResult<regex::Regex> {
    regex::Regex::new(pattern).map_err(|e| CamelError::illegal_argument(e.to_string()))
}

/// Java `Matcher.quoteReplacement` semantics are not needed: Java and Rust both use `$1`
/// for groups; Java `\$` escapes become `$$`.
fn java_replacement(r: &str) -> String {
    r.replace("\\$", "$$")
}

/// Calls the method with the arguments, `None` if the value has no such method.
fn call(value: &Value, name: &str, args: &[Value]) -> CamelResult<Option<Value>> {
    let n = args.len();
    // methods of all objects
    match (name, n) {
        ("toString", 0) => return Ok(Some(Value::from(java_to_string(value)))),
        ("getClass", 0) => return Ok(Some(java_class(value))),
        ("hashCode", 0) => {}
        ("equals", 1) => return Ok(Some(Value::Bool(*value == args[0]))),
        _ => {}
    }
    let v = match value {
        Value::Object(o) if o.is_bean() => match o.bean_invoke(name, args) {
            Some(Ok(v)) => Some(v),
            Some(Err(m)) => return Err(CamelError::runtime(m)),
            None => None,
        },
        Value::Text(s) => string_method(s, name, args)?,
        Value::List(l) => list_method(l, name, args)?,
        Value::Map(m) => map_method(m, name, args),
        Value::Int(i) => match (name, n) {
            ("intValue" | "longValue" | "shortValue" | "byteValue", 0) => Some(Value::Int(*i)),
            ("doubleValue" | "floatValue", 0) => Some(Value::Float(*i as f64)),
            _ => None,
        },
        Value::Float(f) => match (name, n) {
            ("intValue" | "longValue", 0) => Some(Value::Int(*f as i64)),
            ("doubleValue" | "floatValue", 0) => Some(Value::Float(*f)),
            _ => None,
        },
        Value::Bool(b) => match (name, n) {
            ("booleanValue", 0) => Some(Value::Bool(*b)),
            _ => None,
        },
        _ => None,
    };
    Ok(v)
}

/// The methods of `java.lang.String`.
fn string_method(s: &str, name: &str, args: &[Value]) -> CamelResult<Option<Value>> {
    let chars: Vec<char> = s.chars().collect();
    let text = |i| text_arg(args, i).unwrap_or_default();
    let v = match (name, args.len()) {
        ("toUpperCase", 0) => Value::from(s.to_uppercase()),
        ("toLowerCase", 0) => Value::from(s.to_lowercase()),
        ("trim", 0) => Value::from(camel_util::lang::java_trim(s)),
        ("strip", 0) => Value::from(s.trim()),
        ("stripLeading", 0) => Value::from(s.trim_start()),
        ("stripTrailing", 0) => Value::from(s.trim_end()),
        ("length", 0) => Value::Int(chars.len() as i64),
        ("isEmpty", 0) => Value::Bool(s.is_empty()),
        ("isBlank", 0) => Value::Bool(s.trim().is_empty()),
        ("intern", 0) => Value::from(s),
        ("getBytes", 0) => Value::Bytes(s.as_bytes().to_vec()),
        ("toCharArray", 0) => {
            Value::List(chars.iter().map(|c| Value::from(c.to_string())).collect())
        }
        ("concat", 1) => Value::from(format!("{s}{}", text(0))),
        ("contains", 1) => Value::Bool(s.contains(&text(0))),
        ("startsWith", 1) => Value::Bool(s.starts_with(&text(0))),
        ("endsWith", 1) => Value::Bool(s.ends_with(&text(0))),
        ("equalsIgnoreCase", 1) => Value::Bool(s.to_lowercase() == text(0).to_lowercase()),
        ("matches", 1) => {
            let re = regex(&format!("^(?:{})$", text(0)))?;
            Value::Bool(re.is_match(s))
        }
        ("indexOf", 1) => Value::Int(char_index(s, s.find(&text(0)))),
        ("lastIndexOf", 1) => Value::Int(char_index(s, s.rfind(&text(0)))),
        ("charAt", 1) => {
            let i = int_arg(args, 0)?.unwrap_or_default();
            match chars.get(i as usize) {
                Some(c) if i >= 0 => Value::from(c.to_string()),
                _ => {
                    return Err(CamelError::new(
                        ErrorKind::RuntimeCamel,
                        format!("Index {i} out of bounds for length {}", chars.len()),
                    )
                    .with_subtype("java.lang.StringIndexOutOfBoundsException"))
                }
            }
        }
        ("substring", 1 | 2) => {
            let begin = int_arg(args, 0)?.unwrap_or_default();
            let end = if args.len() == 2 {
                int_arg(args, 1)?.unwrap_or_default()
            } else {
                chars.len() as i64
            };
            if begin < 0 || end > chars.len() as i64 || begin > end {
                return Err(CamelError::new(
                    ErrorKind::RuntimeCamel,
                    format!("begin {begin}, end {end}, length {}", chars.len()),
                )
                .with_subtype("java.lang.StringIndexOutOfBoundsException"));
            }
            Value::from(
                chars[begin as usize..end as usize]
                    .iter()
                    .collect::<String>(),
            )
        }
        ("replace", 2) => Value::from(s.replace(&text(0), &text(1))),
        ("replaceAll", 2) => Value::from(
            regex(&text(0))?
                .replace_all(s, java_replacement(&text(1)).as_str())
                .into_owned(),
        ),
        ("replaceFirst", 2) => Value::from(
            regex(&text(0))?
                .replace(s, java_replacement(&text(1)).as_str())
                .into_owned(),
        ),
        ("split", 1) => Value::List(
            camel_util::lang::java_split(s, &text(0))
                .into_iter()
                .map(Value::from)
                .collect(),
        ),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// The char index of a byte index (Java indexes are chars), `-1` for none.
fn char_index(s: &str, byte_index: Option<usize>) -> i64 {
    match byte_index {
        Some(b) => s[..b].chars().count() as i64,
        None => -1,
    }
}

/// The methods of `java.util.List` (and arrays).
fn list_method(l: &[Value], name: &str, args: &[Value]) -> CamelResult<Option<Value>> {
    let v = match (name, args.len()) {
        ("size", 0) => Value::Int(l.len() as i64),
        ("isEmpty", 0) => Value::Bool(l.is_empty()),
        ("getFirst", 0) => l.first().cloned().unwrap_or_default(),
        ("getLast", 0) => l.last().cloned().unwrap_or_default(),
        ("contains", 1) => Value::Bool(l.contains(&args[0])),
        ("indexOf", 1) => Value::Int(
            l.iter()
                .position(|x| *x == args[0])
                .map_or(-1, |i| i as i64),
        ),
        ("get", 1) => {
            let i = int_arg(args, 0)?.unwrap_or(-1);
            match l.get(i as usize) {
                Some(v) if i >= 0 => v.clone(),
                _ => {
                    return Err(CamelError::new(
                        ErrorKind::RuntimeCamel,
                        format!("Index {i} out of bounds for length {}", l.len()),
                    )
                    .with_subtype(INDEX_OUT_OF_BOUNDS_EXCEPTION))
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// The methods of `java.util.Map`.
fn map_method(m: &ValueMap, name: &str, args: &[Value]) -> Option<Value> {
    let key = || args.first().map(|v| v.to_string()).unwrap_or_default();
    let v = match (name, args.len()) {
        ("size", 0) => Value::Int(m.len() as i64),
        ("isEmpty", 0) => Value::Bool(m.is_empty()),
        ("keySet", 0) => Value::List(m.keys().map(|k| Value::from(k.as_str())).collect()),
        ("values", 0) => Value::List(m.values().cloned().collect()),
        ("get", 1) => m.get(&key()).cloned().unwrap_or_default(),
        ("containsKey", 1) => Value::Bool(m.contains_key(&key())),
        ("containsValue", 1) => Value::Bool(m.values().any(|v| *v == args[0])),
        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_api::context::StandaloneContext;
    use camel_util::value::Bean;

    struct OrderLine {
        id: i64,
        name: &'static str,
    }

    impl Bean for OrderLine {
        fn property(&self, name: &str) -> Option<Value> {
            match name {
                "id" => Some(Value::Int(self.id)),
                "name" => Some(Value::from(self.name)),
                _ => None,
            }
        }
    }

    fn exchange() -> Exchange {
        Exchange::new(StandaloneContext::shared())
    }

    #[test]
    fn test_string_methods() {
        let e = exchange();
        let v = Value::from("Camel rocks");
        assert_eq!(
            Value::from("Camel is so cool"),
            invoke_ognl(&v, ".replaceAll(\"rocks\", \"is so cool\")", &e).unwrap()
        );
        assert_eq!(
            Value::from("CAMEL ROCKS"),
            invoke_ognl(&v, ".toUpperCase()", &e).unwrap()
        );
        assert_eq!(Value::Int(11), invoke_ognl(&v, ".length", &e).unwrap());
        assert_eq!(
            Value::from("rocks"),
            invoke_ognl(&v, ".substring(6)", &e).unwrap()
        );
        assert_eq!(
            Value::from("java.lang.String"),
            invoke_ognl(&v, ".class.name", &e).unwrap()
        );
    }

    #[test]
    fn test_bean_properties() {
        let e = exchange();
        let v = Value::bean(OrderLine {
            id: 123,
            name: "Camel in Action",
        });
        assert_eq!(Value::Int(123), invoke_ognl(&v, ".id", &e).unwrap());
        assert_eq!(Value::Int(123), invoke_ognl(&v, ".getId", &e).unwrap());
        assert_eq!(Value::Int(123), invoke_ognl(&v, ".getId()", &e).unwrap());
        assert_eq!(
            Value::from("Camel in Action"),
            invoke_ognl(&v, ".getName", &e).unwrap()
        );
    }

    #[test]
    fn test_method_not_found() {
        let e = exchange();
        let err = invoke_ognl(&Value::Bytes(b"hello".to_vec()), ".toUpperCase()", &e).unwrap_err();
        assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
        let cause = err
            .cause()
            .and_then(|c| c.downcast_ref::<CamelError>())
            .unwrap();
        assert!(cause.is_subtype("MethodNotFoundException"), "{cause}");
    }

    #[test]
    fn test_index_out_of_bounds() {
        let e = exchange();
        let v = Value::List(vec![Value::from("a"), Value::from("b")]);
        let err = invoke_ognl(&v, "[2]", &e).unwrap_err();
        let cause = err
            .cause()
            .and_then(|c| c.downcast_ref::<CamelError>())
            .unwrap();
        assert!(cause.is_subtype("IndexOutOfBoundsException"));
        assert!(cause
            .message()
            .unwrap()
            .starts_with("Index: 2, Size: 2 out of bounds with List from bean"));
        assert_eq!(Value::Null, invoke_ognl(&v, "?[2]", &e).unwrap());
    }
}
