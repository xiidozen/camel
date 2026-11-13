//! The JSON reading and writing of Gson: the lenient reader of `Gson.fromJson` (Java Gson
//! `JsonReader` in lenient mode: unquoted and single quoted names and strings, `=` and `=>`
//! name separators, `;` separators, comments, `NaN` and `Infinity`; all the numbers read as
//! doubles, as the Gson `Object` adapter) and the writer of `Gson.toJson` (HTML safe escapes,
//! null map entries left out unless `serializeNulls`, the Gson pretty printing).

use camel_api::error::{CamelError, CamelResult};
use camel_util::value::{java_double_to_string, Value, ValueMap};

fn syntax(message: &str, pos: usize) -> CamelError {
    CamelError::new(
        camel_api::error::ErrorKind::IllegalArgument,
        format!("com.google.gson.JsonSyntaxException: {message} at character {pos}"),
    )
}

struct Reader<'a> {
    chars: Vec<char>,
    pos: usize,
    text: &'a str,
}

/// The characters ending an unquoted literal (Java `JsonReader.isLiteral`).
fn ends_literal(c: char) -> bool {
    matches!(
        c,
        '/' | '\\'
            | ';'
            | '#'
            | '='
            | '{'
            | '}'
            | '['
            | ']'
            | ':'
            | ','
            | ' '
            | '\t'
            | '\x0c'
            | '\r'
            | '\n'
    )
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Skips the whitespace and the comments (`//`, `#` and `/* */`).
    fn skip(&mut self) -> CamelResult<()> {
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' => self.pos += 1,
                '#' => self.skip_line(),
                '/' => match self.chars.get(self.pos + 1) {
                    Some('/') => self.skip_line(),
                    Some('*') => {
                        let rest: String = self.chars[self.pos + 2..].iter().collect();
                        match rest.find("*/") {
                            Some(i) => self.pos += 2 + rest[..i].chars().count() + 2,
                            None => return Err(syntax("Unterminated comment", self.pos)),
                        }
                    }
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == '\n' || c == '\r' {
                return;
            }
        }
    }

    fn quoted(&mut self, quote: char) -> CamelResult<String> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(syntax("Unterminated string", start));
            };
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some(e) = self.peek() else {
                return Err(syntax("Unterminated escape sequence", self.pos));
            };
            self.pos += 1;
            match e {
                'u' => {
                    let hex: String = self
                        .chars
                        .get(self.pos..self.pos + 4)
                        .map(|h| h.iter().collect())
                        .unwrap_or_default();
                    let code = u32::from_str_radix(&hex, 16)
                        .map_err(|_| syntax("Malformed Unicode escape", self.pos))?;
                    self.pos += 4;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                't' => out.push('\t'),
                'b' => out.push('\x08'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                'f' => out.push('\x0c'),
                '\n' | '\'' | '"' | '\\' | '/' => out.push(e),
                _ => return Err(syntax("Invalid escape sequence", self.pos)),
            }
        }
    }

    fn literal(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(|c| !ends_literal(c)) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn name(&mut self) -> CamelResult<String> {
        match self.peek() {
            Some(q @ ('"' | '\'')) => self.quoted(q),
            Some(_) => {
                let n = self.literal();
                if n.is_empty() {
                    Err(syntax("Expected name", self.pos))
                } else {
                    Ok(n)
                }
            }
            None => Err(syntax("End of input", self.pos)),
        }
    }

    fn value(&mut self) -> CamelResult<Value> {
        self.skip()?;
        match self.peek() {
            None => Err(syntax("End of input", self.pos)),
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some(q @ ('"' | '\'')) => self.quoted(q).map(Value::Text),
            Some(_) => {
                let start = self.pos;
                let l = self.literal();
                if l.is_empty() {
                    return Err(syntax("Expected value", start));
                }
                Ok(literal_value(&l))
            }
        }
    }

    fn object(&mut self) -> CamelResult<Value> {
        self.pos += 1;
        let mut map = ValueMap::new();
        self.skip()?;
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Value::Map(map));
        }
        loop {
            self.skip()?;
            let name = self.name()?;
            self.skip()?;
            match self.peek() {
                Some(':') => self.pos += 1,
                Some('=') => {
                    self.pos += 1;
                    if self.peek() == Some('>') {
                        self.pos += 1;
                    }
                }
                _ => return Err(syntax("Expected ':'", self.pos)),
            }
            let v = self.value()?;
            map.insert(name, v);
            self.skip()?;
            match self.peek() {
                Some(',' | ';') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Value::Map(map));
                }
                _ => return Err(syntax("Unterminated object", self.pos)),
            }
        }
    }

    fn array(&mut self) -> CamelResult<Value> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip()?;
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Value::List(items));
        }
        loop {
            self.skip()?;
            // an omitted value is null in the lenient mode
            if matches!(self.peek(), Some(',' | ';' | ']')) {
                items.push(Value::Null);
            } else {
                items.push(self.value()?);
            }
            self.skip()?;
            match self.peek() {
                Some(',' | ';') => {
                    self.pos += 1;
                    self.skip()?;
                    if self.peek() == Some(']') {
                        items.push(Value::Null);
                        self.pos += 1;
                        return Ok(Value::List(items));
                    }
                }
                Some(']') => {
                    self.pos += 1;
                    return Ok(Value::List(items));
                }
                _ => return Err(syntax("Unterminated array", self.pos)),
            }
        }
    }
}

