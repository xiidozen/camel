//! # camel-jsoup
//!
//! HTML functions of the simple language (Java `SimpleJsoupFunction` and
//! `JSoupExpressionBuilder`, on jsoup): `${htmlClean()}` keeps only the HTML of the jsoup
//! `Safelist.basic()` (links get `rel="nofollow"`), `${htmlParse()}` parses an HTML document
//! ([`HtmlDocument`], with its `title` and `body` text) and `${htmlDecode()}` gives the text
//! of the HTML. The functions take the body, or their argument (`${htmlClean(${body})}`).
//! This crate is the Rust port of the Java `components/camel-jsoup` module.
//!
//! # Examples
//!
//! ```
//! use camel_jsoup::{clean, text};
//!
//! assert_eq!("<p>Hi <b>you</b></p>", clean("<p onclick='x()'>Hi <b>you</b><script>evil()</script></p>"));
//! assert_eq!("My Camel Some blah blah", text("<html><title>My Camel</title><body><p>Some blah blah</p></body></html>"));
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::spi::simple_function::{SimpleFunction, SimpleFunctionRegistry};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::{Bean, Value};
use scraper::{ElementRef, Html, Node, Selector};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The tags of jsoup `Safelist.basic()`.
const BASIC_TAGS: [&str; 24] = [
    "a",
    "b",
    "blockquote",
    "br",
    "cite",
    "code",
    "dd",
    "dl",
    "dt",
    "em",
    "i",
    "li",
    "ol",
    "p",
    "pre",
    "q",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "u",
    "ul",
];

/// The block tags of jsoup (separated by a space in the text).
const BLOCK_TAGS: [&str; 53] = [
    "html",
    "head",
    "body",
    "frameset",
    "script",
    "noscript",
    "style",
    "meta",
    "link",
    "title",
    "frame",
    "noframes",
    "section",
    "nav",
    "aside",
    "hgroup",
    "header",
    "footer",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "pre",
    "div",
    "blockquote",
    "hr",
    "address",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "ins",
    "del",
    "dl",
    "dt",
    "dd",
    "li",
    "table",
    "caption",
    "thead",
    "tfoot",
    "tbody",
    "colgroup",
    "col",
    "tr",
    "th",
    "td",
    "br",
];

/// Keeps the HTML of jsoup `Safelist.basic()` (Java `Jsoup.clean`): the basic text tags, the
/// `href` of the links (http, https, ftp, mailto, with `rel="nofollow"`) and the `cite` of
/// quotes; the other tags are unwrapped and the scripts and styles removed.
pub fn clean(html: &str) -> String {
    let tags: HashSet<&str> = BASIC_TAGS.into_iter().collect();
    let mut attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    attributes.insert("a", ["href"].into_iter().collect());
    attributes.insert("blockquote", ["cite"].into_iter().collect());
    attributes.insert("q", ["cite"].into_iter().collect());
    ammonia::Builder::default()
        .tags(tags)
        .tag_attributes(attributes)
        .generic_attributes(HashSet::new())
        .url_schemes(["http", "https", "ftp", "mailto"].into_iter().collect())
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("nofollow"))
        .clean(html)
        .to_string()
}

/// Appends the text of the element as jsoup does: whitespace collapsed, a space around the
/// block elements, without the scripts and styles.
fn append_text(element: ElementRef<'_>, out: &mut String) {
    for child in element.children() {
        match child.value() {
            Node::Text(t) => {
                for c in t.chars() {
                    if c.is_whitespace() {
                        if !out.is_empty() && !out.ends_with(' ') {
                            out.push(' ');
                        }
                    } else {
                        out.push(c);
                    }
                }
            }
            Node::Element(e) => {
                let name = e.name();
                if matches!(name, "script" | "style") {
                    continue;
                }
                let block = BLOCK_TAGS.contains(&name);
                if block && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                if let Some(el) = ElementRef::wrap(child) {
                    append_text(el, out);
                }
                if block && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            _ => {}
        }
    }
}

fn element_text(element: ElementRef<'_>) -> String {
    let mut out = String::new();
    append_text(element, &mut out);
    out.trim().to_string()
}

/// The text of the HTML document (Java `Jsoup.parse(html).text()`).
pub fn text(html: &str) -> String {
    HtmlDocument::parse(html).text()
}

/// A parsed HTML document (Java jsoup `Document`): the `title`, `text`, `html` and `body`
/// ([`HtmlElement`]) properties for the simple language.
#[derive(Debug, Clone)]
pub struct HtmlDocument {
    title: String,
    text: String,
    html: String,
    body: HtmlElement,
}

/// An element of a document (Java jsoup `Element`): its `text` and `html`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlElement {
    text: String,
    html: String,
}

