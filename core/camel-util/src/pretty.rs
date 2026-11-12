//! Pretty printing of JSON (Java `Jsoner.prettyPrint`, indented with tabs) and XML (Java
//! `XmlPrettyPrinter.pettyPrint`, a SAX based printer without DOM).

/// Escapes a JSON string value (Java `Jsoner.escape`).
pub fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '/' => out.push_str("\\/"),
            c if c <= '\u{1f}'
                || ('\u{7f}'..='\u{9f}').contains(&c)
                || ('\u{2000}'..='\u{20ff}').contains(&c) =>
            {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Reads the JSON string starting after its opening quote; the unescaped value and the index
/// after the closing quote.
fn json_string(chars: &[char], mut i: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    while i < chars.len() {
        match chars[i] {
            '"' => return Some((value, i + 1)),
            '\\' => {
                let e = *chars.get(i + 1)?;
                i += 2;
                match e {
                    'b' => value.push('\u{8}'),
                    'f' => value.push('\u{c}'),
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'u' => {
                        let hex: String = chars.get(i..i + 4)?.iter().collect();
                        value.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                        i += 4;
                    }
                    other => value.push(other),
                }
            }
            c => {
                value.push(c);
                i += 1;
            }
        }
    }
    None
}

/// Pretty prints the JSON with tabs (Java `Jsoner.prettyPrint`): a new line and indentation
/// after `{`, `[` and `,`, and before `}` and `]`, `": "` between keys and values, and a new
/// line at the end.
///
/// # Returns
///
/// `None` if the text is not JSON.
///
/// # Examples
///
/// ```
/// use camel_util::pretty::pretty_json;
///
/// assert_eq!(
///     Some("{\n\t\"name\": \"Jack\",\n\t\"age\": 44\n}\n".to_string()),
///     pretty_json(r#"{ "name": "Jack", "age": 44 }"#)
/// );
/// ```
pub fn pretty_json(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut level: usize = 0;
    let mut i = 0;
    let indent = |out: &mut String, level: usize| {
        out.push('\n');
        out.push_str(&"\t".repeat(level));
    };
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            ':' => {
                out.push_str(": ");
                i += 1;
            }
            ',' => {
                out.push(',');
                indent(&mut out, level);
                i += 1;
            }
            '{' | '[' => {
                out.push(c);
                level += 1;
                indent(&mut out, level);
                i += 1;
            }
            '}' | ']' => {
                level = level.checked_sub(1)?;
                indent(&mut out, level);
                out.push(c);
                i += 1;
            }
            '"' => {
                let (value, next) = json_string(&chars, i + 1)?;
                out.push('"');
                out.push_str(&json_escape(&value));
                out.push('"');
                i = next;
            }
            _ => {
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], ',' | ':' | '{' | '}' | '[' | ']' | '"')
                {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                let valid = matches!(token.as_str(), "true" | "false" | "null")
                    || token.parse::<f64>().is_ok();
                if !valid {
                    return None;
                }
                out.push_str(&token);
            }
        }
    }
    if level != 0 {
        return None;
    }
    out.push('\n');
    Some(out)
}

