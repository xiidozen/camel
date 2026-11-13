//! The TOON (Token-Oriented Object Notation) encoder and decoder (Java JToon): objects as
//! indented `key: value` lines, arrays with their length in a `[N]` header, inline for
//! primitives (`tags[2]: a,b`), tabular for uniform objects (`users[2]{id,name}:` then one
//! row per object) and `- ` list items otherwise; strings are quoted only when needed.

use camel_api::error::{CamelError, CamelResult};
use camel_util::value::{Value, ValueMap};

/// The delimiter of the array values and the tabular rows (Java JToon `Delimiter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delimiter {
    /// `,` (the default, not shown in the headers).
    #[default]
    Comma,
    /// A tab (`[N\t]` headers).
    Tab,
    /// `|` (`[N|]` headers).
    Pipe,
}

impl Delimiter {
    /// The delimiter of its name (`COMMA`, `TAB`, `PIPE`, ignoring the case; blank for comma).
    ///
    /// # Errors
    ///
    /// An unknown name.
    pub fn parse(name: &str) -> CamelResult<Self> {
        Ok(match name.trim().to_ascii_uppercase().as_str() {
            "" | "COMMA" => Delimiter::Comma,
            "TAB" => Delimiter::Tab,
            "PIPE" => Delimiter::Pipe,
            _ => {
                return Err(CamelError::illegal_argument(format!(
                    "No enum constant dev.toonformat.jtoon.Delimiter.{name}"
                )))
            }
        })
    }

    /// The character of the delimiter.
    pub fn char(self) -> char {
        match self {
            Delimiter::Comma => ',',
            Delimiter::Tab => '\t',
            Delimiter::Pipe => '|',
        }
    }

    fn of_char(c: char) -> Option<Self> {
        match c {
            '\t' => Some(Delimiter::Tab),
            '|' => Some(Delimiter::Pipe),
            ',' => Some(Delimiter::Comma),
            _ => None,
        }
    }

    /// The delimiter as shown in the array headers (nothing for the comma).
    fn header(self) -> &'static str {
        match self {
            Delimiter::Comma => "",
            Delimiter::Tab => "\t",
            Delimiter::Pipe => "|",
        }
    }
}

/// The options of the encoder (Java JToon `EncodeOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeOptions {
    /// The spaces per indentation level (2 by default).
    pub indent: usize,
    /// The delimiter of the arrays.
    pub delimiter: Delimiter,
    /// Whether the lengths of the arrays are prefixed by `#` (`[#3]`).
    pub length_marker: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        EncodeOptions {
            indent: 2,
            delimiter: Delimiter::Comma,
            length_marker: false,
        }
    }
}

/// The options of the decoder (Java JToon `DecodeOptions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeOptions {
    /// The spaces per indentation level (2 by default).
    pub indent: usize,
    /// Whether the lengths, the indentation and the strings are checked (true by default);
    /// otherwise the parsing is best effort.
    pub strict: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        DecodeOptions {
            indent: 2,
            strict: true,
        }
    }
}

fn error(message: impl Into<String>) -> CamelError {
    CamelError::illegal_argument(message)
}

fn is_primitive(v: &Value) -> bool {
    !matches!(v, Value::List(_) | Value::Map(_))
}

/// Whether the text reads as a number (so it is quoted when it is a string).
fn looks_numeric(t: &str) -> bool {
    let body = t.strip_prefix('-').unwrap_or(t);
    if body.is_empty() || !body.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let mut parts = mantissa.splitn(2, '.');
    let int = parts.next().unwrap_or("");
    let ok_int = !int.is_empty() && int.chars().all(|c| c.is_ascii_digit());
    let ok_frac = parts.next().map_or(true, |f| {
        !f.is_empty() && f.chars().all(|c| c.is_ascii_digit())
    });
    let ok_exp = exponent.map_or(true, |e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
    });
    ok_int && ok_frac && ok_exp
}

/// Whether a string value must be quoted.
fn needs_quotes(t: &str, delimiter: Delimiter) -> bool {
    t.is_empty()
        || t != t.trim()
        || matches!(t, "true" | "false" | "null")
        || looks_numeric(t)
        || t.starts_with('-')
        || t.contains([':', '"', '\\', '[', ']', '{', '}', ',', '\n', '\r', '\t'])
        || t.contains(delimiter.char())
        || t.chars().any(char::is_control)
}

