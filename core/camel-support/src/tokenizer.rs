//! The tokenize language (Java `TokenizeLanguage` with the tokenizers of `ExpressionBuilder`:
//! `tokenizeExpression`, `regexTokenizeExpression`, `TokenPairExpressionIterator`,
//! `TokenXMLExpressionIterator`, and the group and skip first iterators).
//!
//! The text (the body, or the `source`: `header:name`, `exchangeProperty:name`,
//! `variable:name`) is split into a list of tokens:
//!
//! * by a token (`,`), or a regular expression;
//! * between a start and an end token (a pair, with or without the tokens);
//! * into the XML elements of a tag (`person`, any namespace prefix), optionally with the
//!   namespaces of an ancestor tag added to each element.
//!
//! Then the tokens are optionally grouped (`group`: a number of tokens joined by the
//! delimiter, which may be a simple expression), and the first token skipped. The tokens may
//! be simple expressions (`${header.foo}`).
//!
//! # Examples
//!
//! ```
//! use camel_api::context::StandaloneContext;
//! use camel_api::exchange::Exchange;
//! use camel_support::tokenizer::TokenizeExpression;
//! use camel_util::value::Value;
//!
//! let e = Exchange::with_body(StandaloneContext::shared(), "a,b,c,d,e");
//! let groups = TokenizeExpression::token(",").group("2").evaluate_tokens(&e).unwrap();
//! assert_eq!(Some(vec![Value::from("a,b"), Value::from("c,d"), Value::from("e")]), groups);
//! ```

use crate::builder::named_expression;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::SharedExpression;
use camel_util::value::Value;
use regex::Regex;

/// The kind of tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Token,
    Regex,
    Pair { end: String, include_tokens: bool },
    Xml { inherit_namespace: Option<String> },
}

/// A tokenize expression and its options (Java `TokenizerExpression`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizeExpression {
    token: String,
    kind: Kind,
    source: Option<String>,
    group: Option<String>,
    group_delimiter: Option<String>,
    skip_first: bool,
}

/// Whether the text has a simple function (Java `LanguageSupport.hasSimpleFunction`).
fn has_simple_function(text: &str) -> bool {
    text.contains("${") || text.contains("$simple{")
}

/// The text with its simple functions evaluated.
fn resolve(exchange: &Exchange, text: &str) -> CamelResult<String> {
    if !has_simple_function(text) {
        return Ok(text.to_string());
    }
    let simple = exchange.context().resolve_language("simple")?;
    Ok(simple
        .create_expression(text)?
        .evaluate(exchange)?
        .to_string())
}

impl TokenizeExpression {
    fn new(token: &str, kind: Kind) -> Self {
        TokenizeExpression {
            token: token.to_string(),
            kind,
            source: None,
            group: None,
            group_delimiter: None,
            skip_first: false,
        }
    }

    /// Splits by the token (Java `tokenize(",")`).
    pub fn token(token: &str) -> Self {
        Self::new(token, Kind::Token)
    }

    /// Splits by the regular expression (Java `tokenize(regex, true)`).
    pub fn regex(regex: &str) -> Self {
        Self::new(regex, Kind::Regex)
    }

    /// The texts between the start and end tokens (Java `tokenizePair`).
    pub fn pair(start: &str, end: &str, include_tokens: bool) -> Self {
        Self::new(
            start,
            Kind::Pair {
                end: end.to_string(),
                include_tokens,
            },
        )
    }

    /// The XML elements of the tag, with the namespaces of the ancestor tag if any (Java
    /// `tokenizeXML`).
    pub fn xml(tag: &str, inherit_namespace_tag: Option<&str>) -> Self {
        Self::new(
            tag,
            Kind::Xml {
                inherit_namespace: inherit_namespace_tag.map(str::to_string),
            },
        )
    }

    /// The source of the text (`header:name`, `exchangeProperty:name`, `variable:name`;
    /// default the body).
    pub fn source(mut self, source: &str) -> Self {
        self.source = Some(source.to_string());
        self
    }

