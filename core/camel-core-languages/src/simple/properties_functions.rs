//! The properties and registry functions of the simple language (Java
//! `PropertiesFunctionFactory`): `${properties:key}`, `${properties:key:default}`,
//! `${propertiesExist:key}`, `${propertiesExist:!key}` and `${ref:name}`. The key and the
//! name may have nested functions (`${properties:${header.key}}`).

use super::functions::if_starts_with_return_remainder;
use super::string_functions::Lazy;
use super::ParseResult;
use camel_api::error::CamelError;
use camel_api::processor::SharedExpression;
use camel_api::spi::properties::{PREFIX_TOKEN, SUFFIX_TOKEN};
use camel_support::builder::named_expression;
use camel_util::value::Value;

/// The properties and ref functions, or `None` when the function is not one of them.
///
/// # Errors
///
/// None at parse time; the errors are raised when evaluated.
pub fn properties_function(function: &str, _index: usize) -> ParseResult<Option<SharedExpression>> {
    if let Some(remainder) = if_starts_with_return_remainder("propertiesExist:", function) {
        let key = remainder.split_once(':').map_or(remainder, |(k, _)| k);
        let (key, negate) = match key.strip_prefix('!') {
            Some(k) => (k.to_string(), true),
            None => (key.to_string(), false),
        };
        return Ok(Some(named_expression(
            format!("propertiesExist({key})"),
            move |e| {
                let present = e
                    .context()
                    .properties_component()
                    .and_then(|pc| pc.resolve_property(&key))
                    .is_some();
                Ok(Value::Bool(present != negate))
            },
        )));
    }
    if let Some(remainder) = if_starts_with_return_remainder("properties:", function) {
        let (key, default_value) = match remainder.split_once(':') {
            Some((k, d)) => (k.to_string(), Some(d.to_string())),
            None => (remainder.to_string(), None),
        };
        let lazy = Lazy::new(&key);
        return Ok(Some(named_expression(
            format!("properties({key})"),
            move |e| {
                let text = lazy.evaluate_string(e)?.unwrap_or_default();
                let result = match e.context().properties_component() {
                    // enclose the key with {{ }} to force parsing, as the key can be nested too
                    Some(pc) => pc.parse_uri(&format!("{PREFIX_TOKEN}{text}{SUFFIX_TOKEN}")),
                    None => Err(CamelError::illegal_state(
                        "The CamelContext has no properties component",
                    )),
                };
                match result {
                    Ok(v) => Ok(v.map(Value::from).unwrap_or_default()),
                    // a missing property uses the default value, if any
                    Err(_) if default_value.is_some() => {
                        Ok(Value::from(default_value.clone().unwrap_or_default()))
                    }
                    Err(err) => Err(CamelError::wrap_runtime(err)),
                }
            },
        )));
    }
    if let Some(remainder) = if_starts_with_return_remainder("ref:", function) {
        let lazy = Lazy::new(remainder);
        let name = remainder.to_string();
        return Ok(Some(named_expression(format!("ref({name})"), move |e| {
            let text = lazy.evaluate_string(e)?;
            Ok(text
                .and_then(|n| e.context().registry().and_then(|r| r.lookup_by_name(&n)))
                .unwrap_or_default())
        })));
    }
    Ok(None)
}
