//! Helper methods for working with Strings. Port of `org.apache.camel.util.StringHelper`.
//!
//! Java methods that return `null` return `Option<String>` here. Methods whose Java
//! counterpart only returned `null` for a `null` input take `&str` (Rust has no null
//! references), so the `null`-input assertions of the Java tests are enforced by the
//! type system instead.

use crate::ant_path_matcher::AntPathMatcher;
use crate::lang::{
    char_to_lower, char_to_upper, is_java_identifier_part, is_java_identifier_start,
    is_java_whitespace, java_split, java_trim,
};
use crate::string_quote_helper::double_quote;
use regex::Regex;

/// Ensures that `s` is friendly for a URL or file system.
///
/// Replaces `:`, `_`, `.`, `/` and `\` with `-`.
///
/// # Arguments
///
/// * `s` - string to be sanitized.
///
/// # Returns
///
/// The sanitized version of `s`.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::sanitize;
/// assert_eq!("c--hello-world", sanitize("c:\\hello.world"));
/// ```
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ':' | '_' | '.' | '/' | '\\' => '-',
            c => c,
        })
        .collect()
}

/// Remove carriage return and line feeds from a String, replacing them with an empty String.
///
/// # Arguments
///
/// * `s` - string to be sanitized of carriage return / line feed characters.
///
/// # Returns
///
/// The sanitized version of `s`.
pub fn remove_crlf(s: &str) -> String {
    s.replace(['\r', '\n'], "")
}

/// Counts the number of times the given char is in the string.
///
/// # Arguments
///
/// * `s` - the string
/// * `ch` - the char
///
/// # Returns
///
/// Number of times the char is located in the string.
pub fn count_char(s: &str, ch: char) -> usize {
    s.chars().filter(|&c| c == ch).count()
}

/// Counts the number of times the given char is in the string, looking only at the first `end`
/// characters.
///
/// # Arguments
///
/// * `s` - the string
/// * `ch` - the char
/// * `end` - end index (exclusive), counted in characters
///
/// # Returns
///
/// Number of times the char is located in the string.
pub fn count_char_until(s: &str, ch: char, end: usize) -> usize {
    s.chars().take(end).filter(|&c| c == ch).count()
}

/// Limits the length of a string.
///
/// # Arguments
///
/// * `s` - the string
/// * `max_length` - the maximum length (in characters) of the returned string
///
/// # Returns
///
/// `s` if its length is at most `max_length`, otherwise the first `max_length` characters of `s`.
pub fn limit_length(s: &str, max_length: usize) -> String {
    limit_length_with_suffix(s, max_length, None)
}

/// Limits the length of a string, appending a suffix when the string was cut.
///
/// # Arguments
///
/// * `s` - the string
/// * `max_length` - the maximum length (in characters) of the returned string
/// * `suffix` - suffix to append if the string was limited, such as an ellipsis
///
/// # Returns
///
/// `s` if its length is at most `max_length`, otherwise the first `max_length` characters of `s`
/// followed by the suffix.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::limit_length_with_suffix;
/// assert_eq!("ABC...", limit_length_with_suffix("ABCDEF", 3, Some("...")));
/// ```
pub fn limit_length_with_suffix(s: &str, max_length: usize, suffix: Option<&str>) -> String {
    if s.chars().count() <= max_length {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_length).collect();
    if let Some(suffix) = suffix {
        out.push_str(suffix);
    }
    out
}

/// Removes all quotes (single and double) from the string.
///
/// # Arguments
///
/// * `s` - the string
///
/// # Returns
///
/// The string without quotes (single and double).
pub fn remove_quotes(s: &str) -> String {
    s.replace(['\'', '"'], "")
}

/// Removes all leading and ending quotes (single and double) from the string.
///
/// The string is trimmed before checking for quotes. If it is not quoted it is returned as-is
/// (untrimmed).
///
/// # Arguments
///
/// * `s` - the string
///
/// # Returns
///
/// The string without leading and ending quotes (single and double).
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::remove_leading_and_ending_quotes;
/// assert_eq!("Hello World", remove_leading_and_ending_quotes("'Hello World'"));
/// assert_eq!("Hello 'Camel'", remove_leading_and_ending_quotes("Hello 'Camel'"));
/// ```
pub fn remove_leading_and_ending_quotes(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let copy = java_trim(s);
    if copy.chars().count() < 2 {
        return s.to_string();
    }
    if (copy.starts_with('\'') && copy.ends_with('\''))
        || (copy.starts_with('"') && copy.ends_with('"'))
    {
        return copy[1..copy.len() - 1].to_string();
    }
    s.to_string()
}

/// Whether the string starts and ends with either single or double quotes.
///
/// # Arguments
///
/// * `s` - the string
///
/// # Returns
///
/// `true` if the string starts and ends with either single or double quotes.
pub fn is_quoted(s: &str) -> bool {
    is_single_quoted(s) || is_double_quoted(s)
}

/// Whether the string starts and ends with single quotes.
///
/// # Arguments
///
/// * `s` - the string
///
/// # Returns
///
/// `true` if the string starts and ends with single quotes.
pub fn is_single_quoted(s: &str) -> bool {
    !s.is_empty() && s.starts_with('\'') && s.ends_with('\'')
}

/// Whether the string starts and ends with double quotes.
///
/// # Arguments
///
/// * `s` - the string
///
/// # Returns
///
/// `true` if the string starts and ends with double quotes.
pub fn is_double_quoted(s: &str) -> bool {
    !s.is_empty() && s.starts_with('"') && s.ends_with('"')
}

/// Encodes the text into safe XML by replacing `<`, `>`, `&`, `"` and `'` with XML tokens.
///
/// `&` is replaced first so already produced tokens are not encoded twice.
///
/// # Arguments
///
/// * `text` - the text
///
/// # Returns
///
/// The encoded text.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::xml_encode;
/// assert_eq!("&lt;a&gt; &amp; b", xml_encode("<a> & b"));
/// ```
pub fn xml_encode(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Decodes the text by replacing XML tokens with character values.
///
/// This is the inverse of [`xml_encode`].
///
/// # Arguments
///
/// * `text` - the text
///
/// # Returns
///
/// The decoded text.
pub fn xml_decode(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Determines if the string has at least one letter in upper case.
///
/// # Arguments
///
/// * `text` - the text
///
/// # Returns
///
/// `true` if at least one letter is upper case, `false` otherwise.
pub fn has_upper_case(text: &str) -> bool {
    text.chars().any(char::is_uppercase)
}

/// Determines if the string is a fully qualified class name.
///
/// A text is considered a class name when it contains a `.` (not as the first character) and the
/// character after the last `.` is upper case, e.g. `com.foo.MyClass`.
///
/// # Arguments
///
/// * `text` - the text
///
/// # Returns
///
/// `true` if the text looks like a fully qualified class name.
pub fn is_class_name(text: &str) -> bool {
    match text.rfind('.') {
        None | Some(0) => false,
        Some(pos) => text[pos + 1..]
            .chars()
            .next()
            .map(char::is_uppercase)
            .unwrap_or(false),
    }
}

/// Does the expression have the language start token?
///
/// For the `simple` language the start token `${` is also accepted; otherwise the token must be
/// `$<language>{`.
///
/// # Arguments
///
/// * `expression` - the expression
/// * `language` - the name of the language, such as `simple`
///
/// # Returns
///
/// `true` if the expression contains the start token, `false` otherwise.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::has_start_token;
/// assert!(has_start_token("${body}", Some("simple")));
/// assert!(has_start_token("$foo{body}", Some("foo")));
/// assert!(!has_start_token("${body}", Some("foo")));
/// ```
pub fn has_start_token(expression: &str, language: Option<&str>) -> bool {
    let Some(language) = language else {
        return false;
    };
    if language.eq_ignore_ascii_case("simple") && expression.contains("${") {
        return true;
    }
    expression.contains(&format!("${language}{{"))
}

/// Replaces the first `from` token in the given input string.
///
/// This implementation is not recursive, nor does it check for tokens in the replacement string.
///
/// # Arguments
///
/// * `input` - the input string
/// * `from` - the from string
/// * `to` - the replacement string
///
/// # Returns
///
/// The replaced string, or the input string if no replacement was needed.
pub fn replace_first(input: &str, from: &str, to: &str) -> String {
    input.replacen(from, to, 1)
}

/// Creates a JSON tuple with the given name/value pair.
///
/// # Arguments
///
/// * `name` - the name
/// * `value` - the value
/// * `is_map` - whether the tuple should be a map (wrapped in `{ }`)
///
/// # Returns
///
/// The JSON text.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::to_json;
/// assert_eq!("{ \"a\": \"b\" }", to_json("a", "b", true));
/// ```
pub fn to_json(name: &str, value: &str, is_map: bool) -> String {
    if is_map {
        format!("{{ {}: {} }}", double_quote(name), double_quote(value))
    } else {
        format!("{}: {}", double_quote(name), double_quote(value))
    }
}

/// Asserts whether the string is **not** empty.
///
/// # Arguments
///
/// * `value` - the string to test
/// * `name` - the key that resolved the value
///
/// # Returns
///
/// The passed `value` as is.
///
/// # Errors
///
/// Returns `"<name> must be specified and not empty"` if the value is `None` or empty (the Java
/// version threw `IllegalArgumentException`).
pub fn not_empty<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("{name} must be specified and not empty")),
    }
}

