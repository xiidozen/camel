//! The default header filter strategy (Java
//! `org.apache.camel.support.DefaultHeaderFilterStrategy`).

use camel_api::exchange::Exchange;
use camel_api::spi::header_filter::HeaderFilterStrategy;
use camel_util::value::Value;
use regex::Regex;
use std::collections::HashSet;

/// The default filter prefixes: header names starting with `Camel` or `camel`.
pub const CAMEL_FILTER_STARTS_WITH: [&str; 2] = ["Camel", "camel"];

/// The direction of the filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// From an external message to a Camel message.
    In,
    /// From a Camel message to an external message.
    Out,
}

/// The default header filtering strategy: users configure which headers are allowed or denied
/// by name (a set), name prefixes or a regular expression, per direction.
///
/// The "in" direction is copying headers from an external message to a Camel message, the
/// "out" direction is copying headers from a Camel message to an external message (such as a
/// JMS or HTTP message).
///
/// By default headers starting with `Camel` / `camel` are filtered in both directions (also
/// in any case, as names are lower cased before checking), and headers with a `Null` value are
/// skipped.
///
/// # Examples
///
/// ```
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
/// use camel_api::spi::header_filter::HeaderFilterStrategy;
/// use camel_support::header_filter_strategy::DefaultHeaderFilterStrategy;
/// use camel_util::value::Value;
///
/// let s = DefaultHeaderFilterStrategy::new();
/// let ex = Exchange::new(StandaloneContext::shared());
/// assert!(s.apply_filter_to_external_headers("CAMELFileName", &Value::from("x"), &ex));
/// assert!(!s.apply_filter_to_external_headers("Content-Type", &Value::from("x"), &ex));
/// ```
#[derive(Debug, Clone)]
pub struct DefaultHeaderFilterStrategy {
    in_filter: Option<HashSet<String>>,
    in_filter_pattern: Option<Regex>,
    in_filter_starts_with: Vec<String>,
    out_filter: Option<HashSet<String>>,
    out_filter_pattern: Option<Regex>,
    out_filter_starts_with: Vec<String>,
    lower_case: bool,
    allow_null_values: bool,
    case_insensitive: bool,
    filter_on_match: bool,
}

impl Default for DefaultHeaderFilterStrategy {
    fn default() -> Self {
        Self::new()
    }
}

fn split_set(value: &str) -> HashSet<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

impl DefaultHeaderFilterStrategy {
    /// Creates the strategy with the default settings.
    pub fn new() -> Self {
        let defaults: Vec<String> = CAMEL_FILTER_STARTS_WITH
            .iter()
            .map(|s| s.to_string())
            .collect();
        DefaultHeaderFilterStrategy {
            in_filter: None,
            in_filter_pattern: None,
            in_filter_starts_with: defaults.clone(),
            out_filter: None,
            out_filter_pattern: None,
            out_filter_starts_with: defaults,
            lower_case: true,
            allow_null_values: false,
            case_insensitive: true,
            filter_on_match: true,
        }
    }

    /// Sets the in direction filter set (names, separated by comma).
    pub fn set_in_filter(&mut self, value: &str) {
        self.in_filter = Some(split_set(value));
    }

    /// Sets the in direction filter prefixes.
    pub fn set_in_filter_starts_with(&mut self, prefixes: &[&str]) {
        self.in_filter_starts_with = prefixes.iter().map(|s| s.to_string()).collect();
    }

    /// Sets the in direction filter regular expression (the whole name must match).
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is not a valid regular expression.
    pub fn set_in_filter_pattern(&mut self, pattern: &str) -> Result<(), regex::Error> {
        self.in_filter_pattern = Some(Regex::new(&format!("^(?:{pattern})$"))?);
        Ok(())
    }

    /// Sets the out direction filter set (names, separated by comma).
    pub fn set_out_filter(&mut self, value: &str) {
        self.out_filter = Some(split_set(value));
    }

    /// Sets the out direction filter prefixes.
    pub fn set_out_filter_starts_with(&mut self, prefixes: &[&str]) {
        self.out_filter_starts_with = prefixes.iter().map(|s| s.to_string()).collect();
    }

    /// Sets the out direction filter regular expression (the whole name must match).
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is not a valid regular expression.
    pub fn set_out_filter_pattern(&mut self, pattern: &str) -> Result<(), regex::Error> {
        self.out_filter_pattern = Some(Regex::new(&format!("^(?:{pattern})$"))?);
        Ok(())
    }

    /// Whether header names are converted to lower case before checking the filter set and
    /// prefixes (default `true`).
    pub fn set_lower_case(&mut self, value: bool) {
        self.lower_case = value;
    }

    /// Whether headers with a `Null` value are kept (default `false`: they are skipped).
    pub fn set_allow_null_values(&mut self, value: bool) {
        self.allow_null_values = value;
    }

