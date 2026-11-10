//! URI utilities. Port of `org.apache.camel.util.URISupport` (and the package-private
//! `URIScanner`).
//!
//! **Important:** this module is only intended for Camel internal, Camel components, and other
//! Camel features. If you need a general purpose URI/URL utility then do not use this module.
//! It is implemented in a certain way to work and support how Camel internally parses endpoint
//! URIs.
//!
//! URIs are parsed with [`JavaUri`], which reproduces `java.net.URI`, so endpoint URIs are
//! normalized exactly as in the Java version of Camel.

use crate::camel_uri_parser::{fast_parse_uri, ParsedUri};
use crate::lang::{java_split_literal, java_trim};
use crate::net::{url_decode, url_encode, JavaUri, UriSyntaxError};
use crate::pair::Pair;
use crate::sensitive_utils::{
    contains_sensitive, get_sensitive_pattern, mask_path_user_info, mask_user_info,
};
use crate::string_helper::{after, before, before_or, count_char, count_char_until};
use crate::unsafe_uri_characters_encoder::{encode, encode_http_uri, encode_raw};
use indexmap::IndexMap;
use regex::Regex;
use std::fmt;
use std::sync::{OnceLock, RwLock};

/// The prefix of a RAW token: `key=RAW(value)` tells Camel to use the value as-is.
pub const RAW_TOKEN_PREFIX: &str = "RAW";
/// The characters that can start a RAW token value (`RAW(` or `RAW{`).
pub const RAW_TOKEN_START: [char; 2] = ['(', '{'];
/// The characters that end a RAW token value, matching [`RAW_TOKEN_START`] by index.
pub const RAW_TOKEN_END: [char; 2] = [')', '}'];

// use xxxxxx as replacement as that works well with JMX also
const MASK: &str = "xxxxxx";

/// A query parameter value.
///
/// Java used `Map<String, Object>` where a value was a `String`, a `List<String>` (when the
/// same key occurs several times) or `null`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParamValue {
    /// No value (Java `null`), rendered as just the key in a query string.
    Null,
    /// A single value.
    Text(String),
    /// Multiple values for the same key.
    List(Vec<String>),
}

impl ParamValue {
    /// Returns the single text value, if this is a [`ParamValue::Text`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParamValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Whether this is [`ParamValue::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, ParamValue::Null)
    }
}

impl fmt::Display for ParamValue {
    /// Formats like Java's `toString()` (`null`, the text, or `[a, b]` for a list).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamValue::Null => f.write_str("null"),
            ParamValue::Text(s) => f.write_str(s),
            ParamValue::List(l) => write!(f, "[{}]", l.join(", ")),
        }
    }
}

impl From<&str> for ParamValue {
    fn from(s: &str) -> Self {
        ParamValue::Text(s.to_string())
    }
}

impl From<String> for ParamValue {
    fn from(s: String) -> Self {
        ParamValue::Text(s)
    }
}

impl From<Option<&str>> for ParamValue {
    fn from(s: Option<&str>) -> Self {
        s.map(ParamValue::from).unwrap_or(ParamValue::Null)
    }
}

impl From<Vec<String>> for ParamValue {
    fn from(l: Vec<String>) -> Self {
        ParamValue::List(l)
    }
}

impl PartialEq<&str> for ParamValue {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<ParamValue> for &str {
    fn eq(&self, other: &ParamValue) -> bool {
        other == self
    }
}

/// URI parameters in insertion order (the Java `LinkedHashMap<String, Object>`).
pub type Parameters = IndexMap<String, ParamValue>;

// ---------------------------------------------------------------------------------------
// sanitizing

fn extra_keywords() -> &'static RwLock<Vec<String>> {
    static KEYWORDS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
    KEYWORDS.get_or_init(|| RwLock::new(Vec::new()))
}

fn query_parameter_key() -> &'static Regex {
    // Match the key of a query parameter as first capture group (the value starts after =)
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new("[?&]([^?&=]*)=").unwrap())
}

fn sensitive_keywords() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!("(?i){}", get_sensitive_pattern())).unwrap())
}

/// Adds custom keywords for sanitizing sensitive information (such as passwords) from URIs.
///
/// Notice that when a key has been added it cannot be removed. Keys are matched in lower case
/// and without dashes, the same way as the parameter names.
///
/// # Arguments
///
/// * `keywords` - keywords separated by comma (`None` is ignored)
pub fn add_sanitize_keywords(keywords: Option<&str>) {
    let Some(keywords) = keywords else {
        return;
    };
    let mut extra = extra_keywords().write().unwrap();
    for key in java_split_literal(keywords, ",") {
        let key = java_trim(&key.to_lowercase()).replace('-', "");
        // skip empty and existing keys
        if !key.is_empty() && !contains_sensitive(&key) && !extra.contains(&key) {
            extra.push(key);
        }
    }
}

/// Removes the keywords added by [`add_sanitize_keywords`]. Only for testing.
#[doc(hidden)]
pub fn reset_sanitize_keywords() {
    extra_keywords().write().unwrap().clear();
}