/// Asserts whether the string is **not** empty.
///
/// # Arguments
///
/// * `value` - the string to test
/// * `name` - the key that resolved the value
/// * `on` - additional description to indicate where this problem occurred
///
/// # Returns
///
/// The passed `value` as is.
///
/// # Errors
///
/// Returns `"<name> must be specified and not empty on: <on>"` if the value is `None` or empty.
pub fn not_empty_on<'a>(value: Option<&'a str>, name: &str, on: &str) -> Result<&'a str, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(format!("{name} must be specified and not empty on: {on}")),
    }
}

/// Splits the value on the needle into an array of exactly `count` slots.
///
/// Slot `i` holds the text between the `i`-th and `i+1`-th needle; the last used slot holds the
/// remainder. Slots that are not reached (because there are fewer needles) are `None`.
///
/// # Arguments
///
/// * `value` - the text to split
/// * `needle` - the separator
/// * `count` - the number of slots in the result
///
/// # Returns
///
/// A vector of length `count`.
pub fn split_on_character(value: &str, needle: &str, count: usize) -> Vec<Option<String>> {
    let mut rc: Vec<Option<String>> = vec![None; count];
    if count == 0 {
        return rc;
    }
    rc[0] = Some(value.to_string());
    for i in 1..count {
        let v = rc[i - 1].clone().unwrap();
        match v.find(needle) {
            None => return rc,
            Some(p) => {
                rc[i - 1] = Some(v[..p].to_string());
                rc[i] = Some(v[p + needle.len()..].to_string());
            }
        }
    }
    rc
}

fn strip_needles(value: &str, needle: char, count: usize) -> (&str, usize) {
    let skip_start = value.starts_with(needle);
    let skip_end = value.ends_with(needle);
    let nl = needle.len_utf8();
    if skip_start && skip_end && value.len() >= 2 * nl {
        (&value[nl..value.len() - nl], count.saturating_sub(2))
    } else if skip_start {
        (&value[nl..], count.saturating_sub(1))
    } else if skip_end {
        (&value[..value.len() - nl], count.saturating_sub(1))
    } else {
        (value, count)
    }
}

/// Splits the value on the needle, returning an iterator over the parts.
///
/// A single leading and/or trailing needle is skipped. A value consisting only of the needle
/// yields no parts.
///
/// # Arguments
///
/// * `value` - the text to split
/// * `needle` - the separator
/// * `count` - the expected number of parts (usually number of needles + 1)
///
/// # Returns
///
/// An iterator over at most `count` parts.
pub fn split_on_character_as_iterator(
    value: &str,
    needle: char,
    count: usize,
) -> std::vec::IntoIter<String> {
    split_on_character_as_list(value, needle, count).into_iter()
}

/// Splits the value on the needle, returning the parts as a list.
///
/// A single leading and/or trailing needle is skipped. A value consisting only of the needle
/// yields an empty list.
///
/// # Arguments
///
/// * `value` - the text to split
/// * `needle` - the separator
/// * `count` - the expected number of parts (usually number of needles + 1)
///
/// # Returns
///
/// The list of at most `count` parts.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::split_on_character_as_list;
/// assert_eq!(vec!["foo", "bar"], split_on_character_as_list(",foo,bar,", ',', 4));
/// ```
pub fn split_on_character_as_list(value: &str, needle: char, count: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    if value.chars().count() == 1 && value.starts_with(needle) {
        return Vec::new();
    }
    let (value, count) = strip_needles(value, needle, count);
    let mut rc = Vec::with_capacity(count);
    let mut rest = value;
    for _ in 0..count {
        match rest.find(needle) {
            Some(end) => {
                rc.push(rest[..end].to_string());
                rest = &rest[end + needle.len_utf8()..];
            }
            None => {
                rc.push(rest.to_string());
                break;
            }
        }
    }
    rc
}

/// Removes any starting characters on the given text which match the given character.
///
/// # Arguments
///
/// * `text` - the string
/// * `ch` - the initial characters to remove
///
/// # Returns
///
/// Either the original string or the new substring.
pub fn remove_starting_characters(text: &str, ch: char) -> &str {
    text.trim_start_matches(ch)
}

/// Capitalize the string (upper case only first character).
///
/// # Arguments
///
/// * `text` - the string
///
/// # Returns
///
/// The string capitalized (upper case only first character).
pub fn capitalize(text: &str) -> String {
    do_capitalize(text, false)
}

/// Capitalize the string (upper case first character).
///
/// # Arguments
///
/// * `text` - the string
/// * `dash_to_camel` - whether to also convert dash format into camel case
///   (`hello-great-world` -> `HelloGreatWorld`)
///
/// # Returns
///
/// The string capitalized (upper case first character).
pub fn capitalize_dash(text: &str, dash_to_camel: bool) -> String {
    if dash_to_camel {
        do_capitalize(&dash_to_camel_case(text), false)
    } else {
        do_capitalize(text, false)
    }
}

/// Capitalize the string (upper case first character in every word).
///
/// A word starts after any whitespace character.
///
/// # Arguments
///
/// * `text` - the string
///
/// # Returns
///
/// The string capitalized (upper case first character in every word).
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::capitalize_all;
/// assert_eq!("Hello World", capitalize_all("hello world"));
/// ```
pub fn capitalize_all(text: &str) -> String {
    do_capitalize(text, true)
}

fn do_capitalize(text: &str, all: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    for (i, c) in text.chars().enumerate() {
        let up = i == 0 || (all && prev.map(is_java_whitespace).unwrap_or(false));
        out.push(if up { char_to_upper(c) } else { c });
        prev = Some(c);
    }
    out
}

/// De-capitalize the string (lower case first character).
///
/// # Arguments
///
/// * `text` - the string
///
/// # Returns
///
/// The string decapitalized (lower case first character).
pub fn decapitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out = String::with_capacity(text.len());
            out.push(char_to_lower(first));
            out.extend(chars);
            out
        }
    }
}

/// Whether the string contains dashes or not.
///
/// # Arguments
///
/// * `text` - the string to check
///
/// # Returns
///
/// `true` if it contains dashes, `false` otherwise.
pub fn is_dashed(text: &str) -> bool {
    text.contains('-')
}