/// The value of an unquoted literal: a keyword, a number (a double), or the text itself.
fn literal_value(l: &str) -> Value {
    match l.to_ascii_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }
    match l {
        "NaN" => return Value::Float(f64::NAN),
        "Infinity" => return Value::Float(f64::INFINITY),
        "-Infinity" => return Value::Float(f64::NEG_INFINITY),
        _ => {}
    }
    let numeric = l.starts_with(|c: char| c == '-' || c.is_ascii_digit())
        && l.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'));
    match l.parse::<f64>() {
        Ok(f) if numeric => Value::Float(f),
        _ => Value::from(l),
    }
}

/// Reads the JSON text as Gson reads an `Object` (Java `Gson.fromJson(text, Object.class)`):
/// leniently, the numbers as doubles; an empty text is null.
///
/// # Errors
///
/// Malformed JSON, or a text with more than one value.
///
/// # Examples
///
/// ```
/// use camel_gson::json::from_json;
/// use camel_util::value::Value;
///
/// let v = from_json("{value=123; 'b': [1,,true]}").unwrap();
/// let m = v.as_map().unwrap();
/// assert_eq!(Some(&Value::Float(123.0)), m.get("value"));
/// assert_eq!(Some(&Value::List(vec![Value::Float(1.0), Value::Null, Value::Bool(true)])), m.get("b"));
/// assert_eq!(Value::from("A string"), from_json("\"A string\"").unwrap());
/// ```
pub fn from_json(text: &str) -> CamelResult<Value> {
    let mut r = Reader {
        chars: text.chars().collect(),
        pos: 0,
        text,
    };
    r.skip()?;
    if r.peek().is_none() {
        return Ok(Value::Null);
    }
    let v = r.value()?;
    r.skip()?;
    if r.peek().is_some() {
        return Err(CamelError::illegal_argument(format!(
            "com.google.gson.JsonSyntaxException: JSON document was not fully consumed: {}",
            r.text
        )));
    }
    Ok(v)
}

/// Writes a JSON string with the Gson HTML safe escapes.
fn write_string(t: &str, out: &mut String) {
    out.push('"');
    for c in t.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '<' | '>' | '&' | '=' | '\'' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The options of the writer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Whether the null map entries are written.
    pub serialize_nulls: bool,
    /// Whether the JSON is pretty printed (two spaces per level).
    pub pretty_print: bool,
}

fn write(v: &Value, o: &WriteOptions, depth: usize, out: &mut String) -> CamelResult<()> {
    let newline = |out: &mut String, depth: usize| {
        if o.pretty_print {
            out.push('\n');
            out.push_str(&"  ".repeat(depth));
        }
    };
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(&b.to_string()),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) if !f.is_finite() => {
            return Err(CamelError::illegal_argument(format!(
                "{} is not a valid double value as per JSON specification. To override this behavior, use GsonBuilder.serializeSpecialFloatingPointValues() method.",
                java_double_to_string(*f)
            )))
        }
        Value::Float(f) => out.push_str(&java_double_to_string(*f)),
        Value::Text(t) => write_string(t, out),
        Value::List(l) => {
            out.push('[');
            for (i, item) in l.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write(item, o, depth + 1, out)?;
            }
            if !l.is_empty() {
                newline(out, depth);
            }
            out.push(']');
        }
        Value::Map(m) => {
            out.push('{');
            let mut first = true;
            for (k, item) in m {
                if item.is_null() && !o.serialize_nulls {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                newline(out, depth + 1);
                write_string(k, out);
                out.push(':');
                if o.pretty_print {
                    out.push(' ');
                }
                write(item, o, depth + 1, out)?;
            }
            if !first {
                newline(out, depth);
            }
            out.push('}');
        }
        other => write_string(&other.to_string(), out),
    }
    Ok(())
}

/// Writes the value as Gson does (Java `Gson.toJson`).
///
/// # Errors
///
/// A NaN or infinite number.
///
/// # Examples
///
/// ```
/// use camel_gson::json::{to_json, WriteOptions};
/// use camel_util::value::{Value, ValueMap};
///
/// let mut m = ValueMap::new();
/// m.insert("name".into(), Value::from("<Camel>"));
/// m.insert("none".into(), Value::Null);
/// m.insert("n".into(), Value::Float(2.0));
/// let v = Value::Map(m);
/// let compact = to_json(&v, &WriteOptions::default()).unwrap();
/// assert_eq!(r#"{"name":"\u003cCamel\u003e","n":2.0}"#, compact);
/// let pretty = WriteOptions { pretty_print: true, serialize_nulls: true };
/// assert_eq!("{\n  \"name\": \"\\u003cCamel\\u003e\",\n  \"none\": null,\n  \"n\": 2.0\n}", to_json(&v, &pretty).unwrap());
/// ```
pub fn to_json(value: &Value, options: &WriteOptions) -> CamelResult<String> {
    let mut out = String::new();
    write(value, options, 0, &mut out)?;
    Ok(out)
}