/// Removes detected sensitive information (such as passwords) from the URI and returns the
/// result.
///
/// Both query parameters whose key is sensitive (see
/// [`crate::sensitive_utils::contains_sensitive`]) and the password of the userinfo
/// (`scheme://user:password@host`) are masked as `xxxxxx`. See
/// [`crate::sensitive_utils::mask_user_info_credentials`] for how the userinfo password is
/// found.
///
/// # Arguments
///
/// * `uri` - the uri to sanitize
///
/// # Returns
///
/// The URI with the passphrase, password or secretKey sanitized.
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::sanitize_uri;
/// assert_eq!("telegram:bots?authorizationToken=xxxxxx",
///            sanitize_uri("telegram:bots?authorizationToken=1234567890:XXAuthTokenHereXX"));
/// assert_eq!("ftp://joe:xxxxxx@host/in", sanitize_uri("ftp://joe:secret@host/in"));
/// ```
pub fn sanitize_uri(uri: &str) -> String {
    let sanitized = sanitize_query_parameters(uri);
    mask_user_info(&sanitized, MASK, false)
}

/// Returns a copy of the parameters where the values of sensitive parameters (such as
/// passwords) are masked, using the same rules as [`sanitize_uri`].
///
/// # Arguments
///
/// * `parameters` - the parameters
///
/// # Returns
///
/// A copy of the parameters with the sensitive values masked (`Null` values are kept).
pub fn sanitize_parameters(parameters: &Parameters) -> Parameters {
    parameters
        .iter()
        .map(|(k, v)| {
            let v = if !v.is_null() && is_sensitive_key(k) {
                ParamValue::from(MASK)
            } else {
                v.clone()
            };
            (k.clone(), v)
        })
        .collect()
}

fn sanitize_query_parameters(uri: &str) -> String {
    let mut sb: Option<String> = None;
    let mut copied = 0;
    let mut pos = 0;
    while let Some(caps) = query_parameter_key().captures_at(uri, pos) {
        // the value starts after the = sign
        pos = caps.get(0).unwrap().end();
        if is_sensitive_key(caps.get(1).unwrap().as_str()) {
            let end = value_end(uri, pos);
            let out = sb.get_or_insert_with(|| String::with_capacity(uri.len()));
            out.push_str(&uri[copied..pos]);
            out.push_str(MASK);
            copied = end;
            pos = end;
        }
        // otherwise continue searching from the start of the value, as it can contain another uri
        if pos >= uri.len() {
            break;
        }
    }
    match sb {
        None => uri.to_string(),
        Some(mut out) => {
            out.push_str(&uri[copied..]);
            out
        }
    }
}

fn is_sensitive_key(key: &str) -> bool {
    // ignore dashes the same way as contains_sensitive, as options can be configured in dash case
    let text = key.to_lowercase().replace('-', "");
    if sensitive_keywords().is_match(&text) {
        return true;
    }
    extra_keywords()
        .read()
        .unwrap()
        .iter()
        .any(|k| text.contains(k.as_str()))
}

fn value_end(uri: &str, start: usize) -> usize {
    let bytes = uri.as_bytes();
    let open = start + RAW_TOKEN_PREFIX.len();
    if uri[start..].starts_with(RAW_TOKEN_PREFIX) && open < uri.len() {
        for (i, s) in RAW_TOKEN_START.iter().enumerate() {
            if bytes[open] as char == *s {
                return raw_value_end(uri, open + 1, RAW_TOKEN_END[i]);
            }
        }
    }
    uri[start..]
        .find('&')
        .map(|e| e + start)
        .unwrap_or(uri.len())
}

fn raw_value_end(uri: &str, from: usize, token_end: char) -> usize {
    // a RAW value ends at the closing bracket that is followed by & or the end, the same way
    // as the URI scanner parses it
    let bytes = uri.as_bytes();
    let mut last = None;
    for i in from..bytes.len() {
        if bytes[i] as char == token_end {
            if i + 1 == bytes.len() || bytes[i + 1] == b'&' {
                return i + 1;
            }
            last = Some(i);
        }
    }
    // no such bracket, which happens when the uri is part of a longer text, so mask up to the
    // last closing bracket
    last.map(|l| l + 1).unwrap_or(uri.len())
}

/// Converts a multi-line text block URI into a single line.
///
/// Text blocks may have new lines with optional white space. The text before the query
/// parameters is kept as-is (lines are joined with a space), as this allows some Camel
/// components to provide code or SQL in the context-path. Query parameters are `key=value`
/// pairs and therefore the lines are trimmed (after the first `?` sign is detected).
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::text_block_to_single_line;
/// assert_eq!("sql:select * from t?a=1&b=2",
///            text_block_to_single_line("sql:select *\n  from t?a=1\n  &b=2\n"));
/// ```
pub fn text_block_to_single_line(uri: &str) -> String {
    let mut query = false;
    let mut sj = String::with_capacity(uri.len());
    for l in java_lines(uri) {
        let mut l = java_trim(l).to_string();
        if !query {
            l.push(' ');
            if l.contains('?') {
                query = true;
                l = java_trim(&l).to_string();
            }
        }
        sj.push_str(&l);
    }
    java_trim(&sj).to_string()
}