fn quote(t: &str) -> String {
    let mut out = String::with_capacity(t.len() + 2);
    out.push('"');
    for c in t.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn encode_key(k: &str) -> String {
    let mut chars = k.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if valid {
        k.to_string()
    } else {
        quote(k)
    }
}

fn encode_number(f: f64) -> String {
    if !f.is_finite() {
        return "null".to_string();
    }
    if f == 0.0 {
        return "0".to_string();
    }
    // the shortest decimal form, without exponent
    let s = f.to_string();
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

/// The text of a primitive value.
fn encode_primitive(v: &Value, delimiter: Delimiter) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => encode_number(*f),
        Value::Text(t) if needs_quotes(t, delimiter) => quote(t),
        Value::Text(t) => t.clone(),
        other => {
            let t = other.to_string();
            if needs_quotes(&t, delimiter) {
                quote(&t)
            } else {
                t
            }
        }
    }
}

struct Encoder {
    options: EncodeOptions,
    lines: Vec<(usize, String)>,
}

impl Encoder {
    fn push(&mut self, depth: usize, line: String) {
        self.lines.push((depth, line));
    }

    fn object(&mut self, map: &ValueMap, depth: usize) {
        for (k, v) in map {
            self.field(&encode_key(k), v, depth, depth + 1, "");
        }
    }

    fn field(&mut self, key: &str, v: &Value, depth: usize, child_depth: usize, prefix: &str) {
        match v {
            Value::List(l) => self.array(key, l, depth, child_depth, prefix),
            Value::Map(m) => {
                self.push(depth, format!("{prefix}{key}:"));
                self.object(m, child_depth);
            }
            p => {
                let text = encode_primitive(p, self.options.delimiter);
                self.push(depth, format!("{prefix}{key}: {text}"));
            }
        }
    }

    /// The fields of the tabular form: all the items are non-empty maps of primitive values
    /// with the same keys.
    fn tabular_fields(items: &[Value]) -> Option<Vec<String>> {
        let Some(Value::Map(first)) = items.first() else {
            return None;
        };
        if first.is_empty() {
            return None;
        }
        for item in items {
            let Value::Map(m) = item else { return None };
            if m.len() != first.len()
                || !first.keys().all(|k| m.contains_key(k))
                || !m.values().all(is_primitive)
            {
                return None;
            }
        }
        Some(first.keys().cloned().collect())
    }

    fn array(
        &mut self,
        key: &str,
        items: &[Value],
        depth: usize,
        child_depth: usize,
        prefix: &str,
    ) {
        let d = self.options.delimiter;
        let marker = if self.options.length_marker { "#" } else { "" };
        let header = format!("{prefix}{key}[{marker}{}{}]", items.len(), d.header());
        let joiner = d.char().to_string();
        if items.is_empty() {
            self.push(depth, format!("{header}:"));
        } else if items.iter().all(is_primitive) {
            let values: Vec<String> = items.iter().map(|v| encode_primitive(v, d)).collect();
            self.push(depth, format!("{header}: {}", values.join(&joiner)));
        } else if let Some(fields) = Self::tabular_fields(items) {
            let names: Vec<String> = fields.iter().map(|f| encode_key(f)).collect();
            self.push(depth, format!("{header}{{{}}}:", names.join(&joiner)));
            for item in items {
                let m = item.as_map().unwrap();
                let row: Vec<String> = fields.iter().map(|f| encode_primitive(&m[f], d)).collect();
                self.push(child_depth, row.join(&joiner));
            }
        } else {
            self.push(depth, format!("{header}:"));
            for item in items {
                self.list_item(item, child_depth);
            }
        }
    }

    fn list_item(&mut self, item: &Value, depth: usize) {
        match item {
            Value::List(l) => self.array("", l, depth, depth + 1, "- "),
            Value::Map(m) if m.is_empty() => self.push(depth, "-".to_string()),
            Value::Map(m) => {
                let mut fields = m.iter();
                let (k, v) = fields.next().unwrap();
                self.field(&encode_key(k), v, depth, depth + 2, "- ");
                for (k, v) in fields {
                    self.field(&encode_key(k), v, depth + 1, depth + 2, "");
                }
            }
            p => {
                let text = encode_primitive(p, self.options.delimiter);
                self.push(depth, format!("- {text}"));
            }
        }
    }
}