impl HtmlElement {
    /// The text of the element.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The inner HTML of the element.
    pub fn html(&self) -> &str {
        &self.html
    }
}

impl HtmlDocument {
    /// Parses the HTML (Java `Jsoup.parse`).
    pub fn parse(html: &str) -> Self {
        let doc = Html::parse_document(html);
        let title_selector = Selector::parse("title").expect("a valid selector");
        let title = doc
            .select(&title_selector)
            .next()
            .map(|t| {
                t.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let body_selector = Selector::parse("body").expect("a valid selector");
        let body = doc
            .select(&body_selector)
            .next()
            .map(|b| HtmlElement {
                text: element_text(b),
                html: b.inner_html(),
            })
            .unwrap_or(HtmlElement {
                text: String::new(),
                html: String::new(),
            });
        HtmlDocument {
            title,
            text: element_text(doc.root_element()),
            html: doc.root_element().html(),
            body,
        }
    }

    /// The title of the document.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The text of the document.
    pub fn text(&self) -> String {
        self.text.clone()
    }

    /// The body of the document.
    pub fn body(&self) -> &HtmlElement {
        &self.body
    }
}

impl std::fmt::Display for HtmlDocument {
    /// The HTML of the document (Java `Document.toString`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.html)
    }
}

impl Bean for HtmlDocument {
    fn property(&self, name: &str) -> Option<Value> {
        Some(match name {
            "title" => Value::from(self.title.as_str()),
            "text" => Value::from(self.text.as_str()),
            "html" | "outerHtml" => Value::from(self.html.as_str()),
            "body" => Value::bean(self.body.clone()),
            _ => return None,
        })
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        if args.is_empty() {
            self.property(method).map(Ok)
        } else {
            None
        }
    }
}

impl Bean for HtmlElement {
    fn property(&self, name: &str) -> Option<Value> {
        Some(match name {
            "text" => Value::from(self.text.as_str()),
            "html" => Value::from(self.html.as_str()),
            _ => return None,
        })
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        if args.is_empty() {
            self.property(method).map(Ok)
        } else {
            None
        }
    }
}

/// The text of the input (`None` for null).
fn input_text(input: &Value) -> Option<String> {
    match input {
        Value::Null => None,
        Value::Text(t) => Some(t.clone()),
        Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        other => Some(other.to_string()),
    }
}

/// The `${htmlClean()}` simple function.
#[derive(Debug, Default)]
pub struct HtmlCleanFunction;

impl SimpleFunction for HtmlCleanFunction {
    fn name(&self) -> &str {
        "htmlClean"
    }

    fn apply(&self, _exchange: &Exchange, input: Value) -> CamelResult<Value> {
        Ok(input_text(&input).map_or(Value::Null, |t| Value::from(clean(&t))))
    }
}

/// The `${htmlParse()}` simple function: an [`HtmlDocument`] bean.
#[derive(Debug, Default)]
pub struct HtmlParseFunction;

impl SimpleFunction for HtmlParseFunction {
    fn name(&self) -> &str {
        "htmlParse"
    }

    fn apply(&self, _exchange: &Exchange, input: Value) -> CamelResult<Value> {
        Ok(input_text(&input).map_or(Value::Null, |t| Value::bean(HtmlDocument::parse(&t))))
    }
}

/// The `${htmlDecode()}` simple function: the text of the HTML.
#[derive(Debug, Default)]
pub struct HtmlDecodeFunction;

impl SimpleFunction for HtmlDecodeFunction {
    fn name(&self) -> &str {
        "htmlDecode"
    }

    fn apply(&self, _exchange: &Exchange, input: Value) -> CamelResult<Value> {
        Ok(input_text(&input).map_or(Value::Null, |t| Value::from(text(&t))))
    }
}

/// Registers the simple functions on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    if let Some(r) = camel_api::context::context_plugin::<SimpleFunctionRegistry>(context) {
        r.add_function(Arc::new(HtmlCleanFunction));
        r.add_function(Arc::new(HtmlParseFunction));
        r.add_function(Arc::new(HtmlDecodeFunction));
    }
}
