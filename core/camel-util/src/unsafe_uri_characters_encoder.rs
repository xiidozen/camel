//! Encoder for unsafe URI characters. Port of `org.apache.camel.util.UnsafeUriCharactersEncoder`.
//!
//! A good source for details is <http://en.wikipedia.org/wiki/Url_encode>.

use crate::uri_support::{is_raw, scan_raw};

/// A set of unsafe ASCII characters.
#[derive(Debug, Clone, Copy)]
pub struct UnsafeCharacters(u128);

impl UnsafeCharacters {
    const fn of(chars: &[u8]) -> Self {
        let mut bits = 0u128;
        let mut i = 0;
        while i < chars.len() {
            bits |= 1u128 << chars[i];
            i += 1;
        }
        UnsafeCharacters(bits)
    }

    /// Whether the character is in this set (only ASCII characters can be).
    pub fn contains(&self, ch: char) -> bool {
        let c = ch as u32;
        c > 0 && c < 128 && (self.0 >> c) & 1 == 1
    }
}

/// Unsafe characters for the fast endpoint URI parser.
///
/// `#` is allowed as safe as it is used for looking up beans in the registry (`foo=#myBar`).
pub const UNSAFE_CHARACTERS_FAST_PARSER: UnsafeCharacters =
    UnsafeCharacters::of(b" \"<>%{}|\\^~[]`");

/// Unsafe characters as defined by RFC 1738.
pub const UNSAFE_CHARACTERS_RFC1738: UnsafeCharacters = UnsafeCharacters::of(b" \"<>#%{}|\\^~[]`");

/// Unsafe characters for HTTP URIs (as RFC 1738 but allowing `[` and `]`, used by IPv6).
pub const UNSAFE_CHARACTERS_HTTP: UnsafeCharacters = UnsafeCharacters::of(b" \"<>#%{}|\\^~`");

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Whether the given character is safe for the fast endpoint URI parser.
pub fn is_safe_fast_parser(ch: char) -> bool {
    !UNSAFE_CHARACTERS_FAST_PARSER.contains(ch)
}

/// Encodes the unsafe characters (RFC 1738) in the given string.
///
/// A `%` followed by two hex digits is considered already encoded and kept as is.
///
/// # Examples
///
/// ```
/// use camel_util::unsafe_uri_characters_encoder::encode;
/// assert_eq!("%7Bhttp://www.example.com/test%7DServiceName",
///            encode("{http://www.example.com/test}ServiceName"));
/// assert_eq!("like%20'%25A'", encode("like '%A'"));
/// assert_eq!("query=foo%20bar", encode("query=foo%20bar"));
/// ```
pub fn encode(s: &str) -> String {
    encode_with(s, UNSAFE_CHARACTERS_RFC1738, false)
}

/// Encodes the unsafe characters (RFC 1738), optionally skipping the already-encoded check
/// inside `RAW(...)` / `RAW{...}` values.
///
/// # Arguments
///
/// * `s` - the string to encode
/// * `check_raw` - whether a `%` inside a RAW value must always be encoded (as the value is
///   used as-is and not decoded later)
pub fn encode_raw(s: &str, check_raw: bool) -> String {
    encode_with(s, UNSAFE_CHARACTERS_RFC1738, check_raw)
}

/// Encodes the characters that are unsafe in an HTTP URI.
pub fn encode_http_uri(s: &str) -> String {
    encode_with(s, UNSAFE_CHARACTERS_HTTP, false)
}

/// Encodes the characters that are unsafe in an HTTP URI, optionally honouring RAW values.
pub fn encode_http_uri_raw(s: &str, check_raw: bool) -> String {
    encode_with(s, UNSAFE_CHARACTERS_HTTP, check_raw)
}

