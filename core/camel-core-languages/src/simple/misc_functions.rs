//! The misc functions of the simple language (Java `MiscFunctionFactory` and
//! `MiscExpressionBuilder`): `${isEmpty()}`, `${isAlpha()}`, `${isNumeric()}`, `${not(..)}`,
//! `${kindOfType()}`, `${throwException(..)}`, `${assert(..)}`, `${convertTo(..)}`, `${uuid}`,
//! `${hash(..)}`, `${newEmpty(..)}` and `${iif(..)}`.

use super::functions::{
    convert_to_type, if_starts_with_return_remainder, index_of_closing_parenthesis,
};
use super::math_functions::iterate;
use super::string_functions::{single_argument, string_of, value_of, Lazy};
use super::{ParseResult, ParserError};
use camel_api::error::{CamelError, ErrorKind};
use camel_api::processor::{SharedExpression, SharedPredicate};
use camel_api::spi::uuid::{DefaultUuidGenerator, UuidGenerator};
use camel_support::builder::named_expression;
use camel_support::uuid_generators::{
    ClassicUuidGenerator, RandomUuidGenerator, ShortUuidGenerator, SimpleUuidGenerator,
};
use camel_util::string_helper::{
    before, before_last, between, bytes_to_hex, remove_leading_and_ending_quotes,
};
use camel_util::string_quote_helper::split_safe_quote_full;
use camel_util::value::{Value, ValueMap};
use std::sync::{Arc, OnceLock};

/// The Java class name of the error of a failed `${assert(..)}`.
pub const SIMPLE_ASSERTION: &str = "org.apache.camel.language.simple.SimpleAssertionException";

/// A simple predicate parsed on first use.
#[derive(Clone)]
struct LazyPredicate {
    text: String,
    parsed: Arc<OnceLock<SharedPredicate>>,
}

impl LazyPredicate {
    fn new(text: &str) -> Self {
        LazyPredicate {
            text: text.to_string(),
            parsed: Arc::new(OnceLock::new()),
        }
    }

    fn matches(&self, e: &camel_api::exchange::Exchange) -> camel_api::error::CamelResult<bool> {
        if let Some(p) = self.parsed.get() {
            return p.matches(e);
        }
        let p = super::language::SimpleLanguage::new().parse_predicate(&self.text)?;
        self.parsed.get_or_init(|| p).matches(e)
    }
}

/// The JSON kind of the value (Java `MiscExpressionBuilder.kindOfType`): `null`, `number`,
/// `boolean`, `string`, `array` or `object`.
pub fn kind_of_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Int(_) | Value::Float(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Text(_) => "string",
        Value::List(_) | Value::Bytes(_) => "array",
        Value::Map(_) | Value::Object(_) => "object",
    }
}

fn chars_test(name: &'static str, r: &str, test: fn(char) -> bool) -> SharedExpression {
    let exp = single_argument(r);
    let d = match &exp {
        Some(x) => format!("{name}({})", x.text()),
        None => format!("{name}()"),
    };
    named_expression(d, move |e| {
        let body = string_of(&exp, e)?;
        Ok(Value::Bool(match body {
            Some(b) if !b.trim().is_empty() => b.chars().all(test),
            _ => false,
        }))
    })
}

/// The digest of the bytes with the Java algorithm name (`MD5`, `SHA-1`, `SHA-256`, ...).
///
/// # Errors
///
/// Returns an error for an unknown algorithm (Java `NoSuchAlgorithmException`).
pub fn digest(algorithm: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use sha2::Digest;
    Ok(match algorithm.to_uppercase().as_str() {
        "MD5" => md5::Md5::digest(data).to_vec(),
        "SHA-1" | "SHA1" | "SHA" => sha1::Sha1::digest(data).to_vec(),
        "SHA-224" => sha2::Sha224::digest(data).to_vec(),
        "SHA-256" => sha2::Sha256::digest(data).to_vec(),
        "SHA-384" => sha2::Sha384::digest(data).to_vec(),
        "SHA-512" => sha2::Sha512::digest(data).to_vec(),
        "SHA3-224" => sha3::Sha3_224::digest(data).to_vec(),
        "SHA3-256" => sha3::Sha3_256::digest(data).to_vec(),
        "SHA3-384" => sha3::Sha3_384::digest(data).to_vec(),
        "SHA3-512" => sha3::Sha3_512::digest(data).to_vec(),
        _ => return Err(format!("{algorithm} MessageDigest not available")),
    })
}

fn uuid_generator(kind: Option<&str>) -> Result<Arc<dyn UuidGenerator>, String> {
    let kind = kind.map(|k| remove_leading_and_ending_quotes(k.trim()));
    Ok(match kind.as_deref().map(str::to_lowercase).as_deref() {
        Some("classic") => Arc::new(ClassicUuidGenerator::default()),
        Some("short") => Arc::new(ShortUuidGenerator::default()),
        Some("simple") => Arc::new(SimpleUuidGenerator::default()),
        Some("random") => Arc::new(RandomUuidGenerator),
        None | Some("") | Some("default") => Arc::new(DefaultUuidGenerator::new()),
        Some(_) => return Err(kind.unwrap_or_default()),
    })
}