/// Encodes the value as TOON text (Java `JToon.encode`): the lines are separated by a line
/// feed, without a final one.
///
/// # Examples
///
/// ```
/// use camel_support::json::parse;
/// use camel_toon::codec::{encode, EncodeOptions};
///
/// let v = parse(r#"{"id":1,"tags":["a","b"],"users":[{"id":1,"name":"Ada"},{"id":2,"name":"Grace"}]}"#).unwrap();
/// assert_eq!(
///     "id: 1\ntags[2]: a,b\nusers[2]{id,name}:\n  1,Ada\n  2,Grace",
///     encode(&v, &EncodeOptions::default())
/// );
/// ```
pub fn encode(value: &Value, options: &EncodeOptions) -> String {
    let mut e = Encoder {
        options: *options,
        lines: Vec::new(),
    };
    match value {
        Value::Map(m) => e.object(m, 0),
        Value::List(l) => e.array("", l, 0, 1, ""),
        p => e.push(0, encode_primitive(p, options.delimiter)),
    }
    let indent = " ".repeat(options.indent);
    let lines: Vec<String> = e
        .lines
        .into_iter()
        .map(|(d, l)| format!("{}{l}", indent.repeat(d)))
        .collect();
    lines.join("\n")
}

/// A non-blank line of the text.
struct Line {
    depth: usize,
    content: String,
    number: usize,
}

/// The header of an array (`key[N<delimiter>]{fields}:`) and the text after it.
struct Header {
    length: usize,
    delimiter: Delimiter,
    fields: Option<Vec<String>>,
    rest: String,
}

/// The index of the first `c` outside of quotes.
fn find_unquoted(t: &str, c: char) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, ch) in t.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
        } else if ch == c {
            return Some(i);
        }
    }
    None
}

struct Decoder {
    lines: Vec<Line>,
    pos: usize,
    strict: bool,
}

