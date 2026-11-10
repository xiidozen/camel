//! Helper for escaping special characters so a value can be embedded safely in HTML, XML, JSON,
//! JavaScript, SQL or a URL. Port of `org.apache.camel.util.EscapeHelper`.
//!
//! Every function returns a [`Cow`]: when nothing needs to be escaped the input is returned
//! borrowed (the Java version returned the same `String` instance).

use crate::net::url_encode;
use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

/// The supported escape kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// HTML text or attribute values, see [`html`].
    Html,
    /// XML text or attribute values, see [`xml`].
    Xml,
    /// Inside a JSON string literal, see [`json`].
    Json,
    /// Inside a JavaScript string literal, see [`js`].
    Js,
    /// Inside a single-quoted SQL string literal, see [`sql`].
    Sql,
    /// A URL query parameter or path segment, see [`url`].
    Url,
}

impl Kind {
    /// All the kinds (the Java `Kind.values()`).
    pub const VALUES: [Kind; 6] = [
        Kind::Html,
        Kind::Xml,
        Kind::Json,
        Kind::Js,
        Kind::Sql,
        Kind::Url,
    ];

    /// Resolves the escape kind from its name (case-insensitive, surrounding whitespace ignored).
    ///
    /// # Arguments
    ///
    /// * `name` - the name such as `html`, `xml`, `json`, `js` (or `javascript`), `sql`, or `url`
    ///
    /// # Returns
    ///
    /// The kind, or `None` if the name is not a supported kind.
    pub fn from_name(name: &str) -> Option<Kind> {
        match name.trim().to_uppercase().as_str() {
            "HTML" => Some(Kind::Html),
            "XML" => Some(Kind::Xml),
            "JSON" => Some(Kind::Json),
            "JS" | "JAVASCRIPT" => Some(Kind::Js),
            "SQL" => Some(Kind::Sql),
            "URL" => Some(Kind::Url),
            _ => None,
        }
    }

    /// The upper-case name of the kind (as the Java enum constant).
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Html => "HTML",
            Kind::Xml => "XML",
            Kind::Json => "JSON",
            Kind::Js => "JS",
            Kind::Sql => "SQL",
            Kind::Url => "URL",
        }
    }

    /// Escapes the value according to this kind.
    ///
    /// # Arguments
    ///
    /// * `value` - the value to escape
    ///
    /// # Returns
    ///
    /// The escaped value.
    pub fn escape<'a>(&self, value: &'a str) -> Cow<'a, str> {
        escape(*self, value)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Kind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Kind::from_name(s).ok_or_else(|| format!("Unknown escape kind: {s}"))
    }
}

/// Escapes the value according to the given kind.
///
/// # Arguments
///
/// * `kind` - the escape kind
/// * `value` - the value to escape
///
/// # Returns
///
/// The escaped value.
pub fn escape(kind: Kind, value: &str) -> Cow<'_, str> {
    match kind {
        Kind::Html => html(value),
        Kind::Xml => xml(value),
        Kind::Json => json(value),
        Kind::Js => js(value),
        Kind::Sql => sql(value),
        Kind::Url => url(value),
    }
}

/// Builds the escaped string lazily: only allocates once the first char needs a replacement.
fn escape_with<'a>(
    value: &'a str,
    rep: impl Fn(usize, char, Option<char>) -> Option<Cow<'static, str>>,
) -> Cow<'a, str> {
    let mut sb: Option<String> = None;
    let mut prev = None;
    for (i, ch) in value.char_indices() {
        match rep(i, ch, prev) {
            Some(r) => {
                let out = sb.get_or_insert_with(|| {
                    let mut s = String::with_capacity(value.len() + 16);
                    s.push_str(&value[..i]);
                    s
                });
                out.push_str(&r);
            }
            None => {
                if let Some(out) = sb.as_mut() {
                    out.push(ch);
                }
            }
        }
        prev = Some(ch);
    }
    match sb {
        Some(s) => Cow::Owned(s),
        None => Cow::Borrowed(value),
    }
}

/// Escapes the value for use in HTML text or attribute values: `&`, `<`, `>`, `"` and `'` are
/// replaced with their entities.
///
/// # Examples
///
/// ```
/// use camel_util::escape_helper::html;
/// assert_eq!("say &quot;hi&quot; &amp; &#39;bye&#39;", html("say \"hi\" & 'bye'"));
/// ```
pub fn html(value: &str) -> Cow<'_, str> {
    escape_with(value, |_, ch, _| {
        Some(Cow::Borrowed(match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&#39;",
            _ => return None,
        }))
    })
}

/// Escapes the value for use in XML text or attribute values: `&`, `<`, `>`, `"` and `'` are
/// replaced with the five predefined XML entities.
pub fn xml(value: &str) -> Cow<'_, str> {
    escape_with(value, |_, ch, _| {
        Some(Cow::Borrowed(match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&apos;",
            _ => return None,
        }))
    })
}

/// Escapes the value for use inside a JSON string literal: quotes, backslashes and control
/// characters are backslash-escaped. The surrounding quotes are not added.
///
/// # Examples
///
/// ```
/// use camel_util::escape_helper::json;
/// assert_eq!("line1\\nline2 \\\"q\\\" a\\u0001b", json("line1\nline2 \"q\" a\u{1}b"));
/// ```
pub fn json(value: &str) -> Cow<'_, str> {
    escape_with(value, |_, ch, _| json_escape(ch))
}