    /// Whether names are compared with the filter set ignoring case (default `true`).
    pub fn set_case_insensitive(&mut self, value: bool) {
        self.case_insensitive = value;
    }

    /// What a match means: `true` (default) filters the header out; `false` keeps only the
    /// matched headers and filters out everything else.
    pub fn set_filter_on_match(&mut self, value: bool) {
        self.filter_on_match = value;
    }

    fn do_filtering(&self, direction: Direction, header_name: &str, header_value: &Value) -> bool {
        if header_value.is_null() && !self.allow_null_values {
            return true;
        }
        let (pattern, filter, starts_with) = match direction {
            Direction::Out => (
                &self.out_filter_pattern,
                &self.out_filter,
                &self.out_filter_starts_with,
            ),
            Direction::In => (
                &self.in_filter_pattern,
                &self.in_filter,
                &self.in_filter_starts_with,
            ),
        };
        let lower = header_name.to_lowercase();
        if starts_with
            .iter()
            .any(|s| header_name.starts_with(s.as_str()))
            || (self.lower_case && starts_with.iter().any(|s| lower.starts_with(s.as_str())))
        {
            return self.filter_on_match;
        }
        if let Some(p) = pattern {
            if p.is_match(header_name) {
                return self.filter_on_match;
            }
        }
        if let Some(f) = filter {
            let matched = if self.case_insensitive {
                f.iter().any(|s| s.eq_ignore_ascii_case(header_name))
            } else if self.lower_case {
                f.contains(&lower)
            } else {
                f.contains(header_name)
            };
            if matched {
                return self.filter_on_match;
            }
        }
        !self.filter_on_match
    }
}

impl HeaderFilterStrategy for DefaultHeaderFilterStrategy {
    fn apply_filter_to_camel_headers(
        &self,
        header_name: &str,
        header_value: &Value,
        _exchange: &Exchange,
    ) -> bool {
        self.do_filtering(Direction::Out, header_name, header_value)
    }

    fn apply_filter_to_external_headers(
        &self,
        header_name: &str,
        header_value: &Value,
        _exchange: &Exchange,
    ) -> bool {
        self.do_filtering(Direction::In, header_name, header_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_api::context::StandaloneContext;

    fn ex() -> Exchange {
        Exchange::new(StandaloneContext::shared())
    }

    #[test]
    fn defaults() {
        let s = DefaultHeaderFilterStrategy::new();
        let e = ex();
        let v = Value::from("x");
        for h in ["CamelFileName", "camelFoo", "CAMELfoo", "cAmElX"] {
            assert!(s.apply_filter_to_external_headers(h, &v, &e), "{h}");
            assert!(s.apply_filter_to_camel_headers(h, &v, &e), "{h}");
        }
        assert!(!s.apply_filter_to_external_headers("Content-Type", &v, &e));
        // null values are skipped by default
        assert!(s.apply_filter_to_external_headers("Content-Type", &Value::Null, &e));
    }

    #[test]
    fn filter_set_pattern_and_keep_mode() {
        let mut s = DefaultHeaderFilterStrategy::new();
        s.set_in_filter("Secret, Token");
        s.set_out_filter_pattern("x-.*").unwrap();
        s.set_allow_null_values(true);
        let e = ex();
        let v = Value::from("x");
        assert!(s.apply_filter_to_external_headers("SECRET", &v, &e));
        assert!(!s.apply_filter_to_external_headers("other", &Value::Null, &e));
        assert!(s.apply_filter_to_camel_headers("x-trace", &v, &e));
        assert!(!s.apply_filter_to_camel_headers("y-trace", &v, &e));

        s.set_case_insensitive(false);
        assert!(!s.apply_filter_to_external_headers("SECRET", &v, &e));
        s.set_in_filter("secret");
        assert!(s.apply_filter_to_external_headers("SECRET", &v, &e));
        s.set_lower_case(false);
        assert!(!s.apply_filter_to_external_headers("SECRET", &v, &e));

        // keep only the matched headers
        let mut keep = DefaultHeaderFilterStrategy::new();
        keep.set_in_filter_starts_with(&[]);
        keep.set_in_filter("Content-Type");
        keep.set_filter_on_match(false);
        assert!(!keep.apply_filter_to_external_headers("content-type", &v, &e));
        assert!(keep.apply_filter_to_external_headers("X-Other", &v, &e));
        keep.set_in_filter_pattern("a+").unwrap();
        assert!(!keep.apply_filter_to_external_headers("aaa", &v, &e));
        keep.set_out_filter_starts_with(&["X"]);
        keep.set_out_filter("");
        assert!(!keep.apply_filter_to_camel_headers("Xy", &v, &e));
    }
}