impl Decoder {
    fn unquote(&self, t: &str) -> CamelResult<String> {
        let inner = &t[1..];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => {
                    if self.strict && !chars.as_str().trim().is_empty() {
                        return Err(error(format!(
                            "Unexpected characters after a quoted string: {t}"
                        )));
                    }
                    return Ok(out);
                }
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) if !self.strict => {
                        out.push('\\');
                        out.push(other);
                    }
                    other => {
                        return Err(error(format!(
                            "Invalid escape sequence \\{} in {t}",
                            other.map(String::from).unwrap_or_default()
                        )))
                    }
                },
                c => out.push(c),
            }
        }
        if self.strict {
            Err(error(format!("Unterminated string: {t}")))
        } else {
            Ok(out)
        }
    }

    fn primitive(&self, token: &str) -> CamelResult<Value> {
        let t = token.trim();
        if t.starts_with('"') {
            return self.unquote(t).map(Value::Text);
        }
        Ok(match t {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" => Value::Null,
            _ if looks_numeric(t) && !leading_zero(t) => {
                if t.contains(['.', 'e', 'E']) {
                    t.parse::<f64>()
                        .map_or_else(|_| Value::from(t), Value::Float)
                } else {
                    t.parse::<i64>()
                        .map(Value::Int)
                        .or_else(|_| t.parse::<f64>().map(Value::Float))
                        .unwrap_or_else(|_| Value::from(t))
                }
            }
            _ => Value::from(t),
        })
    }

    fn values(&self, t: &str, d: Delimiter) -> CamelResult<Vec<Value>> {
        if t.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut rest = t;
        loop {
            match find_unquoted(rest, d.char()) {
                Some(i) => {
                    out.push(self.primitive(&rest[..i])?);
                    rest = &rest[i + d.char().len_utf8()..];
                }
                None => {
                    out.push(self.primitive(rest)?);
                    return Ok(out);
                }
            }
        }
    }

    /// The key of a field line and the text after it (from the `[` of an array header or
    /// after the `:`); `None` for a line without a key.
    fn key(&self, content: &str) -> CamelResult<Option<(String, String)>> {
        if content.starts_with('"') {
            let mut end = None;
            let mut escaped = false;
            for (i, c) in content.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    end = Some(i);
                    break;
                }
            }
            let Some(end) = end else {
                return Err(error(format!("Unterminated key: {content}")));
            };
            let key = self.unquote(&content[..=end])?;
            let rest = content[end + 1..].trim_start();
            return Ok(match rest.strip_prefix(':') {
                Some(r) => Some((key, format!(":{r}"))),
                None if rest.starts_with('[') => Some((key, rest.to_string())),
                None => None,
            });
        }
        let stop = content.find(['[', ':']);
        Ok(stop.map(|i| (content[..i].trim().to_string(), content[i..].to_string())))
    }

    /// The array header of the text after the key (`[N]...:`).
    fn header(&self, rest: &str) -> CamelResult<Option<Header>> {
        let Some(inner) = rest.strip_prefix('[') else {
            return Ok(None);
        };
        let Some(close) = inner.find(']') else {
            return Ok(None);
        };
        let mut spec = &inner[..close];
        spec = spec.strip_prefix('#').unwrap_or(spec);
        let delimiter = match spec.chars().last().and_then(Delimiter::of_char) {
            Some(d) if d != Delimiter::Comma => {
                spec = &spec[..spec.len() - 1];
                d
            }
            _ => Delimiter::Comma,
        };
        let Ok(length) = spec.trim().parse::<usize>() else {
            return Ok(None);
        };
        let mut after = &inner[close + 1..];
        let mut fields = None;
        if let Some(f) = after.strip_prefix('{') {
            let Some(end) = find_unquoted(f, '}') else {
                return Err(error(format!("Unterminated fields in {rest}")));
            };
            let names = self
                .values(&f[..end], delimiter)?
                .into_iter()
                .map(|v| v.to_string())
                .collect();
            fields = Some(names);
            after = &f[end + 1..];
        }
        let Some(after) = after.trim_start().strip_prefix(':') else {
            return Ok(None);
        };
        Ok(Some(Header {
            length,
            delimiter,
            fields,
            rest: after.trim().to_string(),
        }))
    }

    fn at(&self, depth: usize) -> bool {
        self.lines.get(self.pos).is_some_and(|l| l.depth == depth)
    }

    fn check_length(&self, h: &Header, n: usize) -> CamelResult<()> {
        if self.strict && h.length != n {
            return Err(error(format!(
                "Array length mismatch: expected {} items but got {n}",
                h.length
            )));
        }
        Ok(())
    }

    fn object(&mut self, depth: usize) -> CamelResult<ValueMap> {
        let mut map = ValueMap::new();
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if line.depth < depth {
                break;
            }
            if line.depth > depth {
                if self.strict {
                    return Err(error(format!(
                        "Unexpected indentation at line {}",
                        line.number
                    )));
                }
                self.pos += 1;
                continue;
            }
            let content = line.content.clone();
            self.pos += 1;
            let (k, v) = self.field(&content, depth + 1)?;
            map.insert(k, v);
        }
        Ok(map)
    }

    fn field(&mut self, content: &str, child_depth: usize) -> CamelResult<(String, Value)> {
        let Some((key, rest)) = self.key(content)? else {
            return Err(error(format!("Missing colon after key: {content}")));
        };
        if let Some(h) = self.header(&rest)? {
            let v = self.array(h, child_depth)?;
            return Ok((key, v));
        }
        let Some(value) = rest.strip_prefix(':') else {
            return Err(error(format!("Missing colon after key: {content}")));
        };
        if !value.trim().is_empty() {
            return Ok((key, self.primitive(value)?));
        }
        if self
            .lines
            .get(self.pos)
            .is_some_and(|l| l.depth >= child_depth)
        {
            let m = self.object(child_depth)?;
            return Ok((key, Value::Map(m)));
        }
        Ok((key, Value::Map(ValueMap::new())))
    }

    fn array(&mut self, h: Header, item_depth: usize) -> CamelResult<Value> {
        if let Some(fields) = &h.fields {
            let mut rows = Vec::new();
            while self.at(item_depth) {
                let content = self.lines[self.pos].content.clone();
                self.pos += 1;
                let values = self.values(&content, h.delimiter)?;
                if self.strict && values.len() != fields.len() {
                    return Err(error(format!(
                        "Expected {} values in the row but got {}: {content}",
                        fields.len(),
                        values.len()
                    )));
                }
                let mut m = ValueMap::new();
                for (f, v) in fields.iter().zip(values) {
                    m.insert(f.clone(), v);
                }
                rows.push(Value::Map(m));
            }
            self.check_length(&h, rows.len())?;
            return Ok(Value::List(rows));
        }
        if !h.rest.is_empty() {
            let values = self.values(&h.rest, h.delimiter)?;
            self.check_length(&h, values.len())?;
            return Ok(Value::List(values));
        }
        let mut items = Vec::new();
        while self.at(item_depth) && self.lines[self.pos].content.starts_with('-') {
            let content = self.lines[self.pos].content.clone();
            self.pos += 1;
            items.push(self.list_item(&content, item_depth)?);
        }
        self.check_length(&h, items.len())?;
        Ok(Value::List(items))
    }

    fn list_item(&mut self, content: &str, depth: usize) -> CamelResult<Value> {
        let item = content[1..].trim_start();
        if item.is_empty() {
            return Ok(Value::Map(ValueMap::new()));
        }
        if item.starts_with('[') {
            if let Some(h) = self.header(item)? {
                return self.array(h, depth + 1);
            }
        }
        if let Some((_, rest)) = self.key(item)? {
            let is_field =
                rest.starts_with(':') || (rest.starts_with('[') && self.header(&rest)?.is_some());
            if is_field {
                let mut m = ValueMap::new();
                let (k, v) = self.field(item, depth + 2)?;
                m.insert(k, v);
                while self.at(depth + 1) {
                    let content = self.lines[self.pos].content.clone();
                    self.pos += 1;
                    let (k, v) = self.field(&content, depth + 2)?;
                    m.insert(k, v);
                }
                return Ok(Value::Map(m));
            }
        }
        self.primitive(item)
    }
}