/// Decodes the XML entities of a text (as a SAX parser reports it).
fn xml_unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        match after.find(';') {
            Some(end) => {
                let entity = &after[1..end];
                let decoded = match entity {
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "amp" => Some('&'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                        .ok()
                        .and_then(char::from_u32),
                    e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
                    _ => None,
                };
                match decoded {
                    Some(c) => out.push(c),
                    None => out.push_str(&after[..=end]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The attributes of a start tag (after the name).
fn xml_attributes(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut attributes = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let eq = rest
            .find('=')
            .ok_or_else(|| format!("Invalid attribute: {rest}"))?;
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let quote = after
            .chars()
            .next()
            .filter(|q| *q == '"' || *q == '\'')
            .ok_or_else(|| format!("Invalid attribute value of {name}"))?;
        let end = after[1..]
            .find(quote)
            .ok_or_else(|| format!("Unterminated attribute value of {name}"))?;
        attributes.push((name, xml_unescape(&after[1..=end])));
        rest = after[end + 2..].trim_start();
    }
    Ok(attributes)
}

/// Pretty prints the XML (Java `XmlPrettyPrinter.pettyPrint`): each element on its own line,
/// indented with the number of blanks per level, the text of the nested elements on its own
/// line, comments left out, the declaration kept if asked.
///
/// # Errors
///
/// A text that is not well formed XML (as far as this printer reads it).
///
/// # Examples
///
/// ```
/// use camel_util::pretty::pretty_xml;
///
/// assert_eq!(
///     "<person>\n  <name>\n    Jack\n  </name>\n</person>",
///     pretty_xml("<person><name>Jack</name></person>", 2, false).unwrap()
/// );
/// ```
pub fn pretty_xml(xml: &str, blanks: usize, declaration: bool) -> Result<String, String> {
    let pad = |indent: usize| " ".repeat(indent * blanks);
    let mut sb = String::new();
    let mut indent: usize = 0;
    let mut in_element = false;
    let mut stack: Vec<String> = Vec::new();
    let mut rest = xml;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<?") {
            let end = after.find("?>").ok_or("Unterminated declaration")?;
            let content = &after[..end];
            if declaration && content.starts_with("xml") {
                let attrs = xml_attributes(&content[3..])?;
                sb.push_str("<?xml");
                for key in ["version", "encoding", "standalone"] {
                    if let Some((_, v)) = attrs.iter().find(|(k, _)| k == key) {
                        sb.push_str(&format!(" {key}=\"{v}\""));
                    }
                }
                sb.push_str("?>\n");
            }
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->").ok_or("Unterminated comment")?;
            rest = &after[end + 3..];
        } else if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let end = after.find("]]>").ok_or("Unterminated CDATA")?;
            if in_element && indent > 1 {
                sb.push_str(&pad(indent));
                sb.push_str(&after[..end]);
                sb.push('\n');
            }
            rest = &after[end + 3..];
        } else if rest.starts_with("<!") {
            return Err("DOCTYPE is not allowed".into());
        } else if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>').ok_or("Unterminated end tag")?;
            let name = after[..end].trim();
            if stack.pop().as_deref() != Some(name) {
                return Err(format!("Unexpected end tag: {name}"));
            }
            in_element = false;
            indent = indent.saturating_sub(1);
            sb.push_str(&pad(indent));
            sb.push_str(&format!("</{name}>"));
            if indent > 0 {
                sb.push('\n');
            }
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>').ok_or("Unterminated start tag")?;
            let mut tag = &after[..end];
            let empty = tag.ends_with('/');
            if empty {
                tag = &tag[..tag.len() - 1];
            }
            let (name, attrs) = match tag.find(char::is_whitespace) {
                Some(i) => (&tag[..i], xml_attributes(&tag[i..])?),
                None => (tag, Vec::new()),
            };
            in_element = true;
            sb.push_str(&pad(indent));
            sb.push('<');
            sb.push_str(name);
            for (k, v) in &attrs {
                sb.push_str(&format!(" {k}=\"{v}\""));
            }
            sb.push_str(">\n");
            indent += 1;
            if empty {
                in_element = false;
                indent -= 1;
                sb.push_str(&pad(indent));
                sb.push_str(&format!("</{name}>"));
                if indent > 0 {
                    sb.push('\n');
                }
            } else {
                stack.push(name.to_string());
            }
            rest = &after[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let text = &rest[..end];
            if stack.is_empty() && !text.trim().is_empty() {
                return Err("Content is not allowed outside the root element".into());
            }
            if in_element && indent > 1 {
                sb.push_str(&pad(indent));
                sb.push_str(&xml_unescape(text));
                sb.push('\n');
            }
            rest = &rest[end..];
        }
    }
    if !stack.is_empty() {
        return Err(format!("Missing end tag of: {}", stack.join(", ")));
    }
    Ok(sb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json() {
        assert_eq!(
            Some("[\n\t1,\n\ttrue,\n\t\"a\\/b\"\n]\n".to_string()),
            pretty_json("[1, true, \"a/b\"]")
        );
        assert_eq!(None, pretty_json("Hello"));
        assert_eq!(None, pretty_json("{\"a\": 1"));
    }

    #[test]
    fn xml() {
        assert_eq!(
            "<?xml version=\"1.0\"?>\n<a x=\"1\">\n  <b>\n  </b>\n</a>",
            pretty_xml("<?xml version=\"1.0\"?><a x='1'><b/></a>", 2, true).unwrap()
        );
        assert!(pretty_xml("<a><b></a>", 2, false).is_err());
    }
}