/// Converts the string from dash format into camel case (`hello-great-world` -> `helloGreatWorld`).
///
/// # Arguments
///
/// * `text` - the string
///
/// # Returns
///
/// The string camel cased.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::dash_to_camel_case;
/// assert_eq!("helloGreatWorld", dash_to_camel_case("hello-great-world"));
/// ```
pub fn dash_to_camel_case(text: &str) -> String {
    if !is_dashed(text) {
        return text.to_string();
    }
    let mut sb = String::with_capacity(text.len());
    let mut upper = false;
    for c in text.chars() {
        if c == '-' {
            upper = true;
        } else {
            sb.push(if upper { char_to_upper(c) } else { c });
            upper = false;
        }
    }
    sb
}

/// Converts the string from dash format into camel case, optionally keeping text inside quotes or
/// keys as-is.
///
/// With `skip_quoted_or_keyed` an input such as
/// `camel.component.rabbitmq.args[queue.x-queue-type]` keeps the text inside `[...]` unchanged,
/// whereas without it the result is `camel.component.rabbitmq.args[queue.xQueueType]`.
///
/// # Arguments
///
/// * `text` - the string
/// * `skip_quoted_or_keyed` - flag to skip converting within a quoted or keyed text
///
/// # Returns
///
/// The string camel cased.
pub fn dash_to_camel_case_skip(text: &str, skip_quoted_or_keyed: bool) -> String {
    if !skip_quoted_or_keyed {
        return dash_to_camel_case(text);
    }
    if !is_dashed(text) {
        return text.to_string();
    }
    let mut sb = String::with_capacity(text.len());
    let mut upper = false;
    let mut single_quotes = 0;
    let mut double_quotes = 0;
    let mut skip = false;
    for c in text.chars() {
        if c == ']' {
            skip = false;
        } else if c == '[' {
            skip = true;
        } else if c == '\'' && !skip {
            single_quotes += 1;
        } else if c == '"' && !skip {
            double_quotes += 1;
        }
        let quoted = single_quotes % 2 == 1 || double_quotes % 2 == 1;
        if skip || quoted {
            sb.push(c);
            continue;
        }
        if c == '-' {
            upper = true;
        } else {
            sb.push(if upper { char_to_upper(c) } else { c });
            upper = false;
        }
    }
    sb
}

/// Returns the string after the given token.
///
/// # Arguments
///
/// * `text` - the text
/// * `after` - the token
///
/// # Returns
///
/// The text after the token, or `None` if the text does not contain the token.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::after;
/// assert_eq!(Some(" World".to_string()), after("Hello World", "Hello"));
/// assert_eq!(None, after("Hello", "Foo"));
/// ```
pub fn after(text: &str, after: &str) -> Option<String> {
    text.find(after)
        .map(|p| text[p + after.len()..].to_string())
}

/// Returns the string after the given token or the default value.
///
/// # Arguments
///
/// * `text` - the text
/// * `after_token` - the token
/// * `default` - the value to return if the text does not contain the token
///
/// # Returns
///
/// The text after the token, or the supplied default if the text does not contain the token.
pub fn after_or(text: &str, after_token: &str, default: &str) -> String {
    after(text, after_token).unwrap_or_else(|| default.to_string())
}

/// Returns an object after the given token.
///
/// # Arguments
///
/// * `text` - the text
/// * `after_token` - the token
/// * `mapper` - a mapping function to convert the string after the token to type `T`
///
/// # Returns
///
/// The result of applying the mapping function to the text after the token, or `None` if the
/// text does not contain the token.
pub fn after_map<T>(text: &str, after_token: &str, mapper: impl FnOnce(&str) -> T) -> Option<T> {
    after(text, after_token).map(|s| mapper(&s))
}

/// Returns the string after the last occurrence of the given token.
///
/// # Arguments
///
/// * `text` - the text
/// * `after` - the token
///
/// # Returns
///
/// The text after the token, or `None` if the text does not contain the token.
pub fn after_last(text: &str, after: &str) -> Option<String> {
    text.rfind(after)
        .map(|p| text[p + after.len()..].to_string())
}

/// Returns the string after the last occurrence of the given token, or the default value.
///
/// # Arguments
///
/// * `text` - the text
/// * `after` - the token
/// * `default` - the value to return if the text does not contain the token
///
/// # Returns
///
/// The text after the token, or the supplied default if the text does not contain the token.
pub fn after_last_or(text: &str, after: &str, default: &str) -> String {
    after_last(text, after).unwrap_or_else(|| default.to_string())
}

/// Returns the string before the given token.
///
/// # Arguments
///
/// * `text` - the text
/// * `before` - the token
///
/// # Returns
///
/// The text before the token, or `None` if the text does not contain the token.
pub fn before(text: &str, before: &str) -> Option<String> {
    text.find(before).map(|p| text[..p].to_string())
}

/// Returns the string before the given token, or the default value.
///
/// # Arguments
///
/// * `text` - the text
/// * `before_token` - the token
/// * `default` - the value to return if the text does not contain the token
///
/// # Returns
///
/// The text before the token, or the supplied default if the text does not contain the token.
pub fn before_or(text: &str, before_token: &str, default: &str) -> String {
    before(text, before_token).unwrap_or_else(|| default.to_string())
}

/// Returns the string before the given character, or the default value.
///
/// # Arguments
///
/// * `text` - the text
/// * `before` - the token character
/// * `default` - the value to return if the text does not contain the token
///
/// # Returns
///
/// The text before the token, or the supplied default if the text does not contain the token.
pub fn before_char_or(text: &str, before: char, default: &str) -> String {
    text.find(before)
        .map(|p| text[..p].to_string())
        .unwrap_or_else(|| default.to_string())
}

/// Returns an object before the given token.
///
/// # Arguments
///
/// * `text` - the text
/// * `before_token` - the token
/// * `mapper` - a mapping function to convert the string before the token to type `T`
///
/// # Returns
///
/// The result of applying the mapping function to the text before the token, or `None` if the
/// text does not contain the token.
pub fn before_map<T>(text: &str, before_token: &str, mapper: impl FnOnce(&str) -> T) -> Option<T> {
    before(text, before_token).map(|s| mapper(&s))
}

/// Returns the string before the last occurrence of the given token.
///
/// # Arguments
///
/// * `text` - the text
/// * `before` - the token
///
/// # Returns
///
/// The text before the token, or `None` if the text does not contain the token.
pub fn before_last(text: &str, before: &str) -> Option<String> {
    text.rfind(before).map(|p| text[..p].to_string())
}

/// Returns the string before the last occurrence of the given token, or the default value.
///
/// # Arguments
///
/// * `text` - the text
/// * `before` - the token
/// * `default` - the value to return if the text does not contain the token
///
/// # Returns
///
/// The text before the token, or the supplied default if the text does not contain the token.
pub fn before_last_or(text: &str, before: &str, default: &str) -> String {
    before_last(text, before).unwrap_or_else(|| default.to_string())
}

/// Returns the string between the given tokens.
///
/// # Arguments
///
/// * `text` - the text
/// * `after_token` - the before token
/// * `before_token` - the after token
///
/// # Returns
///
/// The text between the tokens, or `None` if the text does not contain the tokens.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::between;
/// assert_eq!(Some("foo bar".to_string()), between("Hello ${foo bar} how are you", "${", "}"));
/// ```
pub fn between(text: &str, after_token: &str, before_token: &str) -> Option<String> {
    after(text, after_token).and_then(|r| before(&r, before_token))
}

/// Returns an object between the given tokens.
///
/// # Arguments
///
/// * `text` - the text
/// * `after_token` - the before token
/// * `before_token` - the after token
/// * `mapper` - a mapping function to convert the string between the tokens to type `T`
///
/// # Returns
///
/// The result of applying the mapping function to the text between the tokens, or `None` if the
/// text does not contain the tokens.
pub fn between_map<T>(
    text: &str,
    after_token: &str,
    before_token: &str,
    mapper: impl FnOnce(&str) -> T,
) -> Option<T> {
    between(text, after_token, before_token).map(|s| mapper(&s))
}

