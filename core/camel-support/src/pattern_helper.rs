//! Pattern matching of names (Java `org.apache.camel.support.PatternHelper`), used to match
//! header names, route ids, endpoint uris and so on.

/// Matches the name with the pattern (case insensitive), in this order: exact match, `*`
/// matches everything, wildcard (the pattern ends with `*` and the name starts with the text
/// before it), regular expression.
///
/// # Examples
///
/// ```
/// use camel_support::pattern_helper::match_pattern;
/// assert!(match_pattern("CamelFileName", "camel*"));
/// assert!(match_pattern("axb", "a.b"));
/// ```
pub fn match_pattern(name: &str, pattern: &str) -> bool {
    camel_api::message::header_matches(pattern, name)
}

/// Matches the name with any of the patterns (see [`match_pattern`]).
pub fn match_patterns(name: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| match_pattern(name, p))
}

/// Matches the name with the pattern (case insensitive) using simple matching only (no regular
/// expressions): exact match, `*`, or wildcard prefix.
pub fn match_simple_pattern(name: &str, pattern: &str) -> bool {
    if name.eq_ignore_ascii_case(pattern) || pattern == "*" {
        return true;
    }
    match pattern.strip_suffix('*') {
        Some(prefix) => name.to_lowercase().starts_with(&prefix.to_lowercase()),
        None => false,
    }
}

/// Matches the name with any of the patterns using simple matching only.
pub fn match_simple_patterns(name: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| match_simple_pattern(name, p))
}

/// Matches the name with the regular expression (case insensitive, the whole name must match).
/// An invalid expression never matches.
pub fn match_regex(name: &str, pattern: &str) -> bool {
    regex::Regex::new(&format!("^(?i:{pattern})$"))
        .map(|re| re.is_match(name))
        .unwrap_or(false)
}

/// Whether the key matches any of the exclude patterns.
pub fn is_exclude_pattern_match(key: &str, exclude_patterns: &[&str]) -> bool {
    match_patterns(key, exclude_patterns)
}

/// The keys matching the pattern, except those matching an exclude pattern (used to collect
/// keys for removal).
pub fn matching_set<'a, I: IntoIterator<Item = &'a str>>(
    keys: I,
    pattern: &str,
    exclude_patterns: &[&str],
) -> Vec<String> {
    keys.into_iter()
        .filter(|k| match_pattern(k, pattern) && !is_exclude_pattern_match(k, exclude_patterns))
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_pattern_exact_match() {
        assert!(match_simple_pattern("crm", "crm"));
        assert!(match_simple_pattern("crm", "CRM"));
        assert!(match_simple_pattern("CRM", "crm"));
    }

    #[test]
    fn test_simple_pattern_no_match() {
        assert!(!match_simple_pattern("crm", "erp"));
    }

    #[test]
    fn test_simple_pattern_wildcard_all() {
        assert!(match_simple_pattern("crm", "*"));
        assert!(match_simple_pattern("anything", "*"));
    }

    #[test]
    fn test_simple_pattern_wildcard_prefix() {
        assert!(match_simple_pattern("crm-tools", "crm*"));
        assert!(match_simple_pattern("CRM-Tools", "crm*"));
        assert!(!match_simple_pattern("erp-tools", "crm*"));
    }

    #[test]
    fn test_simple_pattern_no_regex_fallback() {
        assert!(!match_simple_pattern("axb", "a.b"));
        assert!(match_pattern("axb", "a.b"));
    }

    #[test]
    fn test_simple_patterns_multiple() {
        let patterns = ["crm", "erp*"];
        assert!(match_simple_patterns("crm", &patterns));
        assert!(match_simple_patterns("erp-tools", &patterns));
        assert!(!match_simple_patterns("billing", &patterns));
    }

    #[test]
    fn test_regex_and_sets() {
        assert!(match_regex("FOO", "f.o"));
        assert!(!match_regex("foo", "("));
        assert!(is_exclude_pattern_match("CamelA", &["x", "Camel*"]));
        assert_eq!(
            vec!["a1".to_string()],
            matching_set(["a1", "a2", "b"], "a*", &["a2"])
        );
    }
}
