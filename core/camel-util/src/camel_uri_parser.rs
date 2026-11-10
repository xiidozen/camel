//! A Camel specific URI parser that parses endpoint URIs in a quasi syntax that Camel uses.
//!
//! Port of `org.apache.camel.util.CamelURIParser`. The `java.net.URI` class can be slow to
//! parse endpoint URIs, especially if the URIs are not 100% conforming, so Camel first tries
//! this much simpler and faster parser.

use crate::unsafe_uri_characters_encoder::is_safe_fast_parser;

/// The result of parsing an endpoint URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedUri {
    /// The URI is already in normalized Camel style (`scheme://path` without query), so it can
    /// be used as-is (the Java `URI_ALREADY_NORMALIZED` marker).
    AlreadyNormalized,
    /// The URI split in its parts.
    Parts {
        /// The scheme (component name), such as `file`.
        scheme: String,
        /// The context path (without leading `//`).
        path: String,
        /// The query part (without the `?`), or `None` if there is no or an empty query.
        query: Option<String>,
    },
}

/// Parses the URI (in a fast mode).
///
/// Returns [`ParsedUri::AlreadyNormalized`] if the URI is already normalized, `None` if the
/// URI could not be parsed by this simple parser (it must then be parsed by the complex
/// `java.net.URI` compatible parser), or the URI parts otherwise.
///
/// # Arguments
///
/// * `uri` - the uri
pub fn fast_parse_uri(uri: &str) -> Option<ParsedUri> {
    do_parse_uri(uri, true)
}

/// Parses the URI into its scheme, path and query parts.
///
/// Returns `None` if the URI could not be parsed by this simple parser (it must then be parsed
/// by the complex `java.net.URI` compatible parser).
///
/// # Examples
///
/// ```
/// use camel_util::camel_uri_parser::{parse_uri, ParsedUri};
/// assert_eq!(
///     Some(ParsedUri::Parts { scheme: "file".into(), path: "/absolute".into(), query: Some("recursive=true".into()) }),
///     parse_uri("file:///absolute?recursive=true")
/// );
/// assert_eq!(None, parse_uri("doesnotexists"));
/// ```
pub fn parse_uri(uri: &str) -> Option<ParsedUri> {
    do_parse_uri(uri, false)
}

fn do_parse_uri(uri: &str, fast_parse: bool) -> Option<ParsedUri> {
    let bytes = uri.as_bytes();
    let len = bytes.len();
    let mut scheme_end = 0;
    let mut path_start = 0;
    let mut path_end = 0;
    let mut query_start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        // must be an ascii char, and a safe char
        if b >= 128 || !is_safe_fast_parser(b as char) {
            return None;
        }
        if scheme_end == 0 {
            if b == b':' {
                scheme_end = i;
                path_start = i + 1;
            }
        } else if path_end == 0 && b == b'?' {
            path_end = i;
            query_start = i + 1;
        }
    }
    // invalid if there is no path anyway
    if path_start >= len || scheme_end == 0 {
        return None;
    }
    // optimized if there are no query and the uri is already in camel style
    if fast_parse
        && query_start == 0
        && path_start + 1 < len
        && bytes[path_start] == b'/'
        && bytes[path_start + 1] == b'/'
    {
        return Some(ParsedUri::AlreadyNormalized);
    }
    let scheme = uri[..scheme_end].to_string();
    // skip two leading slashes
    if bytes[path_start] == b'/' && path_start + 1 < len && bytes[path_start + 1] == b'/' {
        path_start += 2;
    }
    let path = if path_end != 0 {
        uri[path_start.min(path_end)..path_end].to_string()
    } else {
        uri[path_start..].to_string()
    };
    let query = if query_start != 0 && query_start < len {
        Some(uri[query_start..].to_string())
    } else {
        None
    };
    Some(ParsedUri::Parts {
        scheme,
        path,
        query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(uri: &str) -> (String, String, Option<String>) {
        match parse_uri(uri).expect(uri) {
            ParsedUri::Parts {
                scheme,
                path,
                query,
            } => (scheme, path, query),
            ParsedUri::AlreadyNormalized => panic!("normalized"),
        }
    }

    fn p(s: &str, path: &str, q: Option<&str>) -> (String, String, Option<String>) {
        (s.to_string(), path.to_string(), q.map(String::from))
    }

    #[test]
    fn test_parse_uri() {
        assert_eq!(
            p(
                "smtp",
                "localhost",
                Some("username=davsclaus&password=secret")
            ),
            parts("smtp://localhost?username=davsclaus&password=secret")
        );
    }

    #[test]
    fn test_parse_no_slash_uri() {
        assert_eq!(p("direct", "start", None), parts("direct:start"));
    }

    #[test]
    fn test_parse_uri_slash_and_query() {
        assert_eq!(
            p("file", "/absolute", Some("recursive=true")),
            parts("file:/absolute?recursive=true")
        );
        assert_eq!(
            p("file", "/absolute", Some("recursive=true")),
            parts("file:///absolute?recursive=true")
        );
        assert_eq!(
            p("file", "relative", Some("recursive=true")),
            parts("file://relative?recursive=true")
        );
        assert_eq!(
            p("file", "relative", Some("recursive=true")),
            parts("file:relative?recursive=true")
        );
    }

    #[test]
    fn test_parse_uri_slash() {
        assert_eq!(p("file", "/absolute", None), parts("file:/absolute"));
        assert_eq!(p("file", "/absolute", None), parts("file:///absolute"));
        assert_eq!(p("file", "relative", None), parts("file://relative"));
        assert_eq!(p("file", "relative", None), parts("file:relative"));
    }

    #[test]
    fn test_parse_invalid() {
        assert_eq!(None, parse_uri("doesnotexists"));
        assert_eq!(None, parse_uri("doesnotexists:"));
        assert_eq!(None, parse_uri("doesnotexists/foo"));
        assert_eq!(None, parse_uri("doesnotexists?"));
    }

    #[test]
    fn test_parse_no_path_but_slash() {
        assert_eq!(p("file", "/", None), parts("file:/"));
        assert_eq!(p("file", "/", None), parts("file:///"));
    }

    #[test]
    fn test_parse_empty_query() {
        assert_eq!(p("file", "relative", None), parts("file:relative"));
        assert_eq!(p("file", "relative", None), parts("file:relative?"));
    }

    #[test]
    fn test_fast_parse() {
        let f = |u| fast_parse_uri(u).unwrap();
        assert_eq!(
            ParsedUri::Parts {
                scheme: "file".into(),
                path: "relative".into(),
                query: None
            },
            f("file:relative")
        );
        assert_eq!(ParsedUri::AlreadyNormalized, f("file://relative"));
        assert_eq!(
            ParsedUri::Parts {
                scheme: "file".into(),
                path: "relative".into(),
                query: Some("delete=true".into())
            },
            f("file:relative?delete=true")
        );
        assert_eq!(
            ParsedUri::Parts {
                scheme: "file".into(),
                path: "relative".into(),
                query: Some("delete=true".into())
            },
            f("file://relative?delete=true")
        );
    }
}