    /// Groups the tokens by the number (a number, or a simple expression).
    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }

    /// The delimiter joining the tokens of a group (default the token; none for XML and
    /// pairs with their tokens).
    pub fn group_delimiter(mut self, delimiter: &str) -> Self {
        self.group_delimiter = Some(delimiter.to_string());
        self
    }

    /// Skips the first token (such as a header line).
    pub fn skip_first(mut self) -> Self {
        self.skip_first = true;
        self
    }

    /// The text to tokenize, `None` for a missing value.
    fn text(&self, exchange: &Exchange) -> Option<String> {
        let value = match self.source.as_deref() {
            None | Some("") | Some("body") => exchange.message().body().clone(),
            Some(s) => {
                if let Some(h) = s.strip_prefix("header:") {
                    exchange.message().header(h).cloned().unwrap_or_default()
                } else if let Some(p) = s
                    .strip_prefix("exchangeProperty:")
                    .or_else(|| s.strip_prefix("property:"))
                {
                    exchange.property(p).cloned().unwrap_or_default()
                } else {
                    let v = s.strip_prefix("variable:").unwrap_or(s);
                    exchange.variable(v).cloned().unwrap_or_default()
                }
            }
        };
        match value {
            Value::Null => None,
            Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
            v => Some(v.to_string()),
        }
    }

    /// The tokens of the exchange (`None` for a missing text).
    ///
    /// # Errors
    ///
    /// An invalid regular expression, an invalid XML tag, or a failing simple expression.
    pub fn evaluate_tokens(&self, exchange: &Exchange) -> CamelResult<Option<Vec<Value>>> {
        let Some(text) = self.text(exchange) else {
            return Ok(None);
        };
        let token = resolve(exchange, &self.token)?;
        let mut tokens = match &self.kind {
            Kind::Token => tokenize(&text, &token),
            Kind::Regex => {
                let re = Regex::new(&token).map_err(|e| {
                    CamelError::illegal_argument(format!("Invalid regular expression {token}: {e}"))
                })?;
                regex_tokenize(&text, &re)
            }
            Kind::Pair {
                end,
                include_tokens,
            } => pair_tokenize(&text, &token, &resolve(exchange, end)?, *include_tokens),
            Kind::Xml { inherit_namespace } => {
                let inherit = match inherit_namespace {
                    Some(i) => Some(resolve(exchange, i)?),
                    None => None,
                };
                xml_tokenize(&text, &token, inherit.as_deref())?
            }
        };
        match &self.group {
            None => {
                if self.skip_first && !tokens.is_empty() {
                    tokens.remove(0);
                }
                Ok(Some(tokens.into_iter().map(Value::from).collect()))
            }
            Some(group) => {
                let size: usize = resolve(exchange, group)?.trim().parse().map_err(|_| {
                    CamelError::illegal_argument(format!(
                        "The group must be a number, was: {group}"
                    ))
                })?;
                if size == 0 {
                    return Err(CamelError::illegal_argument(
                        "The group must be a positive number, was: 0",
                    ));
                }
                if self.skip_first && !tokens.is_empty() {
                    tokens.remove(0);
                    if tokens.is_empty() {
                        // only the skipped token: one empty group (as Java)
                        return Ok(Some(vec![Value::from("")]));
                    }
                }
                let delimiter = match &self.group_delimiter {
                    Some(d) => d.clone(),
                    None => match &self.kind {
                        Kind::Xml { .. } => String::new(),
                        Kind::Pair {
                            include_tokens: true,
                            ..
                        } => String::new(),
                        _ => token,
                    },
                };
                Ok(Some(
                    tokens
                        .chunks(size)
                        .map(|c| Value::from(c.join(&delimiter)))
                        .collect(),
                ))
            }
        }
    }

    /// The expression of the tokenizer (its value is the list of tokens).
    pub fn into_expression(self) -> SharedExpression {
        let name = format!("tokenize({})", self.token);
        named_expression(name, move |e| {
            Ok(self
                .evaluate_tokens(e)?
                .map(Value::List)
                .unwrap_or_default())
        })
    }
}

/// Splits by the token as a Java `Scanner` with the token as delimiter: a leading token is
/// skipped, empty tokens between two delimiters are kept, a trailing one is not.
pub fn tokenize(text: &str, token: &str) -> Vec<String> {
    if token.is_empty() {
        return if text.is_empty() {
            vec![]
        } else {
            vec![text.to_string()]
        };
    }
    let text = text.strip_prefix(token).unwrap_or(text);
    let mut parts: Vec<String> = text.split(token).map(str::to_string).collect();
    if parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}

/// Splits by the regular expression as a Java `Scanner` (empty leading and trailing tokens
/// skipped).
pub fn regex_tokenize(text: &str, re: &Regex) -> Vec<String> {
    let mut parts: Vec<String> = re.split(text).map(str::to_string).collect();
    while parts.first().is_some_and(String::is_empty) {
        parts.remove(0);
    }
    while parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}

/// The texts between the start and the end tokens (Java `TokenPairExpressionIterator`).
pub fn pair_tokenize(text: &str, start: &str, end: &str, include_tokens: bool) -> Vec<String> {
    let mut answer = Vec::new();
    let mut rest = text;
    while let Some(s) = rest.find(start) {
        let after = &rest[s + start.len()..];
        let Some(e) = after.find(end) else {
            break;
        };
        let inner = &after[..e];
        answer.push(if include_tokens {
            format!("{start}{inner}{end}")
        } else {
            inner.to_string()
        });
        rest = &after[e + end.len()..];
    }
    answer
}

/// The tag name without `<` and `>`.
fn tag_name(tag: &str) -> CamelResult<String> {
    let mut t = tag.trim().to_string();
    if !t.starts_with('<') {
        t.insert(0, '<');
    }
    if !t.ends_with('>') {
        t.push('>');
    }
    let name = &t[1..t.len() - 1];
    if name.is_empty() || name.contains('<') || name.contains('>') {
        return Err(CamelError::illegal_argument(format!(
            "XML Tag token must be a valid XML tag, was: {tag}"
        )));
    }
    Ok(name.to_string())
}