/// Returns the substring between the given head and tail.
///
/// # Arguments
///
/// * `text` - the text
/// * `head` - number of characters to remove from the start
/// * `tail` - number of characters to remove from the end
///
/// # Returns
///
/// The substring between the given head and tail (empty if they overlap).
pub fn between_index(text: &str, head: usize, tail: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = head.min(chars.len());
    let rest = &chars[start..];
    let end = rest.len().saturating_sub(tail);
    rest[..end].iter().collect()
}

/// Returns the string between the most outer pair of tokens.
///
/// The number of token pairs must be even, i.e. there must be the same number of before and after
/// tokens, otherwise `None` is returned.
///
/// This implementation skips matching when the text is either single or double-quoted. For example
/// `${body.matches("foo('bar')")}` will not match the parenthesis from the quoted text.
///
/// # Arguments
///
/// * `text` - the text
/// * `before` - the before token
/// * `after` - the after token
///
/// # Returns
///
/// The text between the outer most tokens, or `None` if the text does not contain the tokens.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::between_outer_pair;
/// assert_eq!(Some("bar(baz)123".to_string()), between_outer_pair("foo(bar(baz)123)", '(', ')'));
/// assert_eq!(None, between_outer_pair("foo(bar(baz)123))", '(', ')'));
/// ```
pub fn between_outer_pair(text: &str, before: char, after: char) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut pos: Option<usize> = None;
    let mut pos2: Option<usize> = None;
    let mut count = 0;
    let mut count2 = 0;
    let mut single_quoted = false;
    let mut double_quoted = false;
    for (i, &ch) in chars.iter().enumerate() {
        if !double_quoted && ch == '\'' {
            single_quoted = !single_quoted;
        } else if !single_quoted && ch == '"' {
            double_quoted = !double_quoted;
        }
        if single_quoted || double_quoted {
            continue;
        }
        if ch == before {
            count += 1;
        } else if ch == after {
            count2 += 1;
        }
        if ch == before && pos.is_none() {
            pos = Some(i);
        } else if ch == after {
            pos2 = Some(i);
        }
    }
    let (pos, pos2) = (pos?, pos2?);
    if count != count2 || pos2 < pos {
        return None;
    }
    Some(chars[pos + 1..pos2].iter().collect())
}

/// Returns an object between the most outer pair of tokens.
///
/// See [`between_outer_pair`] for the matching rules.
///
/// # Arguments
///
/// * `text` - the text
/// * `before` - the before token
/// * `after` - the after token
/// * `mapper` - a mapping function to convert the string between the most outer pair of tokens to
///   type `T`
///
/// # Returns
///
/// The result of applying the mapping function, or `None` if the text does not contain the tokens.
pub fn between_outer_pair_map<T>(
    text: &str,
    before: char,
    after: char,
    mapper: impl FnOnce(&str) -> T,
) -> Option<T> {
    between_outer_pair(text, before, after).map(|s| mapper(&s))
}

/// Returns `true` if the given name is a valid Java identifier.
///
/// Kept from the Java code base because Camel configuration (class names, bean references) still
/// uses Java naming rules.
///
/// # Arguments
///
/// * `name` - the name to check
pub fn is_java_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if is_java_identifier_start(c) => chars.all(is_java_identifier_part),
        _ => false,
    }
}

/// Cleans the string to a pure Java identifier so it can be used as a class name.
///
/// Especially from XML DSLs people can have `\n`, `\t` or other characters in a name; these are
/// removed. The characters `.`, `[`, `]` and `-` are kept.
///
/// # Arguments
///
/// * `name` - the class name
///
/// # Returns
///
/// The normalized class name.
pub fn normalize_class_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c == '.' || c == '[' || c == ']' || c == '-' || is_java_identifier_part(c))
        .collect()
}

/// Converts JVM internal class name descriptors to human-readable form.
///
/// For example `[B` becomes `byte[]`, `[Ljava.util.HashMap$Node;` becomes
/// `java.util.HashMap$Node[]`, and `[[I` becomes `int[][]`.
///
/// # Arguments
///
/// * `name` - the JVM internal class name
///
/// # Returns
///
/// The human-readable class name, or the input unchanged if it is not an array descriptor.
pub fn readable_class_name(name: &str) -> String {
    if !name.starts_with('[') {
        return name.to_string();
    }
    let dims = name.chars().take_while(|&c| c == '[').count();
    let suffix = "[]".repeat(dims);
    let rest = &name[dims..];
    let Some(first) = rest.chars().next() else {
        return name.to_string();
    };
    let element = match first {
        'B' => "byte".to_string(),
        'C' => "char".to_string(),
        'D' => "double".to_string(),
        'F' => "float".to_string(),
        'I' => "int".to_string(),
        'J' => "long".to_string(),
        'S' => "short".to_string(),
        'Z' => "boolean".to_string(),
        'L' => rest[1..]
            .strip_suffix(';')
            .unwrap_or(&rest[1..])
            .to_string(),
        _ => rest.to_string(),
    };
    element + &suffix
}

/// Compares old and new text content and reports back which lines are changed.
///
/// # Arguments
///
/// * `old_text` - the old text (`None` means there is nothing to compare)
/// * `new_text` - the new text
///
/// # Returns
///
/// A list of (0-based) line numbers that are changed in the new text.
pub fn changed_lines(old_text: Option<&str>, new_text: &str) -> Vec<usize> {
    let Some(old_text) = old_text else {
        return Vec::new();
    };
    if old_text == new_text {
        return Vec::new();
    }
    let old_lines = java_split(old_text, "\n");
    let new_lines = java_split(new_text, "\n");
    new_lines
        .iter()
        .enumerate()
        .filter(|(i, line)| old_lines.get(*i).map(|o| o != *line).unwrap_or(true))
        .map(|(i, _)| i)
        .collect()
}

