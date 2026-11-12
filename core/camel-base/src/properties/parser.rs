//! The parser of property placeholders (Java `DefaultPropertiesParser`).

use super::Config;
use camel_api::error::{CamelError, CamelResult};
use camel_api::spi::properties::{OPTIONAL_TOKEN, PREFIX_TOKEN, SUFFIX_TOKEN};
use camel_util::io_helper::lookup_environment_variable;
use camel_util::system_properties;
use std::collections::HashSet;

/// The marks of an unresolved placeholder kept in the text (replaced by `{{` `}}` at the end).
const UNRESOLVED_PREFIX_TOKEN: &str = "@@[";
const UNRESOLVED_SUFFIX_TOKEN: &str = "]@@";

/// The separator of the default value of `{{key:default}}`.
const GET_OR_ELSE_TOKEN: &str = ":";

/// Never look up the environment variables / system properties.
pub const MODE_NEVER: u8 = 0;
/// Look them up when no other source has the property.
pub const MODE_FALLBACK: u8 = 1;
/// Look them up first (the default).
pub const MODE_OVERRIDE: u8 = 2;

/// Replaces the placeholders of the text (Java `DefaultPropertiesParser.parseUri`).
pub(crate) fn parse_uri(
    config: &Config,
    text: &str,
    keep_unresolved_optional: bool,
) -> CamelResult<Option<String>> {
    let context = ParsingContext {
        config,
        keep_unresolved_optional,
    };
    let answer = context.parse(text)?;
    if keep_unresolved_optional || config.ignore_missing_property {
        if let Some(a) = &answer {
            if a.contains(UNRESOLVED_PREFIX_TOKEN) {
                return Ok(Some(
                    a.replace(UNRESOLVED_PREFIX_TOKEN, PREFIX_TOKEN)
                        .replace(UNRESOLVED_SUFFIX_TOKEN, SUFFIX_TOKEN),
                ));
            }
        }
    }
    Ok(answer)
}

/// A placeholder found in a text.
struct Property {
    begin: usize,
    end: usize,
    key: String,
    value: Option<String>,
}

struct ParsingContext<'a> {
    config: &'a Config,
    keep_unresolved_optional: bool,
}

fn illegal(message: String) -> CamelError {
    CamelError::illegal_argument(message)
}