/// Whether the name (with an optional prefix of up to 15 characters) is the local name.
fn name_matches(name: &str, local: &str) -> bool {
    match name.rsplit_once(':') {
        Some((prefix, l)) => {
            l == local
                && !prefix.is_empty()
                && prefix.chars().count() <= 15
                && !prefix.contains(['<', '>'])
        }
        None => name == local,
    }
}

/// The start of the next element of the name from the position: its index, the end of the
/// start tag (after `>`), and whether it is empty (`/>`).
fn next_start_tag(text: &str, from: usize, local: &str) -> Option<(usize, usize, bool)> {
    let mut pos = from;
    while let Some(i) = text[pos..].find('<').map(|i| i + pos) {
        let rest = &text[i + 1..];
        let name_len = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_len];
        if !name.is_empty() && !name.starts_with(['/', '?', '!']) && name_matches(name, local) {
            if let Some(gt) = rest.find('>') {
                let end = i + 1 + gt + 1;
                let empty = rest[..gt].ends_with('/');
                return Some((i, end, empty));
            }
            return None;
        }
        pos = i + 1;
    }
    None
}

/// The end of the first closing tag of the name from the position (after its `>`).
fn next_end_tag(text: &str, from: usize, local: &str) -> Option<usize> {
    let mut pos = from;
    while let Some(i) = text[pos..].find("</").map(|i| i + pos) {
        let rest = &text[i + 2..];
        let name_len = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len());
        if name_matches(&rest[..name_len], local) {
            let after = &rest[name_len..];
            let ws = after.len() - after.trim_start().len();
            if after[ws..].starts_with('>') {
                return Some(i + 2 + name_len + ws + 1);
            }
        }
        pos = i + 2;
    }
    None
}

/// The namespace declarations (`xmlns="..."`, `xmlns:p='...'`) of a start tag, in order.
fn namespaces(tag: &str) -> Vec<String> {
    let re = Regex::new(r#"xmlns(:\w+|)\s*=\s*('[^']+'|"[^"]+")"#).expect("a valid regex");
    re.captures_iter(tag)
        .map(|c| format!("xmlns{}={}", &c[1], &c[2]))
        .collect()
}

/// The XML elements of the tag (Java `TokenXMLExpressionIterator`): `<tag .../>` or
/// `<tag ...>...</tag>` with any namespace prefix; with the inherited namespace tag, the
/// namespaces declared on its first element are added to each element that does not declare
/// them.
///
/// # Errors
///
/// An invalid tag.
pub fn xml_tokenize(
    text: &str,
    tag: &str,
    inherit_namespace_tag: Option<&str>,
) -> CamelResult<Vec<String>> {
    let local = tag_name(tag)?;
    let mut root_namespaces = Vec::new();
    let mut pos = 0;
    if let Some(inherit) = inherit_namespace_tag {
        let inherit = tag_name(inherit)?;
        if let Some((start, end, _)) = next_start_tag(text, 0, &inherit) {
            root_namespaces = namespaces(&text[start..end]);
        }
    }
    let mut answer = Vec::new();
    while let Some((start, head_end, empty)) = next_start_tag(text, pos, &local) {
        let end = if empty {
            head_end
        } else {
            match next_end_tag(text, head_end, &local) {
                Some(e) => e,
                None => break,
            }
        };
        let mut token = text[start..end].to_string();
        if !root_namespaces.is_empty() {
            let head_len = head_end - start;
            let mut head = token[..head_len - 1].to_string();
            let self_closing = head.ends_with('/');
            if self_closing {
                head.pop();
            }
            let contained = namespaces(&head);
            let missing: String = root_namespaces
                .iter()
                .filter(|n| !contained.contains(n))
                .map(|n| format!(" {n}"))
                .collect();
            token = format!(
                "{head}{missing}{}{}",
                if self_closing { "/>" } else { ">" },
                &token[head_len..]
            );
        }
        answer.push(token);
        pos = end;
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizers() {
        assert_eq!(vec!["a", "", "b"], tokenize("a,,b,", ","));
        let re = Regex::new(r"(\W+)\s*").unwrap();
        assert_eq!(
            vec!["The", "little", "fox"],
            regex_tokenize("The little fox", &re)
        );
        assert_eq!(
            vec!["James", "Claus"],
            pair_tokenize("x[S]James[E]\n[S]Claus[E]y", "[S]", "[E]", false)
        );
        assert_eq!(
            vec!["<a:person x='/'/>", "<person>B</person>"],
            xml_tokenize(
                "<persons><a:person x='/'/><person>B</person></persons>",
                "person",
                None
            )
            .unwrap()
        );
        assert_eq!(
            vec!["<person xmlns=\"n\">A</person>"],
            xml_tokenize(
                "<persons xmlns=\"n\"><person>A</person></persons>",
                "<person>",
                Some("persons")
            )
            .unwrap()
        );
    }
}
