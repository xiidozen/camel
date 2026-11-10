//! Helper for Camel OGNL (Object-Graph Navigation Language) expressions.
//!
//! Port of `org.apache.camel.util.OgnlHelper`.

use crate::key_value_holder::KeyValueHolder;
use crate::lang::{is_java_identifier_part, is_java_identifier_start};
use crate::string_helper::{before_or, count_char};
use regex::Regex;
use std::sync::OnceLock;

/// Tests whether or not the given String is a Camel OGNL expression.
///
/// An expression is considered an OGNL expression when it contains either one of the following
/// chars: `.` or `[` (brackets must come in pairs).
///
/// # Arguments
///
/// * `expression` - the String
///
/// # Returns
///
/// `true` if a Camel OGNL expression, otherwise `false`.
pub fn is_valid_ognl_expression(expression: &str) -> bool {
    if expression.is_empty() {
        return false;
    }
    // the brackets should come in a pair
    let bracket_begin = count_char(expression, '[');
    let bracket_end = count_char(expression, ']');
    if bracket_begin > 0 && bracket_end > 0 {
        return bracket_begin == bracket_end;
    }
    expression.contains('.')
}

/// Tests whether the given expression looks like an OGNL expression but is invalid (unbalanced
/// brackets or double dots).
pub fn is_invalid_valid_ognl_expression(expression: &str) -> bool {
    if !expression.contains(['.', '[', ']']) {
        return false;
    }
    // the brackets should come in pair
    let bracket_begin = count_char(expression, '[');
    let bracket_end = count_char(expression, ']');
    if bracket_begin > 0 || bracket_end > 0 {
        return bracket_begin != bracket_end;
    }
    // check for double dots
    expression.contains("..")
}

/// Validates whether the method name is using valid java identifiers in the name.
///
/// # Errors
///
/// Returns an error message (the Java version threw `IllegalArgumentException`) if the method
/// name is invalid.
pub fn validate_method_name(method: &str) -> Result<(), String> {
    for (i, ch) in method.chars().enumerate() {
        if i == 0 && ch == '.' {
            // its a dot before a method name
            continue;
        }
        if matches!(ch, '(' | '[' | '.' | '?') {
            // break when method name ends and sub method or arguments begin
            break;
        }
        if i == 0 && !is_java_identifier_start(ch) {
            return Err(format!(
                "Method name must start with a valid java identifier at position: 0 in method: {method}"
            ));
        } else if !is_java_identifier_part(ch) {
            return Err(format!(
                "Method name must be valid java identifier at position: {i} in method: {method}"
            ));
        }
    }
    Ok(())
}

/// Tests whether or not the given Camel OGNL expression is using the null safe operator or not.
///
/// # Arguments
///
/// * `ognl_expression` - the Camel OGNL expression
///
/// # Returns
///
/// `true` if the null safe operator (`?`) is used, otherwise `false`.
pub fn is_null_safe_operator(ognl_expression: &str) -> bool {
    ognl_expression.starts_with('?')
}

/// Removes any leading operators from the Camel OGNL expression.
///
/// Will remove any leading of the following chars: `?` or `.`
///
/// # Arguments
///
/// * `ognl_expression` - the Camel OGNL expression
///
/// # Returns
///
/// The Camel OGNL expression without any leading operators.
pub fn remove_leading_operators(ognl_expression: &str) -> &str {
    let s = ognl_expression.strip_prefix('?').unwrap_or(ognl_expression);
    s.strip_prefix('.').unwrap_or(s)
}

/// Removes any trailing operators from the Camel OGNL expression.
///
/// # Arguments
///
/// * `ognl_expression` - the Camel OGNL expression
///
/// # Returns
///
/// The Camel OGNL expression without any trailing operators (anything from the first `[`).
pub fn remove_trailing_operators(ognl_expression: &str) -> String {
    before_or(ognl_expression, "[", ognl_expression)
}