/// Encodes the given unsafe characters in the string.
///
/// # Arguments
///
/// * `s` - the string to encode
/// * `unsafe_chars` - the set of characters to encode
/// * `check_raw` - whether to treat `%` inside RAW values as unsafe even when it looks encoded
///
/// # Returns
///
/// The encoded string (the input as-is if nothing needed to be encoded).
pub fn encode_with(s: &str, unsafe_chars: UnsafeCharacters, check_raw: bool) -> String {
    if s.is_empty() || !s.chars().any(|c| unsafe_chars.contains(c)) {
        return s.to_string();
    }
    let raw_pairs = if check_raw { scan_raw(s) } else { Vec::new() };
    let chars: Vec<char> = s.chars().collect();
    let mut sb = String::with_capacity(s.len() + 8);
    for (i, &ch) in chars.iter().enumerate() {
        if unsafe_chars.contains(ch) {
            if ch == '%' {
                let next = chars.get(i + 1).copied().unwrap_or(' ');
                let next2 = chars.get(i + 2).copied().unwrap_or(' ');
                if next.is_ascii_hexdigit() && next2.is_ascii_hexdigit() && !is_raw(i, &raw_pairs) {
                    // already encoded, so just append as is
                    sb.push(ch);
                    continue;
                }
            }
            append_escape(&mut sb, ch as u8);
        } else {
            sb.push(ch);
        }
    }
    sb
}

fn append_escape(sb: &mut String, b: u8) {
    sb.push('%');
    sb.push(HEX_DIGITS[(b >> 4) as usize & 0x0f] as char);
    sb.push(HEX_DIGITS[b as usize & 0x0f] as char);
}

#[cfg(test)]
mod tests {
    use super::*;

    // camel-core UnsafeCharactersEncoderTest

    #[test]
    fn test_qname_encoder() {
        assert_eq!(
            "%7Bhttp://www.example.com/test%7DServiceName",
            encode("{http://www.example.com/test}ServiceName")
        );
    }

    #[test]
    fn test_no_encoding() {
        assert_eq!("http://www.example.com", encode("http://www.example.com"));
    }

    #[test]
    fn test_unicodes() {
        assert_eq!(
            "http://test.com/\u{FD04}",
            encode("http://test.com/\u{FD04}")
        );
    }

    #[test]
    fn test_percent_encode() {
        let after = "sql:select%20*%20from%20foo%20where%20bar%20like%20'%25A'";
        assert_eq!(after, encode("sql:select * from foo where bar like '%A'"));
        assert_eq!(after, encode("sql:select * from foo where bar like '%25A'"));
    }

    #[test]
    fn test_percent_encode_danish_char() {
        assert_eq!(
            "http://localhost:%7B%7Bport%7D%7D/myapp/mytest?columns=claus,s\u{F8}ren&username=apiuser",
            encode("http://localhost:{{port}}/myapp/mytest?columns=claus,s\u{F8}ren&username=apiuser")
        );
        assert_eq!(
            "http://localhost:%7B%7Bport%7D%7D/myapp/mytest?columns=claus,s%C3%B8ren&username=apiuser",
            encode("http://localhost:{{port}}/myapp/mytest?columns=claus,s%C3%B8ren&username=apiuser")
        );
    }

    #[test]
    fn test_already_encoded() {
        assert_eq!(
            "http://www.example.com?query=foo%20bar",
            encode("http://www.example.com?query=foo%20bar")
        );
        assert_eq!(
            "http://www.example.com?like=foo%25",
            encode("http://www.example.com?like=foo%25")
        );
    }

    #[test]
    fn test_plus_in_query() {
        assert_eq!(
            "http://www.example.com?param1=%2B447777111222",
            encode("http://www.example.com?param1=%2B447777111222")
        );
        assert_eq!(
            "http://www.example.com?param1=+447777111222",
            encode("http://www.example.com?param1=+447777111222")
        );
    }

    #[test]
    fn test_password_encoding_in_raw_mode() {
        let result = encode_raw("RAW(%j#7%c6i)", true);
        let result = &result[4..result.len() - 1];
        assert_eq!("%25j%237%25c6i", result);
    }

    #[test]
    fn test_safe_fast_parser() {
        assert!(is_safe_fast_parser('#'));
        assert!(!is_safe_fast_parser(' '));
        assert!(!is_safe_fast_parser('%'));
    }

    #[test]
    fn test_http() {
        assert_eq!("[::1]%20x", encode_http_uri("[::1] x"));
        assert_eq!("%5B::1%5D%20x", encode("[::1] x"));
        assert_eq!("a%20b", encode_http_uri_raw("a b", true));
    }
}