/// Removes the leading and trailing whitespace and if the resulting string is empty returns `None`.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::trim_to_null;
/// assert_eq!(Some("abc".to_string()), trim_to_null(" abc "));
/// assert_eq!(None, trim_to_null(" "));
/// assert_eq!(None, trim_to_null(""));
/// ```
pub fn trim_to_null(given: &str) -> Option<String> {
    let t = java_trim(given);
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Checks if the `src` string contains `what`, ignoring case.
///
/// # Arguments
///
/// * `src` - the source string to be checked
/// * `what` - the string which will be looked up in `src`
///
/// # Returns
///
/// `true` if `what` is contained in `src` (an empty `what` is always contained).
pub fn contains_ignore_case(src: &str, what: &str) -> bool {
    if what.is_empty() {
        return true;
    }
    src.to_lowercase().contains(&what.to_lowercase())
}

/// Outputs the bytes in human-readable format in units of KB, MB, GB etc.
///
/// The Java version accepted a `Locale`; the Rust port always uses `.` as the decimal separator
/// and rounds half-up like Java's `%.1f`.
///
/// # Arguments
///
/// * `bytes` - number of bytes
///
/// # Returns
///
/// Human readable output.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::human_readable_bytes;
/// assert_eq!("32 B", human_readable_bytes(32));
/// assert_eq!("1.7 KB", human_readable_bytes(1730));
/// ```
pub fn human_readable_bytes(bytes: u64) -> String {
    let unit = 1024f64;
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let exp = ((bytes as f64).ln() / unit.ln()) as i32;
    let pre = "KMGTPE".chars().nth((exp - 1) as usize).unwrap();
    let value = bytes as f64 / unit.powi(exp);
    // Java's %.1f rounds HALF_UP
    let rounded = (value * 10.0 + 0.5).floor() / 10.0;
    format!("{rounded:.1} {pre}B")
}

/// Check for string pattern matching with a number of strategies in the following order:
///
/// 1. equals
/// 2. `None` pattern always matches
/// 3. `*` always matches
/// 4. Ant style matching (see [`AntPathMatcher`])
/// 5. Regular expression (the whole target must match)
///
/// # Arguments
///
/// * `pattern` - the pattern
/// * `target` - the string to test
///
/// # Returns
///
/// `true` if the target matches the pattern. An invalid regular expression never matches.
pub fn matches(pattern: Option<&str>, target: &str) -> bool {
    let Some(pattern) = pattern else {
        return true;
    };
    if pattern == target || pattern == "*" {
        return true;
    }
    if AntPathMatcher::new().match_(pattern, target) {
        return true;
    }
    Regex::new(&format!("^(?:{pattern})$"))
        .map(|re| re.is_match(target))
        .unwrap_or(false)
}

/// Converts the string from camel case into dot format (`helloGreatWorld` -> `hello.great.world`).
///
/// # Arguments
///
/// * `text` - the string
///
/// # Returns
///
/// The string dot cased.
pub fn camel_case_to_dot(text: &str) -> String {
    camel_case_to_dash(text).replace('-', ".")
}

/// Converts the string from camel case into dash format (`helloGreatWorld` -> `hello-great-world`).
///
/// Underscores are turned into dashes, and runs of upper case letters (acronyms) are kept together.
///
/// # Arguments
///
/// * `text` - the string
///
/// # Returns
///
/// The string dash cased.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::camel_case_to_dash;
/// assert_eq!("hello-great-world", camel_case_to_dash("helloGreatWorld"));
/// assert_eq!("use-mdc-logging", camel_case_to_dash("UseMDCLogging"));
/// ```
pub fn camel_case_to_dash(text: &str) -> String {
    let arr: Vec<char> = text.chars().collect();
    let mut answer = String::with_capacity(arr.len() + 8);
    let mut prev: Option<char> = None;
    for (i, &ch) in arr.iter().enumerate() {
        if ch == '-' || ch == '_' {
            answer.push('-');
        } else if ch.is_uppercase() && prev.is_some() {
            let p = prev.unwrap();
            let next = arr.get(i + 1).copied();
            if !p.is_uppercase() || next.map(char::is_lowercase).unwrap_or(false) {
                if p != '-' && p != '_' {
                    answer.push('-');
                }
                answer.push(char_to_lower(ch));
            } else {
                answer.push(char_to_lower(ch));
            }
        } else {
            answer.push(char_to_lower(ch));
        }
        prev = Some(ch);
    }
    answer
}

/// Does the string start with the given prefix (ignoring the case).
///
/// # Arguments
///
/// * `text` - the string
/// * `prefix` - the prefix
pub fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    let mut t = text.chars();
    for p in prefix.chars() {
        match t.next() {
            Some(c)
                if c == p
                    || char_to_upper(c) == char_to_upper(p)
                    || char_to_lower(c) == char_to_lower(p) => {}
            _ => return false,
        }
    }
    true
}

/// Converts the value to an enum constant value that is in the form of upper-cased with
/// underscore.
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::as_enum_constant_value;
/// assert_eq!("HELLO_WORLD", as_enum_constant_value("helloWorld"));
/// ```
pub fn as_enum_constant_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let ret = camel_case_to_dash(value);
    let re = Regex::new("-+").unwrap();
    re.replace_all(&ret, "-").replace('-', "_").to_uppercase()
}

/// Split the text on words, eg `hello/world` becomes a list with `hello` at index 0 and `world` at
/// index 1.
pub fn split_words(text: &str) -> Vec<String> {
    java_split(text, "[\\W]+")
}

/// Splits the input sequence around matches of the regex.
///
/// # Arguments
///
/// * `text` - the input
/// * `regex` - the expression used to split the input
///
/// # Returns
///
/// The strings computed by splitting the input with the given regex.
pub fn split_as_stream(text: &str, regex: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    java_split(text, regex)
}

/// Returns the occurrence of a search string in a string.
///
/// # Arguments
///
/// * `text` - the text
/// * `search` - the string to search
///
/// # Returns
///
/// The number of non-overlapping occurrences of the searched string in the text.
pub fn count_occurrence(text: &str, search: &str) -> usize {
    if search.is_empty() {
        // Java's loop never terminates for an empty search; Rust returns 0 instead.
        return 0;
    }
    text.matches(search).count()
}

/// Replaces a string in a text starting from its second occurrence.
///
/// The first occurrence is kept, all following occurrences are replaced (as plain text, not as a
/// regular expression).
///
/// # Arguments
///
/// * `text` - the text
/// * `search` - the string to search
/// * `replacement` - the replacement for the string
///
/// # Returns
///
/// The string with the replacement.
pub fn replace_from_second_occurrence(text: &str, search: &str, replacement: &str) -> String {
    if search.is_empty() {
        return text.to_string();
    }
    match text.find(search) {
        None => text.to_string(),
        Some(index) => {
            let start = index + search.len();
            format!(
                "{}{}",
                &text[..start],
                text[start..].replace(search, replacement)
            )
        }
    }
}

/// Pad the string with leading spaces.
///
/// # Arguments
///
/// * `level` - level (2 blanks per level)
pub fn pad_string(level: usize) -> String {
    pad_string_blanks(level, 2)
}

/// Pad the string with leading spaces.
///
/// # Arguments
///
/// * `level` - level
/// * `blanks` - number of blanks per level
pub fn pad_string_blanks(level: usize, blanks: usize) -> String {
    " ".repeat(level * blanks)
}

/// Fills the string with repeating chars.
///
/// # Arguments
///
/// * `ch` - the char
/// * `count` - number of chars
pub fn fill_chars(ch: char, count: usize) -> String {
    std::iter::repeat(ch).take(count).collect()
}

/// Is the given string only numbers.
pub fn is_digit(s: &str) -> bool {
    s.chars().all(|c| c.is_numeric())
}