/// Removes both trailing and leading operators from the Camel OGNL expression.
pub fn remove_operators(ognl_expression: &str) -> String {
    remove_leading_operators(&remove_trailing_operators(ognl_expression)).to_string()
}

/// Whether the expression is an index expression such as `foo[bar]`.
///
/// # Returns
///
/// `None` if not an index expression, otherwise the key (before `[`) and value (between the
/// brackets); empty parts are returned as `None`.
///
/// # Examples
///
/// ```
/// use camel_util::ognl_helper::is_ognl_index;
/// let kv = is_ognl_index("foo[0]").unwrap();
/// assert_eq!(Some("foo".to_string()), *kv.key());
/// assert_eq!(Some("0".to_string()), *kv.value());
/// assert!(is_ognl_index("foo").is_none());
/// ```
pub fn is_ognl_index(
    ognl_expression: &str,
) -> Option<KeyValueHolder<Option<String>, Option<String>>> {
    static INDEX_PATTERN: OnceLock<Regex> = OnceLock::new();
    let re = INDEX_PATTERN.get_or_init(|| Regex::new(r"(?s)^(.*)\[(.*)\]$").unwrap());
    let caps = re.captures(ognl_expression)?;
    // to avoid empty strings as we want key/value to be None in such cases
    let non_empty = |i: usize| {
        caps.get(i)
            .map(|m| m.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    Some(KeyValueHolder::new(non_empty(1), non_empty(2)))
}

/// Splits the OGNL expression into methods.
///
/// Regular expressions with repeating groups are a pain to get right and then nobody
/// understands the regexp afterwards, so a bit low-level code is used to split the OGNL into
/// methods. Dots inside `[...]` (map keys), inside `(...)` (method arguments) and inside quotes
/// are not method separators.
///
/// # Arguments
///
/// * `ognl` - the ognl expression
///
/// # Returns
///
/// A list of methods; an empty list if the ognl expression has no methods.
///
/// # Errors
///
/// Returns an error if the last method has a missing ending parenthesis.
///
/// # Examples
///
/// ```
/// use camel_util::ognl_helper::split_ognl;
/// assert_eq!(vec!["foo['key']", "?.bar[0]"], split_ognl("foo['key']?.bar[0]").unwrap());
/// assert_eq!(
///     vec!["foo(${body}, ${header.foo})", ".bar"],
///     split_ognl("foo(${body}, ${header.foo}).bar").unwrap()
/// );
/// ```
pub fn split_ognl(ognl: &str) -> Result<Vec<String>, String> {
    if ognl.trim().is_empty() {
        return Ok(Vec::new());
    }
    let chars: Vec<char> = ognl.chars().collect();
    let mut methods: Vec<String> = Vec::with_capacity(4);
    let mut sb = String::with_capacity(256);
    let mut j = 0; // counter per method
    let mut square_bracket_cnt: i32 = 0;
    let mut parenthesis_bracket_cnt: i32 = 0;
    let mut single_quoted = false;
    let mut double_quoted = false;
    for (i, &ch) in chars.iter().enumerate() {
        if !double_quoted && ch == '\'' {
            single_quoted = !single_quoted;
        } else if !single_quoted && ch == '"' {
            double_quoted = !double_quoted;
        }
        if single_quoted || double_quoted {
            // quoted text so append as literal text
            sb.push(ch);
            continue;
        }
        // special for starting a new method
        if j == 0 || (j == 1 && chars[i - 1] == '?') || (ch != '.' && ch != '?' && ch != ']') {
            sb.push(ch);
            if ch == '[' && parenthesis_bracket_cnt == 0 {
                square_bracket_cnt += 1;
            } else if ch == '(' {
                parenthesis_bracket_cnt += 1;
            } else if ch == ')' {
                parenthesis_bracket_cnt -= 1;
            }
            j += 1;
        } else {
            if ch == '.' && square_bracket_cnt == 0 && parenthesis_bracket_cnt == 0 {
                // a dot denotes end of this method and a new method is to be invoked (only when
                // not inside a square bracket block, as dots can be used in map key names)
                let mut s = std::mem::take(&mut sb);
                // pass over ? to the new method
                if s.ends_with('?') {
                    sb.push('?');
                    s.pop();
                }
                methods.push(s);
                j = 0;
            } else if ch == ']' && parenthesis_bracket_cnt == 0 {
                // append ending ] to method name
                sb.push(ch);
                methods.push(std::mem::take(&mut sb));
                j = 0;
                square_bracket_cnt -= 1;
            }
            // and don't lose the char if its not an ] end marker (as we already added that)
            if ch != ']' || parenthesis_bracket_cnt > 0 {
                sb.push(ch);
            }
            // only advance if already begun on the new method
            if j > 0 {
                j += 1;
            }
        }
    }
    if !sb.is_empty() {
        methods.push(sb);
    }
    if parenthesis_bracket_cnt > 0 {
        if let Some(last) = methods.last() {
            // there is an unclosed parenthesis bracket on the last method
            if last.contains('(') && !last.ends_with(')') {
                return Err(format!("Method should end with parenthesis, was {last}"));
            }
        }
    }
    Ok(methods)
}

/// Converts single quoted method arguments into double quotes (unescaping `\'` inside single
/// quotes).
///
/// # Examples
///
/// ```
/// use camel_util::ognl_helper::method_as_double_quotes;
/// assert_eq!(
///     "${bodyAs(String).compareTo(\"It's a great World\")}",
///     method_as_double_quotes("${bodyAs(String).compareTo('It\\'s a great World')}")
/// );
/// ```
pub fn method_as_double_quotes(ognl: &str) -> String {
    let chars: Vec<char> = ognl.chars().collect();
    let mut sb = String::with_capacity(ognl.len() + 32);
    let mut single_bracket_cnt = 0;
    let mut double_bracket_cnt = 0;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if ch == '\\' && next == '\'' && single_bracket_cnt % 2 != 0 {
            // an escaped single quote inside an existing quote, then unescape it
            sb.push('\'');
            i += 2;
            continue;
        }
        if double_bracket_cnt % 2 == 0 && ch == '\'' {
            single_bracket_cnt += 1;
            sb.push('"');
        } else if single_bracket_cnt % 2 == 0 && ch == '"' {
            double_bracket_cnt += 1;
            sb.push('"');
        } else {
            sb.push(ch);
        }
        i += 1;
    }
    sb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<String> {
        split_ognl(s).unwrap()
    }

    // --- camel-util OgnlHelperTest ---

    #[test]
    fn split_ognl_with_regex_in_method() {
        let ognl = "header.cookie.replaceFirst(\".*;?iwanttoknow=([^;]+);?.*\", \"$1\")";
        assert!(!is_invalid_valid_ognl_expression(ognl));
        assert!(is_valid_ognl_expression(ognl));
        assert_eq!(
            vec![
                "header",
                ".cookie",
                ".replaceFirst(\".*;?iwanttoknow=([^;]+);?.*\", \"$1\")"
            ],
            split(ognl)
        );
    }

    #[test]
    fn split_ognl_with_parenthesis_in_quotes() {
        let ognl = "body.replace(\"((\", \"--\")";
        assert!(!is_invalid_valid_ognl_expression(ognl));
        assert!(is_valid_ognl_expression(ognl));
        assert_eq!(vec!["body", ".replace(\"((\", \"--\")"], split(ognl));
    }

    #[test]
    fn split_ognl_with_parenthesis_in_quotes_two() {
        let ognl = "body.replace(\"((\", \"--\").replace(\"((((\", \"----\")";
        assert!(!is_invalid_valid_ognl_expression(ognl));
        assert!(is_valid_ognl_expression(ognl));
        assert_eq!(
            vec![
                "body",
                ".replace(\"((\", \"--\")",
                ".replace(\"((((\", \"----\")"
            ],
            split(ognl)
        );
    }

    // --- camel-core OgnlHelperTest ---

    #[test]
    fn test_split_ognl_simple() {
        assert!(split("").is_empty());
        assert!(split(" ").is_empty());
        assert_eq!(vec!["foo"], split("foo"));
        assert_eq!(vec!["foo", ".bar"], split("foo.bar"));
        assert_eq!(vec!["foo", ".bar", ".baz"], split("foo.bar.baz"));
    }

    #[test]
    fn test_split_ognl_square() {
        assert_eq!(vec!["foo"], split("foo"));
        assert_eq!(vec!["foo[0]", ".bar"], split("foo[0].bar"));
        assert_eq!(vec!["foo[0]", "?.bar"], split("foo[0]?.bar"));
        assert_eq!(vec!["foo['key']", ".bar"], split("foo['key'].bar"));
        assert_eq!(vec!["foo['key']", "?.bar"], split("foo['key']?.bar"));
        assert_eq!(vec!["foo['key']", ".bar[0]"], split("foo['key'].bar[0]"));
        assert_eq!(vec!["foo['key']", "?.bar[0]"], split("foo['key']?.bar[0]"));
    }

    #[test]
    fn test_split_ognl_parenthesis() {
        assert_eq!(vec!["foo()"], split("foo()"));
        assert_eq!(vec!["foo(${body})"], split("foo(${body})"));
        assert_eq!(
            vec!["foo(${body}, ${header.foo})"],
            split("foo(${body}, ${header.foo})")
        );
        assert_eq!(
            vec!["foo(${body}, ${header.foo})", ".bar"],
            split("foo(${body}, ${header.foo}).bar")
        );
        assert_eq!(
            vec!["foo(${body}, ${header.foo})", ".bar(true, ${header.bar})"],
            split("foo(${body}, ${header.foo}).bar(true, ${header.bar})")
        );
        assert_eq!(
            vec![
                "foo(${body}, ${header.foo})",
                ".bar(true, ${header.bar})",
                ".baz['key']"
            ],
            split("foo(${body}, ${header.foo}).bar(true, ${header.bar}).baz['key']")
        );
    }

    #[test]
    fn test_split_ognl_parenthesis_and_bracket() {
        assert_eq!(vec!["foo(${body['key']})"], split("foo(${body['key']})"));
        assert_eq!(
            vec!["foo(${body}, ${header.foo?['key']})"],
            split("foo(${body}, ${header.foo?['key']})")
        );
        assert_eq!(
            vec![
                "foo(${body}, ${header.foo})",
                ".bar(true, ${header.bar[0]?.code})"
            ],
            split("foo(${body}, ${header.foo}).bar(true, ${header.bar[0]?.code})")
        );
    }

    #[test]
    fn test_method_as_double_quotes() {
        assert_eq!(
            "${bodyAs(String).compareTo(\"It's a great World\")}",
            method_as_double_quotes("${bodyAs(String).compareTo('It\\'s a great World')}")
        );
        assert_eq!(
            "${bodyAs(String).compareTo(\"It's a great World\")}",
            method_as_double_quotes("${bodyAs(String).compareTo(\"It's a great World\")}")
        );
    }

    // --- additional coverage ---

    #[test]
    fn test_misc() {
        assert!(split_ognl("foo.bar(1").is_err());
        assert!(validate_method_name("foo").is_ok());
        assert!(validate_method_name(".foo(1)").is_ok());
        assert!(validate_method_name("1foo").is_err());
        assert!(validate_method_name("fo-o").is_err());
        assert!(is_null_safe_operator("?.foo"));
        assert_eq!("foo", remove_leading_operators("?.foo"));
        assert_eq!("foo", remove_operators("?.foo[0]"));
        assert!(is_invalid_valid_ognl_expression("foo..bar"));
        assert!(is_invalid_valid_ognl_expression("foo[bar"));
        assert!(!is_invalid_valid_ognl_expression("foo"));
        assert!(!is_valid_ognl_expression("foo"));
        let kv = is_ognl_index("[x]").unwrap();
        assert_eq!(None, *kv.key());
    }
}