impl ParsingContext<'_> {
    fn parse(&self, input: &str) -> CamelResult<Option<String>> {
        let mut nested = self.config.nested_placeholder;
        let mut input = input.to_string();
        // the option is in the key of a placeholder (such as {{foo?nested=false}})
        let on = format!("?nested=true{SUFFIX_TOKEN}");
        let off = format!("?nested=false{SUFFIX_TOKEN}");
        if input.contains(&on) {
            nested = true;
            input = input.replace(&on, SUFFIX_TOKEN);
        } else if input.contains(&off) {
            nested = false;
            input = input.replace(&off, SUFFIX_TOKEN);
        }
        if nested {
            self.do_parse_nested(Some(input), &HashSet::new())
        } else {
            self.do_parse(&input)
        }
    }

    /// Replaces the placeholders, not nested.
    fn do_parse(&self, input: &str) -> CamelResult<Option<String>> {
        let mut answer = String::with_capacity(input.len());
        let mut input = input.to_string();
        let mut first = true;
        while let Some(property) = self.read_property(&input)? {
            let mut begin = property.begin;
            if begin > 0 && input.as_bytes()[begin - 1] == b'\\' {
                // the escape character has been escaped, so restore it
                begin -= 1;
            }
            let before = &input[..begin];
            let after = input[property.end..].to_string();
            if property.value.is_none()
                && first
                && property.begin == 0
                && input.len() == property.end
            {
                // only a single placeholder which is parsed as null
                return Ok(None);
            }
            answer.push_str(before);
            if let Some(v) = &property.value {
                answer.push_str(v);
            }
            input = after;
            first = false;
        }
        answer.push_str(&input);
        Ok(Some(answer))
    }

    /// Replaces the placeholders recursively (the values may have placeholders).
    fn do_parse_nested(
        &self,
        input: Option<String>,
        replaced: &HashSet<String>,
    ) -> CamelResult<Option<String>> {
        let Some(input) = input else {
            return Ok(None);
        };
        let mut answer = input.clone();
        // continue after the resolved value of the previous placeholder, as the value must
        // not be parsed again
        let mut from = 0;
        while let Some(property) = self.read_property_from(&answer, from)? {
            if replaced.contains(&property.key) {
                return Err(illegal(format!(
                    "Circular reference detected with key [{}] from text: {input}",
                    property.key
                )));
            }
            let mut new_replaced = replaced.clone();
            new_replaced.insert(property.key.clone());
            let mut begin = property.begin;
            if begin > from && answer.as_bytes()[begin - 1] == b'\\' {
                begin -= 1;
            }
            let before = answer[..begin].to_string();
            let after = answer[property.end..].to_string();
            let parsed = self.do_parse_nested(property.value.clone(), &new_replaced)?;
            match parsed {
                Some(p) => {
                    from = before.len() + p.len();
                    answer = format!("{before}{p}{after}");
                }
                None => {
                    if begin == 0 && input.len() == property.end {
                        // only a single placeholder which is parsed as null
                        return Ok(None);
                    }
                    from = before.len();
                    answer = format!("{before}{after}");
                }
            }
        }
        Ok(Some(answer))
    }

    fn read_property_from(&self, input: &str, from: usize) -> CamelResult<Option<Property>> {
        if from == 0 {
            return self.read_property(input);
        }
        Ok(self.read_property(&input[from..])?.map(|p| Property {
            begin: p.begin + from,
            end: p.end + from,
            ..p
        }))
    }

    fn read_property(&self, input: &str) -> CamelResult<Option<Property>> {
        let Some(suffix) = suffix_index(input) else {
            if matching_prefix_index(input, input.len()).is_some() {
                return Err(illegal(format!(
                    "Missing {SUFFIX_TOKEN} from the text: {input}"
                )));
            }
            return Ok(None);
        };
        let Some(prefix) = matching_prefix_index(input, suffix) else {
            return Err(illegal(format!(
                "Missing {PREFIX_TOKEN} from the text: {input}"
            )));
        };
        let key = input[prefix + PREFIX_TOKEN.len()..suffix].to_string();
        let value = self.property_value(&key, input)?;
        Ok(Some(Property {
            begin: prefix,
            end: suffix + SUFFIX_TOKEN.len(),
            key,
            value,
        }))
    }

    /// The value of the key of a placeholder (Java `getPropertyValue`): a function, else the
    /// property with its default value.
    fn property_value(&self, key: &str, input: &str) -> CamelResult<Option<String>> {
        let optional = key.starts_with(OPTIONAL_TOKEN);
        let key = key.strip_prefix(OPTIONAL_TOKEN).unwrap_or(key);
        let keep = self.keep_unresolved_optional;
        let ignore_missing = self.config.ignore_missing_property;

        // the key may be a function
        if let Some((prefix, remainder)) = key.split_once(':') {
            if let Some(function) = self.config.functions.get(prefix).cloned() {
                let mut remainder = remainder.to_string();
                // optional on the key ({{?fn:key}}) or on the remainder ({{fn:?key}})
                let mut remainder_optional = optional || remainder.starts_with(OPTIONAL_TOKEN);
                if function.lookup_first(&remainder) {
                    let value = self.property_value(&remainder, input)?;
                    if value.is_none() && (remainder_optional || function.optional(&remainder)) {
                        return Ok(None);
                    }
                    match value {
                        Some(v) if v.starts_with(UNRESOLVED_PREFIX_TOKEN) => return Ok(Some(v)),
                        Some(v) => remainder = v,
                        None => {}
                    }
                } else if let Some(r) = remainder.strip_prefix(OPTIONAL_TOKEN) {
                    remainder = r.to_string();
                }
                return match function.apply(&remainder) {
                    Some(value) => {
                        log::debug!(
                            "Property with key [{key}] applied by function [{}]",
                            function.name()
                        );
                        Ok(Some(value))
                    }
                    None => {
                        if !remainder_optional {
                            remainder_optional = function.optional(&remainder);
                        }
                        if !remainder_optional && ignore_missing {
                            return Ok(Some(format!(
                                "{UNRESOLVED_PREFIX_TOKEN}{key}{UNRESOLVED_SUFFIX_TOKEN}"
                            )));
                        }
                        if !remainder_optional {
                            Err(illegal(format!(
                                "Property with key [{key}] using function [{}] returned null value which is not allowed, from input: {input}",
                                function.name()
                            )))
                        } else if keep {
                            Ok(Some(format!(
                                "{UNRESOLVED_PREFIX_TOKEN}{OPTIONAL_TOKEN}{key}{UNRESOLVED_SUFFIX_TOKEN}"
                            )))
                        } else {
                            Ok(None)
                        }
                    }
                };
            }
        }

        // the key may have a default value
        let (key, default_value) = match key.split_once(GET_OR_ELSE_TOKEN) {
            Some((k, d)) if self.config.default_fallback_enabled => (k, Some(d)),
            _ => (key, None),
        };
        let mut value = self.lookup(key);
        if value.is_none() {
            if let Some(d) = default_value {
                log::debug!("Property with key [{key}] not found, using default value");
                value = Some(d.to_string());
            }
        }
        match value {
            Some(v) => Ok(Some(v)),
            None if !optional && ignore_missing => Ok(Some(format!(
                "{UNRESOLVED_PREFIX_TOKEN}{key}{UNRESOLVED_SUFFIX_TOKEN}"
            ))),
            None if !optional => Err(illegal(format!(
                "Property with key [{key}] not found in properties from text: {input}"
            ))),
            None if keep => Ok(Some(format!(
                "{UNRESOLVED_PREFIX_TOKEN}{OPTIONAL_TOKEN}{key}{UNRESOLVED_SUFFIX_TOKEN}"
            ))),
            None => Ok(None),
        }
    }

    /// The value of the property (Java `doGetPropertyValue`): the environment variables and
    /// system properties in override mode, the properties of the component, then the
    /// environment variables and system properties in fallback mode.
    fn lookup(&self, key: &str) -> Option<String> {
        if key.is_empty() {
            return None;
        }
        let c = self.config;
        let env = || lookup_environment_variable(key);
        let sys = || system_properties::get_property(key);
        let mut value = None;
        if c.environment_variable_mode == MODE_OVERRIDE {
            value = env();
        }
        if value.is_none() && c.system_properties_mode == MODE_OVERRIDE {
            value = sys();
        }
        if value.is_none() {
            value = c.lookup(key);
        }
        if value.is_none() && c.environment_variable_mode == MODE_FALLBACK {
            value = env();
        }
        if value.is_none() && c.system_properties_mode == MODE_FALLBACK {
            value = sys();
        }
        value
    }
}