/// Creates the misc function, if the function is one.
///
/// # Errors
///
/// Returns a syntax error for invalid arguments.
pub fn misc_function(function: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let err = |m: &str| ParserError::new(format!("Valid syntax: {m} was: {function}"), index);
    if let Some(r) = if_starts_with_return_remainder("isEmpty(", function) {
        let exp = single_argument(r);
        let d = match &exp {
            Some(x) => format!("isEmpty({})", x.text()),
            None => "isEmpty()".to_string(),
        };
        return Ok(Some(named_expression(d, move |e| {
            let v = value_of(&exp, e)?;
            // a text is empty if it is blank (Java createIterable skips empty parts)
            Ok(Value::Bool(match &v {
                Value::Text(t) => t.split(',').all(|p| p.trim().is_empty()),
                Value::Map(m) => m.is_empty(),
                other => iterate(other.clone()).iter().all(Value::is_null),
            }))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("isAlpha(", function) {
        return Ok(Some(chars_test("isAlpha", r, char::is_alphabetic)));
    }
    if let Some(r) = if_starts_with_return_remainder("isAlphaNumeric(", function) {
        return Ok(Some(chars_test("isAlphaNumeric", r, char::is_alphanumeric)));
    }
    if let Some(r) = if_starts_with_return_remainder("isNumeric(", function) {
        return Ok(Some(chars_test("isNumeric", r, |c| c.is_ascii_digit())));
    }
    if let Some(r) = if_starts_with_return_remainder("not(", function) {
        let text = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "${body}".to_string());
        let p = LazyPredicate::new(&text);
        return Ok(Some(named_expression(format!("not({text})"), move |e| {
            Ok(Value::Bool(!p.matches(e)?))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("kindOfType(", function) {
        let exp = single_argument(r);
        let d = match &exp {
            Some(x) => format!("kindOfType({})", x.text()),
            None => "kindOfType()".to_string(),
        };
        return Ok(Some(named_expression(d, move |e| {
            Ok(Value::from(kind_of_type(&value_of(&exp, e)?)))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("throwException(", function) {
        let syntax = "${throwException(msg)} or ${throwException(msg,type)}";
        let values = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', true, true, false);
        if tokens.len() > 2 {
            return Err(err(syntax));
        }
        let msg = Lazy::new(&remove_leading_and_ending_quotes(&tokens[0]));
        // a Camel exception type is thrown as is; another type (such as java.io.IOException)
        // is wrapped in a RuntimeCamelException, with the type name as the detail of the cause
        let ty = tokens.get(1).map(|t| remove_leading_and_ending_quotes(t));
        let kind = match &ty {
            None => Some(ErrorKind::IllegalArgument),
            Some(name) => ErrorKind::from_java_name(name),
        };
        let d = format!("throwException({})", msg.text());
        return Ok(Some(named_expression(d, move |e| {
            let text = msg.evaluate_string(e)?.unwrap_or_default();
            Err(match kind {
                Some(k) => CamelError::new(k, text),
                None => CamelError::wrap(
                    ErrorKind::RuntimeCamel,
                    CamelError::new(ErrorKind::Runtime, text)
                        .with_detail(ty.clone().unwrap_or_default()),
                ),
            })
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("assert(", function) {
        let values = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| err("${assert(exp,msg)}"))?;
        let tokens = split_safe_quote_full(&values, ',', true, true, false);
        if tokens.len() != 2 {
            return Err(err("${assert(exp,msg)}"));
        }
        let p = LazyPredicate::new(&tokens[0]);
        let message = remove_leading_and_ending_quotes(&tokens[1]);
        let d = format!("assert({},{message})", tokens[0]);
        return Ok(Some(named_expression(d, move |e| {
            if !p.matches(e)? {
                return Err(CamelError::new(ErrorKind::IllegalArgument, message.clone())
                    .with_subtype(SIMPLE_ASSERTION));
            }
            Ok(Value::Null)
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("convertTo(", function) {
        let syntax = "${convertTo(type)} or ${convertTo(exp,type)}";
        let end = index_of_closing_parenthesis(r).ok_or_else(|| err(syntax))?;
        let values = &r[..end];
        if values.is_empty() {
            return Err(err(syntax));
        }
        let (exp, ty) = if values.contains(',') {
            let tokens = split_safe_quote_full(values, ',', true, true, false);
            if tokens.len() > 2 {
                return Err(err(syntax));
            }
            (
                remove_leading_and_ending_quotes(&tokens[0]),
                remove_leading_and_ending_quotes(&tokens[1]),
            )
        } else {
            (
                "${body}".to_string(),
                remove_leading_and_ending_quotes(values.trim()),
            )
        };
        if !r[end + 1..].is_empty() {
            return Err(ParserError::new(format!("The OGNL suffix of convertTo is not supported yet by the Rust port: {function}"), index));
        }
        let x = Lazy::new(&exp);
        return Ok(Some(named_expression(
            format!("convertTo({exp}, {ty})"),
            move |e| convert_to_type(&x.evaluate(e)?, &ty),
        )));
    }
    if let Some(r) = if_starts_with_return_remainder("uuid", function) {
        if r.starts_with('(') {
            let kind = between(r, "(", ")");
            let description = format!("uuid({})", kind.clone().unwrap_or_default());
            return Ok(Some(match uuid_generator(kind.as_deref()) {
                Ok(generator) => named_expression(description, move |_| {
                    Ok(Value::from(generator.generate_uuid()))
                }),
                // a generator bean from the registry
                Err(name) => named_expression(description, move |e| {
                    let generator = e
                        .context()
                        .registry()
                        .and_then(|r| {
                            camel_api::spi::registry::lookup_bean::<Arc<dyn UuidGenerator>>(
                                r.as_ref(),
                                &name,
                            )
                        })
                        .ok_or_else(|| {
                            CamelError::no_such_bean_of_type(
                                Some(&name),
                                "org.apache.camel.spi.UuidGenerator",
                            )
                        })?;
                    Ok(Value::from(generator.generate_uuid()))
                }),
            }));
        }
    } else if function == "uuid" {
        let generator = DefaultUuidGenerator::new();
        return Ok(Some(named_expression("uuid", move |_| {
            Ok(Value::from(generator.generate_uuid()))
        })));
    }
    if let Some(r) = if_starts_with_return_remainder("hash(", function) {
        let syntax = "${hash(value,algorithm)} or ${hash(value)}";
        let values = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', true, true, false);
        if tokens.len() > 2 {
            return Err(err(syntax));
        }
        let value = Lazy::new(&remove_leading_and_ending_quotes(tokens[0].trim()));
        let algorithm = tokens
            .get(1)
            .map(|t| remove_leading_and_ending_quotes(t.trim()))
            .unwrap_or_else(|| "SHA-256".to_string());
        let d = format!("hash({},{algorithm})", value.text());
        return Ok(Some(named_expression(d, move |e| {
            let v = value.evaluate(e)?;
            if v.is_null() {
                return Ok(Value::Null);
            }
            let bytes = match &v {
                Value::Bytes(b) => b.clone(),
                other => other.to_string().into_bytes(),
            };
            let hash = digest(&algorithm, &bytes).map_err(|m| {
                CamelError::wrap_camel_execution(
                    Some(e),
                    CamelError::new(ErrorKind::RuntimeCamel, m),
                )
            })?;
            Ok(Value::from(bytes_to_hex(&hash)))
        })));
    }
    for prefix in ["empty(", "newEmpty("] {
        if let Some(r) = if_starts_with_return_remainder(prefix, function) {
            let name = &prefix[..prefix.len() - 1];
            let ty = before(r, ")").filter(|v| !v.is_empty()).ok_or_else(|| {
                ParserError::new(
                    format!("Valid syntax: ${{{name}(<type>)}} but was: {function}"),
                    index,
                )
            })?;
            return Ok(Some(named_expression(
                format!("newEmpty({ty})"),
                move |_| {
                    match ty.to_lowercase().as_str() {
                        "map" => Ok(Value::Map(ValueMap::new())),
                        "string" => Ok(Value::from("")),
                        // a Rust list keeps the insertion order as Java LinkedHashSet does
                        "list" | "set" => Ok(Value::List(vec![])),
                        _ => Err(CamelError::illegal_argument(format!(
                            "function empty({ty}) has unknown type"
                        ))),
                    }
                },
            )));
        }
    }
    if let Some(r) = if_starts_with_return_remainder("iif(", function) {
        let syntax = "${iif(predicate,trueExpression,falseExpression)}";
        let values = before_last(r, ")")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| err(syntax))?;
        let tokens = split_safe_quote_full(&values, ',', true, true, false);
        if tokens.len() != 3 {
            return Err(err(syntax));
        }
        let p = LazyPredicate::new(tokens[0].trim());
        let (t, f) = (Lazy::new(tokens[1].trim()), Lazy::new(tokens[2].trim()));
        let d = format!(
            "iif({},{},{})",
            tokens[0].trim(),
            tokens[1].trim(),
            tokens[2].trim()
        );
        return Ok(Some(named_expression(d, move |e| {
            if p.matches(e)? {
                t.evaluate(e)
            } else {
                f.evaluate(e)
            }
        })));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!("number", kind_of_type(&Value::from(1)));
        assert_eq!("object", kind_of_type(&Value::Map(ValueMap::new())));
        assert_eq!(
            "a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3",
            bytes_to_hex(&digest("SHA-256", b"123").unwrap())
        );
        assert_eq!(
            "202cb962ac59075b964b07152d234b70",
            bytes_to_hex(&digest("MD5", b"123").unwrap())
        );
        assert!(digest("FOO", b"x").is_err());
        assert!(uuid_generator(Some("nope")).is_err());
    }
}