/// Converts the bytes to (lower case) hex decimal.
pub fn bytes_to_hex(hash: &[u8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// Normalizes the whitespace by removing any excess whitespace so there is only at most a single
/// space between the words (any other whitespace such as tabs and new lines is also turned into a
/// space).
///
/// # Examples
///
/// ```
/// use camel_util::string_helper::normalize_whitespace;
/// assert_eq!("Hello Big World", normalize_whitespace("  Hello \t Big\nWorld "));
/// ```
pub fn normalize_whitespace(text: &str) -> String {
    let mut sb = String::with_capacity(text.len());
    let mut whitespace = false;
    for ch in java_trim(text).chars() {
        if is_java_whitespace(ch) {
            whitespace = true;
        } else {
            if whitespace {
                sb.push(' ');
                whitespace = false;
            }
            sb.push(ch);
        }
    }
    sb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case_to_dash() {
        assert_eq!("", camel_case_to_dash(""));
        let cases = [
            ("hello-world", "HelloWorld"),
            ("hello-big-world", "HelloBigWorld"),
            ("hello-big-world", "Hello-bigWorld"),
            ("my-id", "MyId"),
            ("my-id", "MyID"),
            ("my-url", "MyUrl"),
            ("my-url", "MyURL"),
            ("my-big-id", "MyBigId"),
            ("my-big-id", "MyBigID"),
            ("my-big-url", "MyBigUrl"),
            ("my-big-url", "MyBigURL"),
            ("my-big-id-again", "MyBigIdAgain"),
            ("my-big-id-again", "MyBigIDAgain"),
            ("my-big-url-again", "MyBigUrlAgain"),
            ("my-big-url-again", "MyBigURLAgain"),
            ("use-mdc-logging", "UseMDCLogging"),
            ("mdc-logging-keys-pattern", "MDCLoggingKeysPattern"),
            (
                "available-phone-number-country",
                "AVAILABLE_PHONE_NUMBER_COUNTRY",
            ),
            (
                "available-phone-number-country",
                "AVAILABLE-PHONE_NUMBER-COUNTRY",
            ),
            (
                "available-phone-number-country",
                "Available-Phone-Number-Country",
            ),
            (
                "available-phone-number-country",
                "Available_Phone_Number_Country",
            ),
            (
                "available-phone-number-country",
                "available_phone_number_country",
            ),
            (
                "available-phone-number-country",
                "availablePhoneNumberCountry",
            ),
            (
                "available-phone-number-country",
                "AvailablePhoneNumberCountry",
            ),
            ("enable-cors", "enableCORS"),
        ];
        for (expected, input) in cases {
            assert_eq!(expected, camel_case_to_dash(input), "{input}");
            assert_eq!(
                expected.replace('-', "."),
                camel_case_to_dot(input),
                "{input}"
            );
        }
        assert_eq!("", camel_case_to_dot(""));
    }

    #[test]
    fn test_dash_to_camel_case() {
        assert_eq!("enableCors", dash_to_camel_case("enable-cors"));
        assert_eq!("", dash_to_camel_case(""));
        let cases = [
            ("helloWorld", "hello-world"),
            ("helloBigWorld", "hello-big-world"),
            ("myId", "my-id"),
            ("myUrl", "my-url"),
            ("myBigId", "my-big-id"),
            ("myBigUrl", "my-big-url"),
            ("myBigIdAgain", "my-big-id-again"),
            ("myBigUrlAgain", "my-big-url-again"),
            ("useMdcLogging", "use-mdc-logging"),
            ("mdcLoggingKeysPattern", "mdc-logging-keys-pattern"),
            (
                "availablePhoneNumberCountry",
                "available-phone-number-country",
            ),
            (
                "available_phoneNumberCountry",
                "available_phone-number-country",
            ),
            (
                "available_phone_number_country",
                "available_phone_number_country",
            ),
        ];
        for (expected, input) in cases {
            assert_eq!(expected, dash_to_camel_case(input));
        }
    }

    #[test]
    fn test_dash_to_camel_case_skip_quoted_or_keyed() {
        let line = "camel.component.rabbitmq.args[queue.x-queue-type]";
        assert_eq!(
            "camel.component.rabbitmq.args[queue.xQueueType]",
            dash_to_camel_case(line)
        );
        assert_eq!(line, dash_to_camel_case_skip(line, true));
    }

    #[test]
    fn test_dash_to_camel_case_nested() {
        assert_eq!("", dash_to_camel_case(""));
        assert_eq!("a", dash_to_camel_case("a"));
        assert_eq!("aB", dash_to_camel_case("a-b"));
        assert_eq!("aBbCcDd", dash_to_camel_case("a-bb-cc-dd"));
        assert_eq!("a", dash_to_camel_case("a-"));
        assert_eq!("a", dash_to_camel_case("a----"));
        assert_eq!("aB", dash_to_camel_case("a--b"));
    }

    #[test]
    fn test_split_words() {
        assert_eq!(
            split_words("apiName/methodName"),
            vec!["apiName", "methodName"]
        );
        assert_eq!(split_words("hello"), vec!["hello"]);
    }

    #[test]
    fn test_replace_first() {
        assert_eq!(
            "jms:queue:bar",
            replace_first("jms:queue:bar", "foo", "bar")
        );
        assert_eq!(
            "jms:queue:bar",
            replace_first("jms:queue:foo", "foo", "bar")
        );
        assert_eq!(
            "jms:queue:bar?blah=123",
            replace_first("jms:queue:foo?blah=123", "foo", "bar")
        );
        assert_eq!(
            "jms:queue:bar?blah=foo",
            replace_first("jms:queue:foo?blah=foo", "foo", "bar")
        );
    }

    #[test]
    fn test_remove_leading_and_ending_quotes() {
        assert_eq!("abc", remove_leading_and_ending_quotes("'abc'"));
        assert_eq!("abc", remove_leading_and_ending_quotes("\"abc\""));
        assert_eq!("a'b'c", remove_leading_and_ending_quotes("a'b'c"));
        assert_eq!("'b'c", remove_leading_and_ending_quotes("'b'c"));
        assert_eq!("", remove_leading_and_ending_quotes("''"));
        assert_eq!("'", remove_leading_and_ending_quotes("'"));
    }

    #[test]
    fn test_remove_leading_and_ending_quotes_with_spaces() {
        assert_eq!(" ", remove_leading_and_ending_quotes(" "));
        assert_eq!(
            "Hello World",
            remove_leading_and_ending_quotes("Hello World")
        );
        assert_eq!(
            "Hello World",
            remove_leading_and_ending_quotes("'Hello World'")
        );
        assert_eq!(
            "Hello World",
            remove_leading_and_ending_quotes("\"Hello World\"")
        );
        assert_eq!(
            "Hello 'Camel'",
            remove_leading_and_ending_quotes("Hello 'Camel'")
        );
    }

    fn hundred() -> String {
        (0..100).map(|i| format!("{i},")).collect()
    }

    #[test]
    fn test_split_on_character_as_list() {
        assert_eq!(split_on_character_as_list("foo", ',', 1), vec!["foo"]);
        assert_eq!(
            split_on_character_as_list("foo,bar", ',', 2),
            vec!["foo", "bar"]
        );
        assert_eq!(
            split_on_character_as_list("foo,bar,", ',', 3),
            vec!["foo", "bar"]
        );
        assert_eq!(
            split_on_character_as_list(",foo,bar", ',', 3),
            vec!["foo", "bar"]
        );
        assert_eq!(
            split_on_character_as_list(",foo,bar,", ',', 4),
            vec!["foo", "bar"]
        );
        assert!(split_on_character_as_list(",", ',', 2).is_empty());

        let value = hundred();
        let count = count_char(&value, ',') + 1;
        let list = split_on_character_as_list(&value, ',', count);
        assert_eq!(100, list.len());
        assert_eq!("0", list[0]);
        assert_eq!("50", list[50]);
        assert_eq!("99", list[99]);
    }

    #[test]
    fn test_split_on_character_as_iterator() {
        let mut it = split_on_character_as_iterator("foo", ',', 1);
        assert_eq!(Some("foo".to_string()), it.next());
        assert!(it.next().is_none());

        assert!(split_on_character_as_iterator(",", ',', 2).next().is_none());

        for (input, n) in [
            ("foo,bar", 2),
            ("foo,bar,", 3),
            (",foo,bar", 3),
            (",foo,bar,", 4),
        ] {
            let mut it = split_on_character_as_iterator(input, ',', n);
            assert_eq!(Some("foo".to_string()), it.next());
            assert_eq!(Some("bar".to_string()), it.next());
            assert!(it.next().is_none());
        }

        let value = hundred();
        let count = count_char(&value, ',') + 1;
        let mut it = split_on_character_as_iterator(&value, ',', count);
        for i in 0..100 {
            assert_eq!(Some(i.to_string()), it.next());
        }
        assert!(it.next().is_none());
    }

    #[test]
    fn test_pad() {
        assert_eq!("  ", pad_string(1));
        assert_eq!("    ", pad_string(2));
        assert_eq!("      ", pad_string(3));
        assert_eq!("   ", pad_string_blanks(3, 1));
        assert_eq!(" ", pad_string_blanks(1, 1));
        assert_eq!("", pad_string(0));
        assert_eq!("", pad_string_blanks(0, 2));
    }

    #[test]
    fn test_fill_chars() {
        assert_eq!("", fill_chars('-', 0));
        assert_eq!("==", fill_chars('=', 2));
        assert_eq!("----", fill_chars('-', 4));
        assert_eq!("..........", fill_chars('.', 10));
    }

    #[test]
    fn test_sanitize() {
        let out = sanitize("hello");
        assert!(!out.contains(':') && !out.contains('.'));
        let out = sanitize("c:\\helloworld");
        assert!(!out.contains(':') && !out.contains('.'));
    }

    #[test]
    fn test_simple_crlf() {
        assert_eq!("hello", remove_crlf("hello"));
        assert_eq!("hello", remove_crlf("hello\r\n"));
        assert_eq!("hello", remove_crlf("\r\nhe\r\nllo\n"));
        assert_eq!("hello", remove_crlf("hello\n"));
    }

    #[test]
    fn test_count_char() {
        assert_eq!(0, count_char("Hello World", 'x'));
        assert_eq!(1, count_char("Hello World", 'e'));
        assert_eq!(3, count_char("Hello World", 'l'));
        assert_eq!(1, count_char("Hello World", ' '));
        assert_eq!(0, count_char("", ' '));
        assert_eq!(1, count_char_until("Hello World", 'l', 3));
    }

    #[test]
    fn test_remove_quotes() {
        assert_eq!("Hello World", remove_quotes("Hello World"));
        assert_eq!("", remove_quotes(""));
        assert_eq!(" ", remove_quotes(" "));
        for s in [
            "'foo'", "'foo", "foo'", "\"foo\"", "\"foo", "foo\"", "'foo\"",
        ] {
            assert_eq!("foo", remove_quotes(s));
        }
    }

    #[test]
    fn test_has_upper() {
        assert!(!has_upper_case(""));
        assert!(!has_upper_case(" "));
        assert!(!has_upper_case("com.foo"));
        assert!(!has_upper_case("com.foo.123"));
        assert!(has_upper_case("com.foo.MyClass"));
        assert!(has_upper_case("com.foo.My"));
        assert!(has_upper_case("com.foo.subA"));
    }

    #[test]
    fn test_is_class_name() {
        assert!(!is_class_name(""));
        assert!(!is_class_name(" "));
        assert!(!is_class_name("com.foo"));
        assert!(!is_class_name("com.foo.123"));
        assert!(is_class_name("com.foo.MyClass"));
        assert!(is_class_name("com.foo.My"));
        assert!(!is_class_name("com.foo.subA"));
    }

    #[test]
    fn test_has_start_token() {
        assert!(!has_start_token("", None));
        assert!(!has_start_token("", Some("simple")));
        assert!(!has_start_token("Hello World", None));
        assert!(!has_start_token("Hello World", Some("simple")));
        assert!(!has_start_token("${body}", None));
        assert!(has_start_token("${body}", Some("simple")));
        assert!(has_start_token("$simple{body}", Some("simple")));
        assert!(!has_start_token("${body}", Some("foo")));
        assert!(has_start_token("$foo{body}", Some("foo")));
    }

    #[test]
    fn test_is_quoted() {
        for s in [
            "",
            " ",
            "abc",
            "abc'",
            "\"abc'",
            "abc\"",
            "'abc\"",
            "abc'def'",
            "abc'def'ghi",
            "'def'ghi",
        ] {
            assert!(!is_quoted(s), "{s}");
        }
        assert!(is_quoted("'abc'"));
        assert!(is_quoted("\"abc\""));
    }

    #[test]
    fn test_remove_initial_characters() {
        assert_eq!("foo", remove_starting_characters("foo", '/'));
        assert_eq!("foo", remove_starting_characters("/foo", '/'));
        assert_eq!("foo", remove_starting_characters("//foo", '/'));
    }

    #[test]
    fn test_before() {
        assert_eq!(Some("Hello ".into()), before("Hello World", "World"));
        assert_eq!(Some("Hello ".into()), before("Hello World Again", "World"));
        assert_eq!(None, before("Hello Again", "Foo"));
        assert!(before_map("mykey:ignore", ":", |s| s == "mykey").unwrap_or(false));
        assert!(!before_map("ignore:ignore", ":", |s| s == "mykey").unwrap_or(false));
        assert_eq!("", before_or("Hello World", "Test", ""));
        assert_eq!(Some("a:b".into()), before_last("a:b:c", ":"));
        assert_eq!("", before_last_or("a:b:c", "_", ""));
        assert_eq!("a", before_char_or("a:b", ':', "x"));
    }

    #[test]
    fn test_after() {
        assert_eq!(Some(" World".into()), after("Hello World", "Hello"));
        assert_eq!(
            Some(" World Again".into()),
            after("Hello World Again", "Hello")
        );
        assert_eq!(None, after("Hello Again", "Foo"));
        assert!(after_map("ignore:mykey", ":", |s| s == "mykey").unwrap_or(false));
        assert!(!after_map("ignore:ignore", ":", |s| s == "mykey").unwrap_or(false));
        assert_eq!("", after_or("Hello World", "Test", ""));
        assert_eq!(Some("c".into()), after_last("a:b:c", ":"));
        assert_eq!("", after_last_or("a:b:c", "_", ""));
    }

    #[test]
    fn test_between() {
        assert_eq!(
            Some("foo bar".into()),
            between("Hello 'foo bar' how are you", "'", "'")
        );
        assert_eq!(
            Some("foo bar".into()),
            between("Hello ${foo bar} how are you", "${", "}")
        );
        assert_eq!(None, between("Hello ${foo bar} how are you", "'", "'"));
        assert!(
            between_map("begin:mykey:end", "begin:", ":end", |s| s == "mykey").unwrap_or(false)
        );
        assert!(
            !between_map("begin:ignore:end", "begin:", ":end", |s| s == "mykey").unwrap_or(false)
        );
        assert_eq!("ell", between_index("Hello", 1, 1));
        assert_eq!("", between_index("Hello", 10, 0));
    }

    #[test]
    fn test_between_outer_pair() {
        assert_eq!(
            Some("bar(baz)123".into()),
            between_outer_pair("foo(bar(baz)123)", '(', ')')
        );
        assert_eq!(None, between_outer_pair("foo(bar(baz)123))", '(', ')'));
        assert_eq!(None, between_outer_pair("foo(bar(baz123", '(', ')'));
        assert_eq!(None, between_outer_pair("foo)bar)baz123", '(', ')'));
        assert_eq!(
            Some("bar".into()),
            between_outer_pair("foo(bar)baz123", '(', ')')
        );
        assert_eq!(
            Some("'bar', 'baz()123', 123".into()),
            between_outer_pair("foo('bar', 'baz()123', 123)", '(', ')')
        );
        assert!(
            between_outer_pair_map("foo(bar)baz123", '(', ')', |s| s == "bar").unwrap_or(false)
        );
        assert!(
            !between_outer_pair_map("foo[bar)baz123", '(', ')', |s| s == "bar").unwrap_or(false)
        );
    }

    #[test]
    fn test_is_java_identifier() {
        assert!(is_java_identifier("foo"));
        assert!(!is_java_identifier("foo.bar"));
        assert!(!is_java_identifier(""));
    }

    #[test]
    fn test_normalize_class_name() {
        assert_eq!("my.package-info", normalize_class_name("my.package-info"));
        assert_eq!("Integer[]", normalize_class_name("Integer[] \r"));
        assert_eq!("Hello_World", normalize_class_name("Hello_World"));
        assert_eq!("", normalize_class_name("////"));
    }

    #[test]
    fn test_readable_class_name() {
        assert_eq!("byte[]", readable_class_name("[B"));
        assert_eq!("int[][]", readable_class_name("[[I"));
        assert_eq!(
            "java.util.HashMap$Node[]",
            readable_class_name("[Ljava.util.HashMap$Node;")
        );
        assert_eq!("java.lang.String", readable_class_name("java.lang.String"));
    }

    #[test]
    fn test_changed_lines() {
        assert!(changed_lines(
            Some("Hello\nWorld\nHow are you"),
            "Hello\nWorld\nHow are you"
        )
        .is_empty());
        assert_eq!(
            vec![2],
            changed_lines(
                Some("Hello\nWorld\nHow are you"),
                "Hello\nWorld\nHow are you today"
            )
        );
        assert_eq!(
            vec![1, 2],
            changed_lines(
                Some("Hello\nWorld\nHow are you"),
                "Hello\nCamel\nHow are you today"
            )
        );
        assert_eq!(
            vec![2, 3],
            changed_lines(
                Some("Hello\nWorld\nHow are you"),
                "Hello\nWorld\nHow are you today\nand tomorrow"
            )
        );
        assert!(changed_lines(None, "x").is_empty());
    }

    #[test]
    fn test_trim_to_null() {
        assert_eq!(Some("abc".into()), trim_to_null("abc"));
        assert_eq!(Some("abc".into()), trim_to_null(" abc"));
        assert_eq!(Some("abc".into()), trim_to_null(" abc "));
        for s in [" ", "\t", " \t ", ""] {
            assert_eq!(None, trim_to_null(s));
        }
    }

    #[test]
    fn test_human_readable_bytes() {
        assert_eq!("0 B", human_readable_bytes(0));
        assert_eq!("32 B", human_readable_bytes(32));
        assert_eq!("1.0 KB", human_readable_bytes(1024));
        assert_eq!("1.7 KB", human_readable_bytes(1730));
        assert_eq!("108.0 KB", human_readable_bytes(110592));
        assert_eq!("6.8 MB", human_readable_bytes(7077888));
        assert_eq!("432.0 MB", human_readable_bytes(452984832));
        assert_eq!("27.0 GB", human_readable_bytes(28991029248));
        assert_eq!("1.7 TB", human_readable_bytes(1855425871872));
        assert_eq!("1.3 KB", human_readable_bytes(1280));
    }

    #[test]
    fn test_capitalize() {
        assert_eq!("", dash_to_camel_case(""));
        assert_eq!("hello", dash_to_camel_case("hello"));
        assert_eq!("helloGreat", dash_to_camel_case("helloGreat"));
        assert_eq!("helloGreat", dash_to_camel_case("hello-great"));
        assert_eq!("helloGreatWorld", dash_to_camel_case("hello-great-world"));
        assert_eq!(
            "HelloGreatWorld",
            capitalize_dash("hello-great-world", true)
        );
        assert_eq!("Hello", capitalize("hello"));
        assert_eq!("hello", decapitalize("Hello"));
    }

    #[test]
    fn test_capitalize_all() {
        assert_eq!(
            "Hello World How Are You",
            capitalize_all("hello world how are you")
        );
        assert_eq!("", capitalize_all(""));
        assert_eq!("", capitalize(""));
        assert_eq!(" Hello World", capitalize_all(" hello world"));
        assert_eq!("A B", capitalize_all("a b"));
        assert_eq!("Hello\tWorld\nAgain", capitalize_all("hello\tworld\nagain"));
    }

    #[test]
    fn test_starts_with_ignore_case() {
        assert!(!starts_with_ignore_case("HelloWorld", "bar"));
        assert!(starts_with_ignore_case("HelloWorld", "Hello"));
        assert!(starts_with_ignore_case("HelloWorld", "hello"));
        assert!(!starts_with_ignore_case("HelloWorld", "Helo"));
        assert!(starts_with_ignore_case("HelloWorld", "HelloWorld"));
        assert!(starts_with_ignore_case("HelloWorld", "helloWORLD"));
        assert!(starts_with_ignore_case("HelloWorld", "HELLO"));
        assert!(starts_with_ignore_case("helloworld", "helloWORLD"));
        assert!(starts_with_ignore_case("HELLOWORLD", "HELLO"));
    }

    #[test]
    fn test_split_as_stream() {
        let items = split_as_stream("a,b,c", ",");
        assert!(items.contains(&"a".to_string()));
        assert!(items.contains(&"b".to_string()));
        assert!(items.contains(&"c".to_string()));
    }

    #[test]
    fn test_split_on_character() {
        let s = |x: &str| Some(x.to_string());
        assert_eq!(split_on_character("foo", "'", 1), vec![s("foo")]);
        assert_eq!(
            split_on_character("foo,bar", ",", 2),
            vec![s("foo"), s("bar")]
        );
        let list = split_on_character("foo,bar,", ",", 3);
        assert_eq!(3, list.len());
        assert_eq!(s("foo"), list[0]);
        assert_eq!(s("bar"), list[1]);
        let list = split_on_character(",foo,bar", ",", 3);
        assert_eq!(s("foo"), list[1]);
        assert_eq!(s("bar"), list[2]);
        let list = split_on_character(",foo,bar,", ",", 4);
        assert_eq!(4, list.len());
        assert_eq!(s("foo"), list[1]);
        assert_eq!(s("bar"), list[2]);

        let value = hundred();
        let count = count_char(&value, ',') + 1;
        let list = split_on_character(&value, ",", count);
        assert_eq!(101, list.len());
        assert_eq!(s("0"), list[0]);
        assert_eq!(s("50"), list[50]);
        assert_eq!(s("99"), list[99]);
    }

    #[test]
    fn test_limit_length() {
        let s = "ABCDEFGHIJKLMNOPQRST";
        assert_eq!(s, limit_length(s, 30));
        assert_eq!(s, limit_length(s, 20));
        assert_eq!(&s[..10], limit_length(s, 10));
        assert_eq!(s, limit_length_with_suffix(s, 30, Some(" (cut)")));
        assert_eq!(s, limit_length_with_suffix(s, 20, Some(" (cut)")));
        assert_eq!(
            format!("{} (cut)", &s[..10]),
            limit_length_with_suffix(s, 10, Some(" (cut)"))
        );
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!("", normalize_whitespace(""));
        assert_eq!("", normalize_whitespace(" "));
        assert_eq!("", normalize_whitespace("  "));
        assert_eq!("", normalize_whitespace("                        "));
        assert_eq!("A", normalize_whitespace("            A            "));
        assert_eq!(
            "Hello World How Are You",
            normalize_whitespace("Hello World How Are You")
        );
        assert_eq!("Hello World", normalize_whitespace(" Hello World "));
        assert_eq!("Hello World", normalize_whitespace("   Hello World   "));
        assert_eq!("Hello World", normalize_whitespace("   Hello   World   "));
        assert_eq!(
            "Hello Big World",
            normalize_whitespace("   Hello   Big   World   ")
        );
        assert_eq!(
            "Hello Big World",
            normalize_whitespace("Hello   Big   World")
        );
        assert_eq!(
            "Hello Big World",
            normalize_whitespace(" Hello   Big   World")
        );
        assert_eq!(
            "Hello Big World",
            normalize_whitespace(" Hello   Big   World ")
        );
    }

    #[test]
    fn test_misc() {
        assert_eq!("HELLO_WORLD", as_enum_constant_value("helloWorld"));
        assert_eq!(2, count_occurrence("abcabc", "bc"));
        assert_eq!("a.b-c-", replace_from_second_occurrence("a.b.c.", ".", "-"));
        assert!(is_digit("123"));
        assert!(!is_digit("12a"));
        assert_eq!("00ff10", bytes_to_hex(&[0, 255, 16]));
        assert_eq!(
            "&lt;a&gt; &amp; &quot;b&quot; &apos;c&apos;",
            xml_encode("<a> & \"b\" 'c'")
        );
        assert_eq!(
            "<a> & \"b\" 'c'",
            xml_decode("&lt;a&gt; &amp; &quot;b&quot; &apos;c&apos;")
        );
        assert_eq!("{ \"a\": \"b\" }", to_json("a", "b", true));
        assert_eq!("\"a\": \"b\"", to_json("a", "b", false));
        assert!(contains_ignore_case("Hello World", "WORLD"));
        assert!(contains_ignore_case("Hello", ""));
        assert!(!contains_ignore_case("Hello", "x"));
        assert!(matches(None, "x"));
        assert!(matches(Some("*"), "x"));
        assert!(matches(Some("foo*"), "foobar"));
        assert!(matches(Some("fo+bar"), "fooobar"));
        assert!(!matches(Some("fo+bar"), "fobaz"));
        assert!(not_empty(Some(""), "name").is_err());
        assert_eq!(
            "name must be specified and not empty",
            not_empty(None, "name").unwrap_err()
        );
        assert_eq!(Ok("x"), not_empty(Some("x"), "name"));
        assert_eq!(
            "name must be specified and not empty on: foo",
            not_empty_on(None, "name", "foo").unwrap_err()
        );
    }
}