/// Whether the token at the byte index is between the same quotes (`'{{'`).
fn is_quoted(input: &str, index: usize, token: &str) -> bool {
    let bytes = input.as_bytes();
    let after = index + token.len();
    if index >= 1 && after < bytes.len() {
        let (b, a) = (bytes[index - 1], bytes[after]);
        return b == a && (b == b'\'' || b == b'"');
    }
    false
}

/// Whether the escape character is at the byte index, and is not itself escaped.
fn is_escaped(input: &str, index: Option<usize>) -> bool {
    let bytes = input.as_bytes();
    match index {
        Some(i) if i < bytes.len() => bytes[i] == b'\\' && (i == 0 || bytes[i - 1] != b'\\'),
        _ => false,
    }
}

/// The first suffix token not quoted nor escaped.
fn suffix_index(input: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(i) = input[start..].find(SUFFIX_TOKEN).map(|i| i + start) {
        if !(is_quoted(input, i, SUFFIX_TOKEN) || is_escaped(input, i.checked_sub(1))) {
            return Some(i);
        }
        start = i + 1;
    }
    None
}

/// The prefix token matching the suffix at the index (the last one before it), not quoted
/// nor escaped.
fn matching_prefix_index(input: &str, suffix: usize) -> Option<usize> {
    let mut end = suffix;
    while end > 0 {
        // lastIndexOf(PREFIX, end - 1): a match starting at or before end - 1
        let limit = (end - 1 + PREFIX_TOKEN.len()).min(input.len());
        let i = input[..limit].rfind(PREFIX_TOKEN)?;
        if !(is_quoted(input, i, PREFIX_TOKEN) || is_escaped(input, i.checked_sub(1))) {
            return Some(i);
        }
        end = i;
    }
    None
}
