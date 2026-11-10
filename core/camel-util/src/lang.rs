//! Small helpers that reproduce the semantics of `java.lang` methods the original
//! Camel code relied on (so behaviour stays identical after the port).

use regex::Regex;

/// Java `String.trim()`: removes leading/trailing characters with code point `<= ' '`.
pub fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Java `String.split(regex)`: splits around matches of the regex, keeping leading empty
/// strings but discarding trailing empty strings. A zero-width match at the beginning never
/// produces a leading empty string.
pub fn java_split(s: &str, regex: &str) -> Vec<String> {
    let re = Regex::new(regex).expect("invalid regex");
    java_split_re(s, &re)
}

/// Same as [`java_split`] with a pre-compiled regex.
pub fn java_split_re(s: &str, re: &Regex) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut last = 0;
    for m in re.find_iter(s) {
        if m.start() == 0 && m.end() == 0 {
            continue;
        }
        if m.start() == m.end() && m.start() == s.len() {
            continue;
        }
        out.push(s[last..m.start()].to_string());
        last = m.end();
    }
    out.push(s[last..].to_string());
    while out.len() > 1 && out.last().map(|x| x.is_empty()).unwrap_or(false) {
        out.pop();
    }
    if out.len() == 1 && out[0].is_empty() && !s.is_empty() {
        // e.g. "," split on "," gives an empty array in Java
        return Vec::new();
    }
    out
}

/// Java `String.split(String.valueOf(ch))` for a plain literal separator.
pub fn java_split_literal(s: &str, sep: &str) -> Vec<String> {
    java_split_re(s, &Regex::new(&regex::escape(sep)).unwrap())
}

/// Java `Character.isJavaIdentifierStart` (approximation for Unicode scalar values).
pub fn is_java_identifier_start(c: char) -> bool {
    c.is_alphabetic() || c == '$' || c == '_' || is_currency_or_connector(c)
}

/// Java `Character.isJavaIdentifierPart` (approximation for Unicode scalar values).
pub fn is_java_identifier_part(c: char) -> bool {
    c.is_alphanumeric()
        || c == '$'
        || c == '_'
        || is_currency_or_connector(c)
        || is_identifier_ignorable(c)
}

fn is_currency_or_connector(c: char) -> bool {
    matches!(c, '\u{a2}'..='\u{a5}' | '\u{20a0}'..='\u{20cf}' | '\u{203f}' | '\u{2040}' | '\u{2054}' | '\u{fe33}' | '\u{fe34}' | '\u{fe4d}'..='\u{fe4f}' | '\u{ff3f}')
}

fn is_identifier_ignorable(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{e}'..='\u{1b}' | '\u{7f}'..='\u{9f}')
}

/// Java `Character.isWhitespace`.
pub fn is_java_whitespace(c: char) -> bool {
    if matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}') {
        return false;
    }
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// Upper-cases a single character the way Java's `Character.toUpperCase(char)` does for the
/// common case (falls back to the original char when the upper-case form is not a single char).
pub fn char_to_upper(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// Lower-case counterpart of [`char_to_upper`].
pub fn char_to_lower(c: char) -> char {
    let mut lo = c.to_lowercase();
    match (lo.next(), lo.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_semantics() {
        assert_eq!(java_split("a,b,,", ","), vec!["a", "b"]);
        assert_eq!(java_split(",a", ","), vec!["", "a"]);
        assert!(java_split(",", ",").is_empty());
        assert_eq!(java_split("", ","), vec![""]);
        assert_eq!(java_split("hello", "[\\W]+"), vec!["hello"]);
    }

    #[test]
    fn trim_semantics() {
        assert_eq!(java_trim("\t a \r\n"), "a");
        assert_eq!(java_trim("   "), "");
    }
}
