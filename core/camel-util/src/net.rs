//! A `java.net.URI` compatible URI parser and `java.net.URLEncoder`/`URLDecoder` compatible
//! form encoding.
//!
//! Camel's endpoint URI handling (see [`crate::uri_support`]) was written against the exact
//! behaviour of `java.net.URI`: RFC 2396 syntax with the JDK's documented deviations (empty
//! authority before a path, registry-based authority fallback, decoding of the
//! scheme-specific part, ...). General purpose Rust URL crates implement the WHATWG URL
//! standard instead, which normalizes and rejects URIs differently, so this module
//! reproduces the JDK parser to keep endpoint URIs behaving identically after the port.

use std::fmt;

/// Error raised when a string cannot be parsed as a URI reference.
///
/// Port of `java.net.URISyntaxException`; the [`Display`](fmt::Display) output matches
/// `URISyntaxException.getMessage()`, e.g. `Illegal character in path at index 6: foo:a b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriSyntaxError {
    input: String,
    reason: String,
    index: Option<usize>,
}

impl UriSyntaxError {
    /// Creates a new error.
    ///
    /// # Arguments
    ///
    /// * `input` - the input string
    /// * `reason` - a string explaining why the input could not be parsed
    /// * `index` - the index (in characters) at which the parse error occurred, if known
    pub fn new(input: impl Into<String>, reason: impl Into<String>, index: Option<usize>) -> Self {
        UriSyntaxError {
            input: input.into(),
            reason: reason.into(),
            index,
        }
    }

    /// Returns the input string.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Returns a string explaining why the input string could not be parsed.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the index into the input string of the position at which the parse error
    /// occurred, or `None` if this position is not known.
    pub fn index(&self) -> Option<usize> {
        self.index
    }
}

impl fmt::Display for UriSyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(i) => write!(f, "{} at index {}: {}", self.reason, i, self.input),
            None => write!(f, "{}: {}", self.reason, self.input),
        }
    }
}

impl std::error::Error for UriSyntaxError {}

/// A parsed URI reference with the semantics of `java.net.URI`.
///
/// Raw (still percent-encoded) components are available through the `raw_*` accessors,
/// decoded components through the plain accessors. [`fmt::Display`] returns the original
/// string the URI was parsed from (like `URI.toString()`).
///
/// # Examples
///
/// ```
/// use camel_util::net::JavaUri;
///
/// let uri = JavaUri::parse("http://user@localhost:8080/my%20path?q=a%20b#frag").unwrap();
/// assert_eq!(Some("http"), uri.scheme());
/// assert_eq!(Some("localhost"), uri.host());
/// assert_eq!(Some(8080), uri.port());
/// assert_eq!(Some("/my path".to_string()), uri.path());
/// assert_eq!(Some("q=a b".to_string()), uri.query());
/// assert_eq!(Some("frag".to_string()), uri.fragment());
///
/// // opaque URIs have no path or query
/// let opaque = JavaUri::parse("direct:start?x=1").unwrap();
/// assert!(opaque.is_opaque());
/// assert_eq!(None, opaque.query());
/// assert_eq!("start?x=1", opaque.scheme_specific_part());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JavaUri {
    string: String,
    scheme: Option<String>,
    ssp: String,
    authority: Option<String>,
    user_info: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    path: Option<String>,
    query: Option<String>,
    fragment: Option<String>,
}

impl fmt::Display for JavaUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string)
    }
}

impl std::str::FromStr for JavaUri {
    type Err = UriSyntaxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        JavaUri::parse(s)
    }
}

impl JavaUri {
    /// Constructs a URI by parsing the given string (Java `new URI(String)`).
    ///
    /// # Errors
    ///
    /// Returns a [`UriSyntaxError`] if the given string violates RFC 2396, as augmented by
    /// the deviations of `java.net.URI`.
    pub fn parse(s: &str) -> Result<JavaUri, UriSyntaxError> {
        Parser::new(s).parse()
    }