/// Java `String.lines()`: splits on `\n`, `\r` or `\r\n` without a trailing empty line.
fn java_lines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push(&s[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                out.push(&s[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Removes detected sensitive information (such as passwords) from the *path part* of a URI
/// (that is, the part without the query parameters or component prefix) and returns the
/// result.
///
/// # Arguments
///
/// * `path` - the URI path to sanitize
///
/// # Returns
///
/// The sanitized path.
pub fn sanitize_path(path: &str) -> String {
    mask_path_user_info(path, MASK)
}

/// Extracts the scheme specific path from the URI that is used as the remainder option when
/// creating endpoints.
///
/// # Arguments
///
/// * `u` - the URI
/// * `use_raw` - whether to force using raw values
///
/// # Returns
///
/// The remainder path.
pub fn extract_remainder_path(u: &JavaUri, use_raw: bool) -> String {
    let path = if use_raw {
        u.raw_scheme_specific_part().to_string()
    } else {
        u.scheme_specific_part()
    };
    // lets trim off any query arguments
    let path = path.strip_prefix("//").unwrap_or(&path);
    before_or(path, "?", path)
}

/// Extracts the query part of the given uri.
///
/// # Returns
///
/// The query parameters or `None` if the uri has no query.
pub fn extract_query(uri: &str) -> Option<String> {
    after(uri, "?")
}

/// Strips the query parameters from the uri.
///
/// # Returns
///
/// The uri without the query parameter.
pub fn strip_query(uri: &str) -> String {
    before_or(uri, "?", uri)
}

/// Parses the query part of the uri (eg the parameters).
///
/// Same as [`parse_query_full`] with `use_raw = false` and `lenient = false`.
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the uri has invalid syntax.
pub fn parse_query(uri: &str) -> Result<Parameters, UriSyntaxError> {
    parse_query_full(uri, false, false)
}

/// Parses the query part of the uri (eg the parameters).
///
/// The URI parameters will by default be URI encoded. However you can define a parameter value
/// with the syntax `key=RAW(value)` which tells Camel to not encode the value, and use the
/// value as is (eg `key=value`) and the value has **not** been encoded.
///
/// A key that occurs several times gets a [`ParamValue::List`] of all its values. Empty
/// parameters (such as from `&&`) are skipped.
///
/// See also [`RAW_TOKEN_PREFIX`], [`RAW_TOKEN_START`] and [`RAW_TOKEN_END`].
///
/// # Arguments
///
/// * `uri` - the uri (query part)
/// * `use_raw` - whether to force using raw values
/// * `lenient` - whether to parse lenient and ignore trailing `&` markers that have no key or
///   value, which can happen when using HTTP components
///
/// # Returns
///
/// The parameters, or an empty map if no parameters (eg never `None`).
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the uri has invalid syntax (such as a trailing `&` when not
/// lenient, or an invalid escape in a key).
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::parse_query_full;
/// let map = parse_query_full("password=RAW(++?w0rd)&serviceName=some+chat", false, false).unwrap();
/// assert_eq!("RAW(++?w0rd)", map["password"]);
/// assert_eq!("some chat", map["serviceName"]);
/// ```
pub fn parse_query_full(
    uri: &str,
    use_raw: bool,
    lenient: bool,
) -> Result<Parameters, UriSyntaxError> {
    if uri.is_empty() {
        return Ok(Parameters::new());
    }
    // must check for trailing & as the uri.split("&") will ignore those
    if !lenient && uri.ends_with('&') {
        return Err(UriSyntaxError::new(
            uri,
            "Invalid uri syntax: Trailing & marker found. Check the uri and remove the trailing & marker.",
            None,
        ));
    }
    UriScanner::default().parse_query(uri, use_raw)
}

/// Scans RAW tokens in the string and returns the list of pair indexes which tell where a RAW
/// token starts and ends in the string.
///
/// This is a companion function with [`is_raw`] and the returned value is supposed to be used
/// as the parameter of that function. Indexes are character (not byte) positions.
///
/// # Arguments
///
/// * `s` - the string to scan RAW tokens
///
/// # Returns
///
/// The list of pair indexes which represent the start and end positions of a RAW token.
///
/// # Examples
///
/// ```
/// use camel_util::{pair::Pair, uri_support::scan_raw};
/// assert_eq!(vec![Pair::new(9, 21)], scan_raw("password=RAW(++?5w0rd)&serviceName=somechat"));
/// ```
pub fn scan_raw(s: &str) -> Vec<Pair<usize>> {
    let chars: Vec<char> = s.chars().collect();
    let mut answer = Vec::new();
    if chars.is_empty() {
        return answer;
    }
    let prefix: Vec<char> = RAW_TOKEN_PREFIX.chars().collect();
    let find = |from: usize, needle: &[char]| -> Option<usize> {
        (from..chars.len()).find(|&i| chars[i..].starts_with(needle))
    };
    let mut offset = 0;
    let mut start = find(0, &prefix);
    while let Some(st) = start {
        if offset >= chars.len() {
            break;
        }
        offset = st + prefix.len();
        for (i, &ts) in RAW_TOKEN_START.iter().enumerate() {
            let token_end = RAW_TOKEN_END[i];
            let mut token_start = prefix.clone();
            token_start.push(ts);
            if chars[st..].starts_with(&token_start) {
                // we search the first end bracket to close the RAW token; as opposed to parsing
                // the query, this doesn't allow end brackets in between because this may be
                // used on the host/path parts of URI and thus we cannot rely on '&'
                match (st + token_start.len()..chars.len()).find(|&j| chars[j] == token_end) {
                    None => {
                        // still return a pair even if the RAW token is not closed
                        answer.push(Pair::new(st, chars.len()));
                        offset = chars.len();
                    }
                    Some(end) => {
                        answer.push(Pair::new(st, end));
                        offset = end + 1;
                    }
                }
            }
        }
        start = find(offset, &prefix);
    }
    answer
}

/// Tests if the index is within any pair of the start and end indexes which represent the
/// start and end positions of a RAW token.
///
/// This is a companion function with [`scan_raw`] and is supposed to consume the returned
/// value of that function as the second parameter `pairs`.
///
/// # Arguments
///
/// * `index` - the index to be tested
/// * `pairs` - the list of pair indexes which represent the start and end positions of a RAW
///   token
///
/// # Returns
///
/// `true` if the index is within any pair of the indexes, `false` otherwise.
pub fn is_raw(index: usize, pairs: &[Pair<usize>]) -> bool {
    for pair in pairs {
        if index < *pair.left() {
            return false;
        }
        if index <= *pair.right() {
            return true;
        }
    }
    false
}

/// Parses the query parameters of the uri (eg the query part).
///
/// # Returns
///
/// The parameters, or an empty map if no parameters (eg never `None`).
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the query has invalid syntax.
pub fn parse_parameters(uri: &JavaUri) -> Result<Parameters, UriSyntaxError> {
    match prepare_query(uri) {
        None => Ok(Parameters::new()),
        Some(q) => parse_query(&q),
    }
}

/// Returns the (decoded) query of the URI, also for opaque URIs (where the query is the part of
/// the scheme specific part after `?`). A leading `?` is skipped.
pub fn prepare_query(uri: &JavaUri) -> Option<String> {
    match uri.query() {
        None => after(&uri.scheme_specific_part(), "?"),
        Some(q) => Some(q.strip_prefix('?').map(String::from).unwrap_or(q)),
    }
}

/// Traverses the given parameters, and resolves any parameter values which use the RAW token
/// syntax `key=RAW(value)`. The RAW tokens are removed, and the value is replaced with just
/// the value.
///
/// See also [`parse_query`].
///
/// # Arguments
///
/// * `parameters` - the uri parameters
pub fn resolve_raw_parameter_values(parameters: &mut Parameters) {
    resolve_raw_parameter_values_with(parameters, |s| s.to_string())
}

/// Traverses the given parameters, and resolves any parameter values which use the RAW token
/// syntax `key=RAW(value)`, applying `on_replace` to each resolved value.
///
/// # Arguments
///
/// * `parameters` - the uri parameters
/// * `on_replace` - function executed when replacing the raw value
pub fn resolve_raw_parameter_values_with(
    parameters: &mut Parameters,
    on_replace: impl Fn(&str) -> String,
) {
    let resolve = |s: &str| {
        // do not encode RAW parameters unless it has %; need to reverse the % -> %25
        resolve_raw(s).map(|raw| on_replace(&raw.replace("%25", "%")))
    };
    for value in parameters.values_mut() {
        match value {
            ParamValue::Null => {}
            ParamValue::Text(s) => {
                if let Some(r) = resolve(s) {
                    *s = r;
                }
            }
            ParamValue::List(list) => {
                for s in list.iter_mut() {
                    if let Some(r) = resolve(s) {
                        *s = r;
                    }
                }
            }
        }
    }
}

/// Creates a URI with the given query (replacing any existing query, keeping the fragment).
///
/// # Arguments
///
/// * `uri` - the uri
/// * `query` - the query to append to the uri (`None` or empty for no query)
///
/// # Returns
///
/// The uri with the query appended.
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the resulting uri has invalid syntax.
pub fn create_uri_with_query(
    uri: &JavaUri,
    query: Option<&str>,
) -> Result<JavaUri, UriSyntaxError> {
    let mut s = uri.to_string();
    if let Some(b) = before(&s, "?").or_else(|| before(&s, "#")) {
        s = b;
    }
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        s.push('?');
        s.push_str(q);
    }
    if !s.contains('#') {
        if let Some(f) = uri.fragment() {
            s.push('#');
            s.push_str(&f);
        }
    }
    JavaUri::parse(&s)
}

/// Strips the prefix from the value.
///
/// Returns the value as-is if not starting with the prefix.
pub fn strip_prefix(value: &str, prefix: &str) -> String {
    value.strip_prefix(prefix).unwrap_or(value).to_string()
}

/// Strips the suffix from the value.
///
/// Returns the value as-is if not ending with the suffix.
pub fn strip_suffix(value: &str, suffix: &str) -> String {
    value.strip_suffix(suffix).unwrap_or(value).to_string()
}

/// Assembles a URL encoded query from the given map.
///
/// # Arguments
///
/// * `options` - the map with the options (eg key/value pairs)
///
/// # Returns
///
/// A query string with `key1=value&key2=value2&...`, or an empty string if there are no
/// options.
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::{create_query_string, Parameters};
/// let mut map = Parameters::new();
/// map.insert("foo".into(), "abc def".into());
/// map.insert("param1".into(), "+447777111222".into());
/// assert_eq!("foo=abc+def&param1=%2B447777111222", create_query_string(&map));
/// ```
pub fn create_query_string(options: &Parameters) -> String {
    create_query_string_encode(options, true)
}

/// Assembles a query from the given map.
///
/// # Arguments
///
/// * `options` - the map with the options (eg key/value pairs)
/// * `encode` - whether to URL encode the query string
///
/// # Returns
///
/// A query string with `key1=value&key2=value2&...`, or an empty string if there are no
/// options.
pub fn create_query_string_encode(options: &Parameters, encode: bool) -> String {
    let keys: Vec<&str> = options.keys().map(String::as_str).collect();
    create_query_string_sorted(&keys, options, encode)
}

/// Assembles a query from the given map, using the ampersand text between parameters.
///
/// # Arguments
///
/// * `options` - the map with the options (eg key/value pairs)
/// * `ampersand` - to use `&` for Java code, and `&amp;` for XML
/// * `encode` - whether to URL encode the query string
#[deprecated(since = "4.1.0", note = "use create_query_string_encode")]
pub fn create_query_string_with_ampersand(
    options: &IndexMap<String, String>,
    ampersand: &str,
    encode: bool,
) -> String {
    let mut rc = String::new();
    for (i, (key, value)) in options.iter().enumerate() {
        if i > 0 {
            rc.push_str(ampersand);
        }
        append_query_string_parameter(key, Some(value), &mut rc, encode);
    }
    rc
}

fn create_query_string_sorted(sorted_keys: &[&str], options: &Parameters, encode: bool) -> String {
    if options.is_empty() {
        return String::new();
    }
    let mut rc = String::with_capacity(128);
    let mut first = true;
    for key in sorted_keys {
        let value = &options[*key];
        if let ParamValue::List(l) = value {
            if l.is_empty() {
                // an empty list has no values
                continue;
            }
        }
        if first {
            first = false;
        } else {
            rc.push('&');
        }
        match value {
            ParamValue::List(list) => {
                for (i, s) in list.iter().enumerate() {
                    if i > 0 {
                        rc.push('&');
                    }
                    append_query_string_parameter(key, Some(s), &mut rc, encode);
                }
            }
            ParamValue::Text(s) => append_query_string_parameter(key, Some(s), &mut rc, encode),
            ParamValue::Null => append_query_string_parameter(key, None, &mut rc, encode),
        }
    }
    rc
}

fn append_query_string_parameter(key: &str, value: Option<&str>, rc: &mut String, encode: bool) {
    if encode {
        rc.push_str(&url_encode(key));
    } else {
        rc.push_str(key);
    }
    let Some(value) = value else {
        return;
    };
    rc.push('=');
    if resolve_raw(value).is_some() {
        // do not encode RAW parameters unless it has %
        rc.push_str(&replace_percent(value));
    } else if encode {
        rc.push_str(&url_encode(value));
    } else {
        rc.push_str(value);
    }
}

/// Creates a URI from the original URI and the remaining parameters.
///
/// Used by various Camel components.
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the resulting uri has invalid syntax.
pub fn create_remaining_uri(
    original_uri: &JavaUri,
    params: &Parameters,
) -> Result<JavaUri, UriSyntaxError> {
    let s = create_query_string(params);
    create_uri_with_query(original_uri, if s.is_empty() { None } else { Some(&s) })
}

/// Appends the given parameters to the given URI.
///
/// It keeps the original parameters and if a new parameter is already defined in
/// `original_uri`, it will be replaced by its value in `new_parameters`.
///
/// # Arguments
///
/// * `original_uri` - the original URI
/// * `new_parameters` - the parameters to add
///
/// # Returns
///
/// The URI with all the parameters.
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the uri syntax is invalid.
pub fn append_parameters_to_uri(
    original_uri: &str,
    new_parameters: &Parameters,
) -> Result<String, UriSyntaxError> {
    let uri = JavaUri::parse(&normalize_uri(original_uri)?)?;
    let mut parameters = parse_parameters(&uri)?;
    for (k, v) in new_parameters {
        parameters.insert(k.clone(), v.clone());
    }
    Ok(create_remaining_uri(&uri, &parameters)?.to_string())
}

/// Normalizes the uri by reordering the parameters so they are sorted and thus we can use the
/// uris for endpoint matching.
///
/// The URI parameters will by default be URI encoded. However you can define a parameter value
/// with the syntax `key=RAW(value)` which tells Camel to not encode the value, and use the
/// value as is (eg `key=value`) and the value has **not** been encoded.
///
/// A normalized uri normalizes to itself, so endpoint keys and lookups agree.
///
/// See also [`RAW_TOKEN_PREFIX`], [`RAW_TOKEN_START`] and [`RAW_TOKEN_END`].
///
/// # Arguments
///
/// * `uri` - the uri
///
/// # Returns
///
/// The normalized uri.
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if the uri syntax is invalid.
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::normalize_uri;
/// assert_eq!("smtp://localhost?from=me&to=foo", normalize_uri("smtp:localhost?to=foo&from=me").unwrap());
/// assert_eq!("http://www.google.com?q=S%C3%B8ren", normalize_uri("http://www.google.com?q=Søren").unwrap());
/// assert_eq!("log://foo?marker=a+b", normalize_uri("log:foo?marker=a b").unwrap());
/// ```
pub fn normalize_uri(uri: &str) -> Result<String, UriSyntaxError> {
    // try to parse using the simpler and faster Camel URI parser
    match fast_parse_uri(uri) {
        Some(ParsedUri::AlreadyNormalized) => return Ok(uri.to_string()),
        Some(ParsedUri::Parts {
            scheme,
            path,
            query,
        }) => {
            // use the faster and more simple normalizer
            if let Some(answer) = do_fast_normalize_uri(&scheme, &path, query.as_deref())? {
                return Ok(answer);
            }
        }
        None => {}
    }
    // use the legacy normalizer as the uri is complex and may have unsafe URL characters
    do_complex_normalize_uri(uri)
}

/// Normalizes the URI so unsafe characters are encoded.
///
/// # Arguments
///
/// * `uri` - the input uri
///
/// # Returns
///
/// The URI instance.
///
/// # Errors
///
/// Returns a [`UriSyntaxError`] if there is a syntax error in the input uri.
pub fn normalize_uri_as_uri(uri: &str) -> Result<JavaUri, UriSyntaxError> {
    let uri = text_block_to_single_line(uri);
    JavaUri::parse(&encode_raw(&uri, true))
}

/// The complex (and Camel 2.x) compatible URI normalizer when the URI is more complex such as
/// having percent encoded values, or other unsafe URL characters, or have authority
/// user/password, etc.
fn do_complex_normalize_uri(uri: &str) -> Result<String, UriSyntaxError> {
    let uri = text_block_to_single_line(uri);
    let u = JavaUri::parse(&encode_raw(&uri, true))?;
    let Some(scheme) = u.scheme() else {
        // not possible to normalize
        return Ok(uri);
    };
    let ssp = u.scheme_specific_part();

    // find start and end position in path as we only check the context-path and not the query
    let start = if ssp.starts_with("//") { 2 } else { 0 };
    let end = ssp.find('?');
    let mut path = if (start == 0 && end == Some(0)) || (start == 2 && end == Some(2)) {
        // special when there is no context path
        String::new()
    } else {
        let p = match end {
            None if start != 0 => ssp[start..].to_string(),
            None => ssp.clone(),
            Some(e) => ssp[start..e].to_string(),
        };
        if scheme.starts_with("http") {
            encode_http_uri(&p)
        } else {
            encode(&p)
        }
    };

    // if we have user info in the path and they use @ in username or password, then we need to
    // encode them (but leave the last @ sign before the hostname). Camel end users may not
    // encode their user info properly, but expect this to work out of the box.
    let idx_path = path.find('/');
    let count = match idx_path {
        Some(i) => count_char_until(&path, '@', path[..i].chars().count()),
        None => count_char(&path, '@'),
    };
    if count > 1 {
        let user_info_path = match idx_path {
            Some(i) if i > 0 => &path[..i],
            _ => path.as_str(),
        };
        let max = user_info_path.rfind('@').unwrap();
        let before = user_info_path[..max].replace('@', "%40");
        // after must be from original path
        let after = &path[max..];
        path = before + after;
    }

    // in case there are parameters we should reorder them
    match prepare_query(&u) {
        None => Ok(build_uri(scheme, &path, None)),
        Some(query) => {
            let parameters = parse_query_full(&query, false, false)?;
            let query = if parameters.len() == 1 {
                create_query_string(&parameters)
            } else {
                // reorder parameters a..z
                let mut keys: Vec<&str> = parameters.keys().map(String::as_str).collect();
                keys.sort_unstable();
                create_query_string_sorted(&keys, &parameters, true)
            };
            Ok(build_uri(scheme, &path, Some(&query)))
        }
    }
}

/// The fast parser for normalizing Camel endpoint URIs when the URI is not complex and can be
/// parsed in a much more efficient way.
///
/// Returns `None` if the uri must be normalized by the complex normalizer.
fn do_fast_normalize_uri(
    scheme: &str,
    path: &str,
    query: Option<&str>,
) -> Result<Option<String>, UriSyntaxError> {
    match query {
        None => Ok(Some(build_uri(scheme, path, None))),
        Some(query) => build_reordering_parameters(scheme, path, query),
    }
}

fn build_reordering_parameters(
    scheme: &str,
    path: &str,
    query: &str,
) -> Result<Option<String>, UriSyntaxError> {
    let parameters = parse_query_full(query, false, false)?;
    let mut keys: Vec<&str> = parameters.keys().map(String::as_str).collect();
    if keys.len() > 1 {
        // reorder parameters a..z
        keys.sort_unstable();
    }
    // always rebuild the query, even if the keys were already in order, so the output does not
    // depend on the incidental original parameter order. Escape only the characters that are
    // genuinely unsafe in a URI rather than the much more aggressive form encoding used by
    // create_query_string, which would needlessly percent-encode characters that are legal
    // unescaped in a URI query, such as ':' (eg host:port) or '/' (eg produces=application/json)
    let query = build_safe_query_string(&keys, &parameters);
    if query.contains('%') {
        // a key or value needed a percent escape (such as = or # in a value), and a uri with %
        // is normalized by the complex normalizer, which form-encodes the whole query; let the
        // complex normalizer normalize this uri as well, so normalizing a normalized uri gives
        // the same uri
        return Ok(None);
    }
    Ok(Some(build_uri(scheme, path, Some(&query))))
}

fn build_safe_query_string(sorted_keys: &[&str], parameters: &Parameters) -> String {
    if parameters.is_empty() {
        return String::new();
    }
    let mut sb = String::with_capacity(128);
    let mut first = true;
    for key in sorted_keys {
        let value = &parameters[*key];
        if let ParamValue::List(l) = value {
            if l.is_empty() {
                continue;
            }
        }
        if first {
            first = false;
        } else {
            sb.push('&');
        }
        match value {
            ParamValue::List(list) => {
                for (i, s) in list.iter().enumerate() {
                    if i > 0 {
                        sb.push('&');
                    }
                    append_safe_query_string_parameter(key, Some(s), &mut sb);
                }
            }
            ParamValue::Text(s) => append_safe_query_string_parameter(key, Some(s), &mut sb),
            ParamValue::Null => append_safe_query_string_parameter(key, None, &mut sb),
        }
    }
    sb
}

fn append_safe_query_string_parameter(key: &str, value: Option<&str>, sb: &mut String) {
    sb.push_str(&safe_encode_query_part(key));
    let Some(value) = value else {
        return;
    };
    sb.push('=');
    if resolve_raw(value).is_some() {
        sb.push_str(&replace_percent(value));
    } else {
        sb.push_str(&safe_encode_query_part(value));
    }
}

fn safe_encode_query_part(text: &str) -> String {
    // '&' and '=' are structurally significant in Camel's key=value&key=value query syntax and
    // must stay escaped inside a key or value even though they are otherwise legal
    let encoded = encode(text).replace('&', "%26").replace('=', "%3D");
    // a space as +, as the complex normalizer writes it; the fast parser only takes uris
    // without %, so %20 here is always a space
    encoded.replace("%20", "+")
}

fn build_uri(scheme: &str, path: &str, query: Option<&str>) -> String {
    // must include :// to do a correct URI all components can work with
    match query {
        Some(q) => format!("{scheme}://{path}?{q}"),
        None => format!("{scheme}://{path}"),
    }
}

/// Extracts (and removes) the properties whose name starts with the given prefix.
///
/// # Arguments
///
/// * `properties` - the properties; matching entries are removed
/// * `option_prefix` - the prefix
///
/// # Returns
///
/// The extracted properties with the prefix removed from their names.
pub fn extract_properties(properties: &mut Parameters, option_prefix: &str) -> Parameters {
    let mut rc = Parameters::new();
    properties.retain(|name, value| match name.strip_prefix(option_prefix) {
        Some(stripped) => {
            rc.insert(stripped.to_string(), value.clone());
            false
        }
        None => true,
    });
    rc
}

/// Returns the uri with its query decoded and re-encoded in sorted order, or `None` if the uri
/// is invalid.
///
/// # Arguments
///
/// * `uri` - the uri
pub fn get_decode_query(uri: &str) -> Option<String> {
    let u = JavaUri::parse(uri).ok()?;
    let uri_without_query = strip_query(uri);
    match prepare_query(&u) {
        None => Some(uri_without_query),
        Some(query) => {
            let parameters = parse_query_full(&query, false, false).ok()?;
            let query = if parameters.len() == 1 {
                create_query_string(&parameters)
            } else {
                let mut keys: Vec<&str> = parameters.keys().map(String::as_str).collect();
                keys.sort_unstable();
                create_query_string_sorted(&keys, &parameters, true)
            };
            Some(format!("{uri_without_query}?{query}"))
        }
    }
}

/// Returns the path (`/` if empty) and query (if any) of the URI, eg `/path?query=value`.
pub fn path_and_query_of(uri: &JavaUri) -> String {
    let mut path_and_query = match uri.path() {
        Some(p) if !p.is_empty() => p,
        _ => "/".to_string(),
    };
    if let Some(q) = uri.query().filter(|q| !q.is_empty()) {
        path_and_query.push('?');
        path_and_query.push_str(&q);
    }
    path_and_query
}

/// Joins the paths with a single `/` between them, ignoring `None` and empty paths.
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::join_paths;
/// assert_eq!("/a/b/c/d/", join_paths(&[Some("/a/"), Some("/b"), Some("c/"), Some("/d/")]));
/// assert_eq!("a/c", join_paths(&[Some("a"), None, Some("c")]));
/// ```
pub fn join_paths(paths: &[Option<&str>]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut added_last = false;
    for (i, path) in paths.iter().enumerate().rev() {
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            continue;
        };
        let path = if added_last {
            strip_suffix(path, "/")
        } else {
            path.to_string()
        };
        added_last = true;
        if path.starts_with('/') || i == 0 {
            parts.push(path);
        } else {
            parts.push(format!("/{path}"));
        }
    }
    parts.reverse();
    parts.concat()
}

/// Builds a query with the same key for every value: `key=v1&key=v2&...`.
pub fn build_multi_value_query<I, T>(key: &str, values: I) -> String
where
    I: IntoIterator<Item = T>,
    T: fmt::Display,
{
    values
        .into_iter()
        .map(|v| format!("{key}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Removes white-space noise from uri, xxxUri attributes, eg new lines, and tabs etc, which
/// allows end users to format their Camel routes in more human-readable format, but at runtime
/// those attributes must be trimmed. The parser removes most of the noise, but keeps spaces in
/// the attribute values.
pub fn remove_noise_from_uri(uri: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"&\s+").unwrap());
    if let (Some(before), Some(after)) = (before(uri, "?"), after(uri, "?")) {
        let changed = java_trim(&re.replace_all(&after, "&")).to_string();
        if after != changed {
            return format!("{}?{}", java_trim(&before), changed);
        }
    }
    uri.to_string()
}

// ---------------------------------------------------------------------------------------
// URIScanner

/// Resolves a RAW value: returns the text inside `RAW(...)` / `RAW{...}`, or `None` if the
/// string is not a RAW value.
///
/// # Examples
///
/// ```
/// use camel_util::uri_support::resolve_raw;
/// assert_eq!(Some("++?w0rd".to_string()), resolve_raw("RAW(++?w0rd)"));
/// assert_eq!(Some("++?w0rd".to_string()), resolve_raw("RAW{++?w0rd}"));
/// assert_eq!(None, resolve_raw("foo"));
/// ```
pub fn resolve_raw(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 4 || chars[0] != 'R' || chars[1] != 'A' || chars[2] != 'W' {
        return None;
    }
    let last = chars[chars.len() - 1];
    let open = chars[3];
    if (last == ')' && open == '(') || (last == '}' && open == '{') {
        Some(chars[4..chars.len() - 1].iter().collect())
    } else {
        None
    }
}

/// Replaces `%` with `%25` to avoid losing `%` when decoding.
pub fn replace_percent(input: &str) -> String {
    input.replace('%', "%25")
}

/// RAW syntax aware URI scanner that ensures the `RAW(...)` (or `RAW{...}`) value is kept as is.
#[derive(Default)]
struct UriScanner {
    key: String,
    value: String,
    value_mode: bool,
    is_raw: bool,
    raw_token_end: char,
}

impl UriScanner {
    fn init_state(&mut self) {
        self.value_mode = false;
        self.key.clear();
        self.value.clear();
        self.is_raw = false;
    }

    fn parse_query(&mut self, uri: &str, use_raw: bool) -> Result<Parameters, UriSyntaxError> {
        // need to parse the uri query parameters manually as we cannot rely on splitting by &,
        // as & can be used in a parameter value as well.
        let mut answer = Parameters::new();
        let chars: Vec<char> = uri.chars().collect();
        let len = chars.len();
        let mut i = 0;
        while i < len {
            let ch = chars[i];
            if !self.value_mode {
                // if there is a = sign then the key ends and we are in value mode
                if ch == '=' {
                    self.value_mode = true;
                    i += 1;
                    continue;
                }
                if ch != '&' {
                    self.key.push(ch);
                }
            } else {
                // are we a raw value
                self.is_raw = self.check_raw();
                // if we are in raw mode, then we keep adding until we hit the end marker
                if self.is_raw {
                    self.value.push(ch);
                    let next = chars.get(i + 1).copied().unwrap_or('\0');
                    if ch == self.raw_token_end && (next == '&' || next == '\0') {
                        // raw value end, so add that as a parameter, and reset flags
                        self.add_parameter(&mut answer, uri, use_raw || self.is_raw)?;
                        self.init_state();
                        // skip to next as we are in raw mode and have already added the value
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                if ch != '&' {
                    self.value.push(ch);
                }
            }
            if ch == '&' {
                // parameter is ended (an empty parameter such as from && is skipped)
                if !self.key.is_empty() || !self.value.is_empty() {
                    self.add_parameter(&mut answer, uri, use_raw || self.is_raw)?;
                }
                self.init_state();
            }
            i += 1;
        }
        // any left over parameters, then add that
        if !self.key.is_empty() {
            self.add_parameter(&mut answer, uri, use_raw || self.is_raw)?;
        }
        Ok(answer)
    }

    fn check_raw(&mut self) -> bool {
        self.raw_token_end = '\0';
        let mut c = self.value.chars();
        match (c.next(), c.next(), c.next(), c.next()) {
            (Some('R'), Some('A'), Some('W'), Some('(')) => {
                self.raw_token_end = RAW_TOKEN_END[0];
                true
            }
            (Some('R'), Some('A'), Some('W'), Some('{')) => {
                self.raw_token_end = RAW_TOKEN_END[1];
                true
            }
            _ => false,
        }
    }

    fn add_parameter(
        &self,
        answer: &mut Parameters,
        uri: &str,
        is_raw: bool,
    ) -> Result<(), UriSyntaxError> {
        let name = url_decode(&self.key).map_err(|e| UriSyntaxError::new(uri, e, None))?;
        let text = if is_raw {
            self.value.clone()
        } else {
            // need to replace % with %25 to avoid losing "%" when decoding
            url_decode(&replace_percent(&self.value))
                .map_err(|e| UriSyntaxError::new(uri, e, None))?
        };
        match answer.get_mut(&name) {
            Some(existing) => {
                // support multiple values, using a list to hold the multiple values
                let list = match std::mem::replace(existing, ParamValue::Null) {
                    ParamValue::List(mut l) => {
                        l.push(text);
                        l
                    }
                    ParamValue::Text(s) => vec![s, text],
                    ParamValue::Null => vec![text],
                };
                *existing = ParamValue::List(list);
            }
            None => {
                answer.insert(name, ParamValue::Text(text));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