fn leading_zero(t: &str) -> bool {
    let body = t.strip_prefix('-').unwrap_or(t);
    body.len() > 1 && body.starts_with('0') && body.as_bytes()[1].is_ascii_digit()
}

/// Decodes the TOON text (Java `JToon.decode`): an empty text is an empty object, a root
/// `[N]:` header an array, a single line without key a primitive.
///
/// # Errors
///
/// Invalid TOON (in strict mode also: a length that is not the one of its array, an
/// indentation that is not a multiple of the indent, tabs in the indentation).
///
/// # Examples
///
/// ```
/// use camel_toon::codec::{decode, DecodeOptions};
/// use camel_util::value::Value;
///
/// let v = decode("id: 1\ntags[2]: a,b", &DecodeOptions::default()).unwrap();
/// assert_eq!(Some(&Value::Int(1)), v.as_map().unwrap().get("id"));
/// assert!(decode("tags[2]: a,b,c", &DecodeOptions::default()).is_err());
/// ```
pub fn decode(text: &str, options: &DecodeOptions) -> CamelResult<Value> {
    let indent = options.indent.max(1);
    let mut lines = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            continue;
        }
        let spaces = raw.len() - raw.trim_start_matches(' ').len();
        let after = &raw[spaces..];
        if options.strict && after.starts_with('\t') {
            return Err(error(format!(
                "Tabs are not allowed in the indentation at line {}",
                i + 1
            )));
        }
        if options.strict && spaces % indent != 0 {
            return Err(error(format!(
                "The indentation must be a multiple of {indent} spaces at line {}",
                i + 1
            )));
        }
        lines.push(Line {
            depth: spaces / indent,
            content: after.trim_end().to_string(),
            number: i + 1,
        });
    }
    let mut d = Decoder {
        lines,
        pos: 0,
        strict: options.strict,
    };
    let Some(first) = d.lines.first() else {
        return Ok(Value::Map(ValueMap::new()));
    };
    let first = first.content.clone();
    if first.starts_with('[') {
        if let Some(h) = d.header(&first)? {
            d.pos = 1;
            let v = d.array(h, 1)?;
            if d.strict && d.pos < d.lines.len() {
                return Err(error(format!("Unexpected line {}", d.lines[d.pos].number)));
            }
            return Ok(v);
        }
    }
    if d.lines.len() == 1 {
        let is_field = match d.key(&first)? {
            Some((_, rest)) => rest.starts_with(':') || d.header(&rest)?.is_some(),
            None => false,
        };
        if !is_field {
            return d.primitive(&first);
        }
    }
    d.object(0).map(Value::Map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_support::json::parse;

    fn round(json: &str, o: &EncodeOptions) -> (String, Value) {
        let v = parse(json).unwrap();
        let text = encode(&v, o);
        let back = decode(
            &text,
            &DecodeOptions {
                indent: o.indent,
                strict: true,
            },
        )
        .unwrap();
        assert_eq!(v, back, "{text}");
        (text, back)
    }

    #[test]
    fn test_round_trips() {
        let d = EncodeOptions::default();
        assert_eq!(
            "[3]: red,green,blue",
            round(r#"["red","green","blue"]"#, &d).0
        );
        assert_eq!(
            "name: Ada\naddress:\n  city: London\n  zip: 12345",
            round(
                r#"{"name":"Ada","address":{"city":"London","zip":12345}}"#,
                &d
            )
            .0
        );
        assert_eq!("[0]:", round("[]", &d).0);
        assert_eq!("", round("{}", &d).0);
        assert_eq!("1.5", round("1.5", &d).0);
        assert_eq!("null", round("null", &d).0);
        assert_eq!("hello", round(r#""hello""#, &d).0);
        assert_eq!("\"123\"", round(r#""123""#, &d).0);
        round(
            r#"{"comma":"red,green","tab":"left\tright","pipe":"a|b","quotes":"say \"hello\"","newline":"line1\nline2","unicode":"café 日本語","empty":"","dash":"-x","t":"true","sp":" a "}"#,
            &d,
        );
        round(
            r#"{"items":[1,{"a":1,"b":[1,2],"c":{"x":1}},[1,2],[],{},"x",[{"k":1},{"k":[3]}]],"after":true}"#,
            &d,
        );
        round(
            r#"[{"user":{"id":1,"tags":["a"]},"note":"x"},{"rows":[{"a":1,"b":2},{"a":3,"b":4}],"z":0}]"#,
            &d,
        );
        round(r#"{"weird key":1,"a.b":2,"":3,"x":-0.25}"#, &d);
        let pipe = EncodeOptions {
            delimiter: Delimiter::Pipe,
            ..d
        };
        assert_eq!("[3|]: a|b|c", round(r#"["a","b","c"]"#, &pipe).0);
        assert_eq!(
            "t[2|]{a|b}:\n  1|\"x,y\"\n  2|z",
            round(r#"{"t":[{"a":1,"b":"x,y"},{"a":2,"b":"z"}]}"#, &pipe).0
        );
        let tab = EncodeOptions {
            delimiter: Delimiter::Tab,
            indent: 4,
            length_marker: true,
        };
        let (text, _) = round(r#"{"outer":{"inner":"value","l":["a","b"]}}"#, &tab);
        assert_eq!("outer:\n    inner: value\n    l[#2\t]: a\tb", text);
    }

    #[test]
    fn test_decode() {
        let strict = DecodeOptions::default();
        let lenient = DecodeOptions {
            strict: false,
            ..strict
        };
        assert!(decode("tags[2]: a,b,c", &strict).is_err());
        assert_eq!(
            parse(r#"{"tags":["a","b","c"]}"#).unwrap(),
            decode("tags[2]: a,b,c", &lenient).unwrap()
        );
        assert!(decode("a:\n   b: 1", &strict).is_err());
        assert!(decode("a: \"x", &strict).is_err());
        assert_eq!(Value::Map(ValueMap::new()), decode("", &strict).unwrap());
        assert_eq!(Value::from("05"), decode("05", &strict).unwrap());
        assert_eq!(Value::Float(1e3), decode("1e3", &strict).unwrap());
    }
}