/// Escapes the value for use inside a JavaScript string literal: the JSON rules plus single
/// quotes are backslash-escaped, and `</` is broken up so the value cannot terminate an
/// enclosing `<script>` element. The surrounding quotes are not added.
///
/// # Examples
///
/// ```
/// use camel_util::escape_helper::js;
/// assert_eq!("<\\/script> it\\'s", js("</script> it's"));
/// ```
pub fn js(value: &str) -> Cow<'_, str> {
    escape_with(value, |_, ch, prev| {
        if ch == '\'' {
            Some(Cow::Borrowed("\\'"))
        } else if ch == '/' && prev == Some('<') {
            Some(Cow::Borrowed("\\/"))
        } else {
            json_escape(ch)
        }
    })
}

/// Escapes the value for use inside a single-quoted SQL string literal by doubling single
/// quotes.
///
/// This is a last-resort measure only; prefer parameterized queries wherever possible.
pub fn sql(value: &str) -> Cow<'_, str> {
    if value.contains('\'') {
        Cow::Owned(value.replace('\'', "''"))
    } else {
        Cow::Borrowed(value)
    }
}

/// Escapes the value for use as a URL query parameter or path segment using percent-encoding
/// (RFC 3986).
///
/// Unlike [`url_encode`] (Java `URLEncoder`), a space is encoded as `%20` rather than `+`.
///
/// # Examples
///
/// ```
/// use camel_util::escape_helper::url;
/// assert_eq!("Camel%20in%20Action", url("Camel in Action"));
/// assert_eq!("a%2Bb", url("a+b"));
/// ```
pub fn url(value: &str) -> Cow<'_, str> {
    let encoded = url_encode(value).replace('+', "%20");
    if encoded == value {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(encoded)
    }
}

fn json_escape(ch: char) -> Option<Cow<'static, str>> {
    Some(match ch {
        '"' => Cow::Borrowed("\\\""),
        '\\' => Cow::Borrowed("\\\\"),
        '\u{8}' => Cow::Borrowed("\\b"),
        '\u{c}' => Cow::Borrowed("\\f"),
        '\n' => Cow::Borrowed("\\n"),
        '\r' => Cow::Borrowed("\\r"),
        '\t' => Cow::Borrowed("\\t"),
        c if (c as u32) < 0x20 => Cow::Owned(format!("\\u{:04x}", c as u32)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html() {
        assert_eq!("Monday &amp; Tuesday", html("Monday & Tuesday"));
        assert_eq!("&lt;b&gt;bold&lt;/b&gt;", html("<b>bold</b>"));
        assert_eq!(
            "say &quot;hi&quot; &amp; &#39;bye&#39;",
            html("say \"hi\" & 'bye'")
        );
        assert_eq!("caf\u{e9}", html("caf\u{e9}"));
    }

    #[test]
    fn test_xml() {
        assert_eq!("Monday &amp; Tuesday", xml("Monday & Tuesday"));
        assert_eq!(
            "&lt;a href=&quot;x&quot;&gt;it&apos;s&lt;/a&gt;",
            xml("<a href=\"x\">it's</a>")
        );
    }

    #[test]
    fn test_json() {
        assert_eq!("say \\\"hi\\\"", json("say \"hi\""));
        assert_eq!("c:\\\\temp", json("c:\\temp"));
        assert_eq!(
            "line1\\nline2\\ttab\\r\\b\\f",
            json("line1\nline2\ttab\r\u{8}\u{c}")
        );
        assert_eq!("a\\u0001b", json("a\u{1}b"));
        assert_eq!("it's <b>", json("it's <b>"));
    }

    #[test]
    fn test_js() {
        assert_eq!("it\\'s \\\"quoted\\\"", js("it's \"quoted\""));
        assert_eq!("<\\/script>", js("</script>"));
        assert_eq!("a\\nb", js("a\nb"));
        assert_eq!("a/b", js("a/b"));
    }

    #[test]
    fn test_sql() {
        assert_eq!("O''Reilly", sql("O'Reilly"));
        assert_eq!("'''' ", sql("'' "));
        assert_eq!("plain", sql("plain"));
    }

    #[test]
    fn test_url() {
        assert_eq!("Camel%20in%20Action", url("Camel in Action"));
        assert_eq!("a%26b%3Dc%2Fd%3Fe", url("a&b=c/d?e"));
        assert_eq!("caf%C3%A9", url("caf\u{e9}"));
        assert_eq!("plain-text_1.2*", url("plain-text_1.2*"));
        assert_eq!("a%2Bb", url("a+b")); // + in input -> %2B, not %20
    }

    #[test]
    fn test_no_change_returns_same_instance() {
        let value = "nothing to escape";
        for f in [html, xml, json, js, sql] {
            assert!(matches!(f(value), Cow::Borrowed(v) if std::ptr::eq(v, value)));
        }
    }

    #[test]
    fn test_kind_from_name() {
        assert_eq!(Some(Kind::Html), Kind::from_name("html"));
        assert_eq!(Some(Kind::Xml), Kind::from_name(" XML "));
        assert_eq!(Some(Kind::Json), Kind::from_name("Json"));
        assert_eq!(Some(Kind::Js), Kind::from_name("js"));
        assert_eq!(Some(Kind::Js), Kind::from_name("javascript"));
        assert_eq!(Some(Kind::Sql), Kind::from_name("sql"));
        assert_eq!(Some(Kind::Url), Kind::from_name("url"));
        assert_eq!(None, Kind::from_name("csv"));
        assert!("csv".parse::<Kind>().is_err());
        assert_eq!(Ok(Kind::Url), "url".parse::<Kind>());
        for k in Kind::VALUES {
            assert_eq!(Some(k), Kind::from_name(&k.to_string()));
        }
    }

    #[test]
    fn test_kind_escape() {
        assert_eq!("a &amp; b", Kind::Html.escape("a & b"));
        assert_eq!("a%20%26%20b", Kind::Url.escape("a & b"));
    }
}