    /// Creates a URI by parsing the given string (Java `URI.create(String)`).
    ///
    /// # Panics
    ///
    /// Panics if the string is not a valid URI; use [`JavaUri::parse`] to handle the error.
    pub fn create(s: &str) -> JavaUri {
        JavaUri::parse(s).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Returns the scheme component of this URI, or `None` if the scheme is undefined.
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    /// Tells whether or not this URI is absolute (has a scheme).
    pub fn is_absolute(&self) -> bool {
        self.scheme.is_some()
    }

    /// Tells whether or not this URI is opaque.
    ///
    /// A URI is opaque if, and only if, it is absolute and its scheme-specific part does not
    /// begin with a slash character (`/`).
    pub fn is_opaque(&self) -> bool {
        self.path.is_none()
    }

    /// Returns the raw scheme-specific part of this URI (never empty, never decoded).
    pub fn raw_scheme_specific_part(&self) -> &str {
        &self.ssp
    }

    /// Returns the decoded scheme-specific part of this URI.
    pub fn scheme_specific_part(&self) -> String {
        decode(&self.ssp)
    }

    /// Returns the raw authority component of this URI, or `None` if undefined.
    pub fn raw_authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Returns the decoded authority component of this URI, or `None` if undefined.
    pub fn authority(&self) -> Option<String> {
        self.authority.as_deref().map(decode)
    }

    /// Returns the raw user-information component of this URI, or `None` if undefined.
    pub fn raw_user_info(&self) -> Option<&str> {
        self.user_info.as_deref()
    }

    /// Returns the decoded user-information component of this URI, or `None` if undefined.
    pub fn user_info(&self) -> Option<String> {
        self.user_info.as_deref().map(decode)
    }

    /// Returns the host component of this URI, or `None` if undefined (for example when the
    /// authority is registry based rather than server based).
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// Returns the port number of this URI, or `None` if undefined (Java returns `-1`).
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// Returns the raw path component of this URI, or `None` if the URI is opaque.
    pub fn raw_path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Returns the decoded path component of this URI, or `None` if the URI is opaque.
    pub fn path(&self) -> Option<String> {
        self.path.as_deref().map(decode)
    }

    /// Returns the raw query component of this URI, or `None` if undefined.
    pub fn raw_query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// Returns the decoded query component of this URI, or `None` if undefined.
    pub fn query(&self) -> Option<String> {
        self.query.as_deref().map(decode)
    }

    /// Returns the raw fragment component of this URI, or `None` if undefined.
    pub fn raw_fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    /// Returns the decoded fragment component of this URI, or `None` if undefined.
    pub fn fragment(&self) -> Option<String> {
        self.fragment.as_deref().map(decode)
    }

    /// Returns the content of this URI as a string (the original input).
    pub fn as_str(&self) -> &str {
        &self.string
    }

    /// Returns the content of this URI as a US-ASCII string: every non-ASCII character is
    /// encoded as UTF-8 percent escapes (Java `URI.toASCIIString()`).
    pub fn to_ascii_string(&self) -> String {
        let mut out = String::with_capacity(self.string.len());
        for c in self.string.chars() {
            if c.is_ascii() {
                out.push(c);
            } else {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------
// character classes (RFC 2396 as used by java.net.URI)

// RFC 2396 grammar used below:
//   mark     = "-_.!~*'()"            unreserved = alphanum | mark
//   reserved = ";/?:@&=+$,[]"         uric       = reserved | unreserved | escaped
//   pchar    = unreserved | escaped | ":@&=+$,"   path = pchar | ";/"
//   userinfo = unreserved | escaped | ";:&=+$,"
//   reg_name = unreserved | escaped | "$,;:@&=+"
//   server   = userinfo | alphanum | "-.:@[]"

#[derive(Clone, Copy)]
struct Class {
    alpha: bool,
    digit: bool,
    extra: &'static str,
    escaped: bool,
}

const C_SCHEME: Class = Class {
    alpha: true,
    digit: true,
    extra: "+-.",
    escaped: false,
};
const C_URIC: Class = Class {
    alpha: true,
    digit: true,
    extra: concat!("-_.!~*'()", ";/?:@&=+$,[]"),
    escaped: true,
};
const C_PATH: Class = Class {
    alpha: true,
    digit: true,
    extra: concat!("-_.!~*'()", ":@&=+$,", ";/"),
    escaped: true,
};
const C_USERINFO: Class = Class {
    alpha: true,
    digit: true,
    extra: concat!("-_.!~*'()", ";:&=+$,"),
    escaped: true,
};
const C_REG_NAME: Class = Class {
    alpha: true,
    digit: true,
    extra: concat!("-_.!~*'()", "$,;:@&=+"),
    escaped: true,
};
const C_SERVER: Class = Class {
    alpha: true,
    digit: true,
    extra: concat!("-_.!~*'()", ";:&=+$,", "-.:@[]"),
    escaped: true,
};
const C_SERVER_PERCENT: Class = Class {
    alpha: true,
    digit: true,
    extra: concat!("-_.!~*'()", ";:&=+$,", "-.:@[]%"),
    escaped: true,
};
const C_DIGIT: Class = Class {
    alpha: false,
    digit: true,
    extra: "",
    escaped: false,
};
const C_ALPHANUM: Class = Class {
    alpha: true,
    digit: true,
    extra: "",
    escaped: false,
};
const C_ALPHANUM_DASH: Class = Class {
    alpha: true,
    digit: true,
    extra: "-",
    escaped: false,
};
const C_IPV6: Class = Class {
    alpha: true,
    digit: true,
    extra: ":.%",
    escaped: false,
};

impl Class {
    fn matches(&self, c: char) -> bool {
        (self.alpha && c.is_ascii_alphabetic())
            || (self.digit && c.is_ascii_digit())
            || (c.is_ascii() && self.extra.contains(c))
    }
}

fn is_other(c: char) -> bool {
    // a non-ASCII character that is neither a space nor a control character
    (c as u32) > 128 && !c.is_whitespace() && !c.is_control() && !is_space_char(c)
}

fn is_space_char(c: char) -> bool {
    matches!(
        c,
        '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

struct Parser<'a> {
    input: &'a str,
    chars: Vec<char>,
    // byte offset of each char index (plus the end)
    offsets: Vec<usize>,
    uri: JavaUri,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        let mut offsets: Vec<usize> = input.char_indices().map(|(i, _)| i).collect();
        offsets.push(input.len());
        Parser {
            input,
            chars,
            offsets,
            uri: JavaUri {
                string: input.to_string(),
                scheme: None,
                ssp: String::new(),
                authority: None,
                user_info: None,
                host: None,
                port: None,
                path: None,
                query: None,
                fragment: None,
            },
        }
    }

    fn sub(&self, start: usize, end: usize) -> String {
        self.input[self.offsets[start]..self.offsets[end]].to_string()
    }

    fn fail<T>(&self, reason: &str, p: usize) -> Result<T, UriSyntaxError> {
        Err(UriSyntaxError::new(self.input, reason, Some(p)))
    }

    fn fail_expecting<T>(&self, expected: &str, p: usize) -> Result<T, UriSyntaxError> {
        self.fail(&format!("Expected {expected}"), p)
    }

    fn at(&self, p: usize, n: usize, c: char) -> bool {
        p < n && self.chars[p] == c
    }

    /// Scan forward until a char in `stop` (returns its index), or `None` if a char in `err`
    /// is found first; returns `end` if neither is found.
    fn scan_stop(&self, start: usize, end: usize, err: &str, stop: &str) -> Option<usize> {
        let mut p = start;
        while p < end {
            let c = self.chars[p];
            if err.contains(c) {
                return None;
            }
            if stop.contains(c) {
                break;
            }
            p += 1;
        }
        Some(p)
    }

    fn scan_to(&self, start: usize, end: usize, stop: &str) -> usize {
        self.scan_stop(start, end, "", stop).unwrap()
    }

    /// Scan forward while chars match the class (handling escapes and "other" chars).
    fn scan_class(&self, start: usize, end: usize, class: Class) -> Result<usize, UriSyntaxError> {
        let mut p = start;
        while p < end {
            let c = self.chars[p];
            if class.matches(c) {
                p += 1;
                continue;
            }
            if class.escaped {
                if c == '%' {
                    if p + 3 <= end
                        && self.chars[p + 1].is_ascii_hexdigit()
                        && self.chars[p + 2].is_ascii_hexdigit()
                    {
                        p += 3;
                        continue;
                    }
                    return self.fail("Malformed escape pair", p);
                }
                if is_other(c) {
                    p += 1;
                    continue;
                }
            }
            break;
        }
        Ok(p)
    }

    fn check_chars(
        &self,
        start: usize,
        end: usize,
        class: Class,
        what: &str,
    ) -> Result<(), UriSyntaxError> {
        let p = self.scan_class(start, end, class)?;
        if p < end {
            return self.fail(&format!("Illegal character in {what}"), p);
        }
        Ok(())
    }

    fn parse(mut self) -> Result<JavaUri, UriSyntaxError> {
        let n = self.chars.len();
        if n == 0 {
            // java.net.URI accepts the empty string as an empty relative reference
            self.uri.path = Some(String::new());
            return Ok(self.uri);
        }
        let p0 = self.scan_stop(0, n, "/?#", ":");
        let (ssp_start, mut p);
        if let Some(pc) = p0.filter(|&pc| self.at(pc, n, ':')) {
            if pc == 0 {
                return self.fail_expecting("scheme name", 0);
            }
            if !self.chars[0].is_ascii_alphabetic() {
                return self.fail("Illegal character in scheme name", 0);
            }
            self.check_chars(1, pc, C_SCHEME, "scheme name")?;
            self.uri.scheme = Some(self.sub(0, pc));
            p = pc + 1;
            ssp_start = p;
            if self.at(p, n, '/') {
                p = self.parse_hierarchical(p, n)?;
            } else {
                let q = self.scan_to(p, n, "#");
                if q <= p {
                    return self.fail_expecting("scheme-specific part", p);
                }
                self.check_chars(p, q, C_URIC, "opaque part")?;
                p = q;
            }
        } else {
            ssp_start = 0;
            p = self.parse_hierarchical(0, n)?;
        }
        self.uri.ssp = self.sub(ssp_start, p);
        if self.at(p, n, '#') {
            self.check_chars(p + 1, n, C_URIC, "fragment")?;
            self.uri.fragment = Some(self.sub(p + 1, n));
            p = n;
        }
        if p < n {
            return self.fail("end of URI", p);
        }
        Ok(self.uri)
    }

    fn parse_hierarchical(&mut self, start: usize, n: usize) -> Result<usize, UriSyntaxError> {
        let mut p = start;
        if self.at(p, n, '/') && self.at(p + 1, n, '/') {
            p += 2;
            let q = self.scan_to(p, n, "/?#");
            if q > p {
                p = self.parse_authority(p, q)?;
            } else if q < n {
                // DEVIATION: allow empty authority prior to non-empty path, query or fragment
            } else {
                return self.fail_expecting("authority", p);
            }
        }
        let q = self.scan_to(p, n, "?#");
        self.check_chars(p, q, C_PATH, "path")?;
        self.uri.path = Some(self.sub(p, q));
        p = q;
        if self.at(p, n, '?') {
            p += 1;
            let q = self.scan_to(p, n, "#");
            self.check_chars(p, q, C_URIC, "query")?;
            self.uri.query = Some(self.sub(p, q));
            p = q;
        }
        Ok(p)
    }

    fn parse_authority(&mut self, start: usize, n: usize) -> Result<usize, UriSyntaxError> {
        let p = start;
        let has_bracket = self.chars[start..n].contains(&']');
        let server_chars = if has_bracket {
            self.scan_class(p, n, C_SERVER_PERCENT).ok() == Some(n)
        } else {
            self.scan_class(p, n, C_SERVER).ok() == Some(n)
        };
        let reg_chars = self.scan_class(p, n, C_REG_NAME).ok() == Some(n);

        if reg_chars && !server_chars {
            self.uri.authority = Some(self.sub(p, n));
            return Ok(n);
        }
        let mut q = p;
        let mut ex: Option<UriSyntaxError> = None;
        if server_chars {
            match self.parse_server(p, n) {
                Ok(end) if end >= n => {
                    q = end;
                    self.uri.authority = Some(self.sub(p, n));
                }
                Ok(end) => {
                    self.uri.user_info = None;
                    self.uri.host = None;
                    self.uri.port = None;
                    ex = Some(UriSyntaxError::new(
                        self.input,
                        "Expected end of authority",
                        Some(end),
                    ));
                }
                Err(e) => {
                    self.uri.user_info = None;
                    self.uri.host = None;
                    self.uri.port = None;
                    ex = Some(e);
                }
            }
        }
        if q < n {
            if reg_chars {
                self.uri.authority = Some(self.sub(p, n));
            } else if let Some(e) = ex {
                return Err(e);
            } else {
                return self.fail("Illegal character in authority", q);
            }
        }
        Ok(n)
    }

    fn parse_server(&mut self, start: usize, n: usize) -> Result<usize, UriSyntaxError> {
        let mut p = start;
        // userinfo
        if let Some(q) = self.scan_stop(p, n, "/?#", "@") {
            if self.at(q, n, '@') {
                self.check_chars(p, q, C_USERINFO, "user info")?;
                self.uri.user_info = Some(self.sub(p, q));
                p = q + 1;
            }
        }
        // hostname, IPv4 address, or IPv6 address
        if self.at(p, n, '[') {
            p += 1;
            let q = self.scan_stop(p, n, "/?#", "]").unwrap_or(p);
            if q > p && self.at(q, n, ']') {
                let end = self.scan_class(p, q, C_IPV6)?;
                if end < q {
                    return self.fail("Malformed IPv6 address", end);
                }
                self.uri.host = Some(self.sub(p - 1, q + 1));
                p = q + 1;
            } else {
                return self.fail_expecting("closing bracket for IPv6 address", q);
            }
        } else {
            let q = self.parse_ipv4(p, n);
            p = match q {
                Some(q) if q > p => {
                    self.uri.host = Some(self.sub(p, q));
                    q
                }
                _ => self.parse_hostname(p, n)?,
            };
        }
        // port
        if self.at(p, n, ':') {
            p += 1;
            let q = self.scan_class(p, n, C_DIGIT)?;
            if q > p {
                match self.sub(p, q).parse::<u16>() {
                    Ok(port) => self.uri.port = Some(port),
                    Err(_) => return self.fail("Malformed port number", p),
                }
                p = q;
            }
        }
        if p < n {
            return self.fail_expecting("port number", p);
        }
        Ok(p)
    }

    fn parse_ipv4(&self, start: usize, n: usize) -> Option<usize> {
        let mut p = start;
        for i in 0..4 {
            if i > 0 {
                if !self.at(p, n, '.') {
                    return None;
                }
                p += 1;
            }
            let q = self.scan_class(p, n, C_DIGIT).ok()?;
            if q <= p || q - p > 3 {
                return None;
            }
            if self.sub(p, q).parse::<u32>().ok()? > 255 {
                return None;
            }
            p = q;
        }
        if p < n && !self.at(p, n, ':') {
            return None;
        }
        Some(p)
    }

    fn parse_hostname(&mut self, start: usize, n: usize) -> Result<usize, UriSyntaxError> {
        let mut p = start;
        let mut l: Option<usize> = None;
        loop {
            // domainlabel = alphanum [ *( alphanum | "-" ) alphanum ]
            let q = self.scan_class(p, n, C_ALPHANUM)?;
            if q <= p {
                break;
            }
            l = Some(p);
            p = q;
            let q = self.scan_class(p, n, C_ALPHANUM_DASH)?;
            if q > p {
                if self.chars[q - 1] == '-' {
                    return self.fail("Illegal character in hostname", q - 1);
                }
                p = q;
            }
            if self.at(p, n, '.') {
                p += 1;
            } else {
                break;
            }
            if p >= n {
                break;
            }
        }
        if p < n && !self.at(p, n, ':') {
            return self.fail("Illegal character in hostname", p);
        }
        let Some(l) = l else {
            return self.fail_expecting("hostname", start);
        };
        // for a fully qualified hostname the rightmost label must start with an alpha char
        if l > start && !self.chars[l].is_ascii_alphabetic() {
            return self.fail("Illegal character in hostname", l);
        }
        self.uri.host = Some(self.sub(start, p));
        Ok(p)
    }
}

/// Decodes percent escapes as UTF-8 the way `java.net.URI` does (malformed sequences are
/// replaced with U+FFFD; `%` inside `[...]` such as an IPv6 scope id is kept as is).
pub fn decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut sb = String::with_capacity(s.len());
    let mut i = 0;
    let mut between_brackets = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            between_brackets = true;
        } else if between_brackets && c == ']' {
            between_brackets = false;
        }
        if c != '%' || between_brackets || i + 3 > chars.len() {
            sb.push(c);
            i += 1;
            continue;
        }
        let mut bytes = Vec::new();
        while i + 3 <= chars.len() && chars[i] == '%' {
            let h = chars[i + 1].to_digit(16);
            let l = chars[i + 2].to_digit(16);
            match (h, l) {
                (Some(h), Some(l)) => {
                    bytes.push((h * 16 + l) as u8);
                    i += 3;
                }
                _ => break,
            }
        }
        if bytes.is_empty() {
            sb.push(c);
            i += 1;
        } else {
            sb.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    sb
}

/// Translates a string into `application/x-www-form-urlencoded` format using UTF-8
/// (Java `URLEncoder.encode(s, UTF_8)`).
///
/// The alphanumeric characters and `.`, `-`, `*`, `_` remain the same, a space becomes `+`
/// and every other character is encoded as `%XX` (upper case hex) of its UTF-8 bytes.
///
/// # Examples
///
/// ```
/// use camel_util::net::url_encode;
/// assert_eq!("abc+def%2C+S%C3%B8ren", url_encode("abc def, Søren"));
/// ```
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decodes an `application/x-www-form-urlencoded` string using UTF-8
/// (Java `URLDecoder.decode(s, UTF_8)`).
///
/// `+` is decoded as a space and `%XX` sequences as UTF-8 bytes.
///
/// # Errors
///
/// Returns an error message (Java threw `IllegalArgumentException`) if the string contains an
/// incomplete trailing escape or illegal hex characters in an escape pattern.
///
/// # Examples
///
/// ```
/// use camel_util::net::url_decode;
/// assert_eq!(Ok("a b+Søren".to_string()), url_decode("a+b%2BS%C3%B8ren"));
/// assert!(url_decode("100%").is_err());
/// ```
pub fn url_decode(s: &str) -> Result<String, String> {
    if !s.contains(['%', '+']) {
        return Ok(s.to_string());
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut start = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push_str(&s[start..i]);
                out.push(' ');
                i += 1;
                start = i;
            }
            b'%' => {
                out.push_str(&s[start..i]);
                let mut buf = Vec::new();
                while i < bytes.len() && bytes[i] == b'%' {
                    if i + 3 > bytes.len() {
                        return Err(
                            "URLDecoder: Incomplete trailing escape (%) pattern".to_string()
                        );
                    }
                    let hex = &bytes[i + 1..i + 3];
                    let hex = String::from_utf8_lossy(hex);
                    match u8::from_str_radix(&hex, 16) {
                        Ok(v) if hex.bytes().all(|b| b.is_ascii_hexdigit()) => buf.push(v),
                        _ => {
                            return Err(format!(
                                "URLDecoder: Illegal hex characters in escape (%) pattern - {hex}"
                            ))
                        }
                    }
                    i += 3;
                }
                out.push_str(&String::from_utf8_lossy(&buf));
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&s[start..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hierarchical() {
        let u =
            JavaUri::parse("ftp://us%40r:t%25st@localhost:21000/tmp3/camel?foo=us@r#x").unwrap();
        assert_eq!(Some("ftp"), u.scheme());
        assert_eq!(Some("us%40r:t%25st"), u.raw_user_info());
        assert_eq!(Some("us@r:t%st".to_string()), u.user_info());
        assert_eq!(Some("localhost"), u.host());
        assert_eq!(Some(21000), u.port());
        assert_eq!(Some("/tmp3/camel".to_string()), u.path());
        assert_eq!(Some("foo=us@r".to_string()), u.query());
        assert_eq!(Some("x".to_string()), u.fragment());
        assert_eq!(
            "//us@r:t%st@localhost:21000/tmp3/camel?foo=us@r",
            u.scheme_specific_part()
        );
    }

    #[test]
    fn registry_based_authority() {
        // two @ signs is not a server based authority, but is accepted as registry based
        let u = JavaUri::parse("sftp://me@example.com@host/in?password=pa=ss").unwrap();
        assert_eq!(None, u.host());
        assert_eq!(Some("me@example.com@host"), u.raw_authority());
        assert_eq!(Some("/in".to_string()), u.path());
        // underscore is not valid in a hostname
        let u = JavaUri::parse("http://my_host:8080/x").unwrap();
        assert_eq!(None, u.host());
        assert_eq!(None, u.port());
    }

    #[test]
    fn ipv4_and_ipv6() {
        let u = JavaUri::parse("http://127.0.0.1:8080/x").unwrap();
        assert_eq!(Some("127.0.0.1"), u.host());
        let u = JavaUri::parse("http://[2a00:8a00:6000:40::1413]:30300/test").unwrap();
        assert_eq!(Some("[2a00:8a00:6000:40::1413]"), u.host());
        assert_eq!(Some(30300), u.port());
    }

    #[test]
    fn empty_authority_and_opaque() {
        let u = JavaUri::parse("foo://?test=1").unwrap();
        assert_eq!(Some("test=1".to_string()), u.query());
        assert!(JavaUri::parse("foo://").is_err());
        let u = JavaUri::parse("smtp:localhost?x=1").unwrap();
        assert!(u.is_opaque());
        assert_eq!("localhost?x=1", u.scheme_specific_part());
        assert!(JavaUri::parse("foo:").is_err());
    }

    #[test]
    fn illegal_characters() {
        let e = JavaUri::parse("foo:a b").unwrap_err();
        assert_eq!(
            "Illegal character in opaque part at index 5: foo:a b",
            e.to_string()
        );
        let e = JavaUri::parse("http://h/a b").unwrap_err();
        assert_eq!(
            "Illegal character in path at index 10: http://h/a b",
            e.to_string()
        );
        let e = JavaUri::parse("http://h/a%zz").unwrap_err();
        assert_eq!(
            "Malformed escape pair at index 10: http://h/a%zz",
            e.to_string()
        );
        let e = JavaUri::parse(":x").unwrap_err();
        assert_eq!("Expected scheme name at index 0: :x", e.to_string());
        // non-ascii is allowed
        assert!(JavaUri::parse("http://www.google.com?q=S\u{f8}ren").is_ok());
    }

    #[test]
    fn ascii_string() {
        let u = JavaUri::parse("http://h?q=S\u{f8}ren").unwrap();
        assert_eq!("http://h?q=S%C3%B8ren", u.to_ascii_string());
        assert_eq!("http://h?q=S\u{f8}ren", u.to_string());
    }

    #[test]
    fn form_encoding() {
        assert_eq!("param1%3D%2B44", url_encode("param1=+44"));
        assert_eq!(Ok("+44".to_string()), url_decode("%2B44"));
        assert_eq!(Ok("a b".to_string()), url_decode("a+b"));
        assert!(url_decode("%zz").is_err());
        assert!(url_decode("%2").is_err());
    }

    #[test]
    fn decode_like_java() {
        assert_eq!("S\u{f8}ren Hansen", decode("S%C3%B8ren%20Hansen"));
        assert_eq!("a+b", decode("a+b"));
    }
}
