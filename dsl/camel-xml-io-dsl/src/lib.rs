//! The XML DSL (port of `dsl/camel-xml-io-dsl`): routes written in XML, in the
//! `http://camel.apache.org/schema/xml-io` namespace (or without namespace).
//!
//! ```xml
//! <routes xmlns="http://camel.apache.org/schema/xml-io">
//!   <route id="bar">
//!     <from uri="direct:bar"/>
//!     <filter>
//!       <simple>${body} contains 'World'</simple>
//!       <to uri="mock:filtered"/>
//!     </filter>
//!     <to uri="mock:bar"/>
//!   </route>
//! </routes>
//! ```
//!
//! The XML elements are converted to the tree of the YAML DSL and created by it
//! ([`camel_yaml_dsl`]): the attributes are the options, a language element (`<simple>`,
//! `<constant>`, ...) is the expression, the option elements (`<when>`, `<otherwise>`,
//! `<doCatch>`, `<exception>`, `<onWhen>`, `<handled>`, ...) are options, and the other child
//! elements are the nested steps. The root is `<routes>` (or `<camel>`) with `<route>`,
//! `<onException>`, `<intercept>`, `<interceptFrom>`, `<interceptSendToEndpoint>`,
//! `<errorHandler>` elements, or a single `<route>`.
//!
//! # Examples
//!
//! ```
//! use camel_core::prelude::*;
//! use camel_xml_io_dsl::XmlRoutes;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let routes = XmlRoutes::parse(r#"
//! <routes xmlns="http://camel.apache.org/schema/xml-io">
//!   <route id="bar">
//!     <from uri="direct:bar"/>
//!     <setBody><simple>Bye ${body}</simple></setBody>
//!     <to uri="mock:bar"/>
//!   </route>
//! </routes>"#).unwrap();
//! let context = camel_core::default_context();
//! context.add_routes(routes).await.unwrap();
//! context.start().await.unwrap();
//! let mock = MockEndpoint::resolve(context.as_ref(), "mock:bar").unwrap();
//! mock.expected_bodies_received(["Bye World"]);
//! context.create_producer_template().send_body("direct:bar", "World").await.unwrap();
//! mock.assert_is_satisfied().await;
//! context.stop().await.unwrap();
//! # });
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult};
use camel_core_model::builder::{RouteBuilder, Routes};
use camel_yaml_dsl::{Yaml, YamlRoutes, LANGUAGES, OUTPUT_EIPS};
use roxmltree::Node;
use serde_yaml::Mapping;

/// The option elements of the EIPs: their content is an option (an expression or nested
/// options), not a nested step. `when` and `doCatch` are lists.
const OPTION_ELEMENTS: &[&str] = &[
    "when",
    "otherwise",
    "doCatch",
    "doFinally",
    "exception",
    "onWhen",
    "correlationExpression",
    "completionPredicate",
    "completionSizeExpression",
    "completionTimeoutExpression",
    "handled",
    "continued",
    "retryWhile",
    "redeliveryPolicy",
    "batchConfig",
    "streamConfig",
    "roundRobin",
    "random",
    "topic",
    "sticky",
    "failover",
    "weighted",
    "deadLetterChannel",
    "defaultErrorHandler",
    "noErrorHandler",
    "expression",
];

/// The list options of the EIPs.
const LIST_OPTIONS: &[&str] = &["when", "doCatch", "exception"];

fn error(message: impl Into<String>) -> CamelError {
    CamelError::illegal_argument(message.into())
}

fn key(s: &str) -> Yaml {
    Yaml::String(s.to_string())
}

/// The trimmed text content of the element.
fn text_of(node: Node<'_, '_>) -> String {
    node.children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .collect::<String>()
        .trim()
        .to_string()
}

fn elements<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(|c| c.is_element())
}

fn attributes(node: Node<'_, '_>, map: &mut Mapping) {
    for a in node.attributes() {
        map.insert(key(a.name()), Yaml::String(a.value().to_string()));
    }
}

/// A language element: its text, or a map of its text (`expression`) and attributes.
fn language(node: Node<'_, '_>) -> Yaml {
    let text = text_of(node);
    if node.attributes().len() == 0 {
        return Yaml::String(text);
    }
    let mut map = Mapping::new();
    map.insert(key("expression"), Yaml::String(text));
    attributes(node, &mut map);
    Yaml::Mapping(map)
}

/// The options and the nested steps of an EIP element.
fn eip(node: Node<'_, '_>) -> CamelResult<Yaml> {
    let mut map = Mapping::new();
    attributes(node, &mut map);
    let mut steps = Vec::new();
    for c in elements(node) {
        let name = c.tag_name().name();
        if matches!(node.tag_name().name(), "marshal" | "unmarshal") {
            // the data format element: its attributes are its options
            let mut options = Mapping::new();
            attributes(c, &mut options);
            map.insert(key(name), Yaml::Mapping(options));
        } else if LANGUAGES.contains(&name) {
            map.insert(key(name), language(c));
        } else if name == "exception" {
            push_list(&mut map, name, Yaml::String(text_of(c)));
        } else if (name == "setHeader" && node.tag_name().name() == "setHeaders")
            || (name == "setVariable" && node.tag_name().name() == "setVariables")
        {
            let list = if name == "setHeader" {
                "headers"
            } else {
                "variables"
            };
            push_list(&mut map, list, eip(c)?);
        } else if OPTION_ELEMENTS.contains(&name) {
            let value = eip(c)?;
            if LIST_OPTIONS.contains(&name) {
                push_list(&mut map, name, value);
            } else {
                map.insert(key(name), value);
            }
        } else {
            steps.push(step(c)?);
        }
    }
    // the nesting is explicit in XML: an EIP with outputs has its (possibly empty) steps, so
    // it does not take the following steps as the YAML DSL does
    if !steps.is_empty() || OUTPUT_EIPS.contains(&node.tag_name().name()) {
        map.insert(key("steps"), Yaml::Sequence(steps));
    }
    if map.is_empty() {
        let t = text_of(node);
        if !t.is_empty() {
            return Ok(Yaml::String(t));
        }
    }
    Ok(Yaml::Mapping(map))
}

fn push_list(map: &mut Mapping, name: &str, value: Yaml) {
    match map.get_mut(key(name)) {
        Some(Yaml::Sequence(l)) => l.push(value),
        _ => {
            map.insert(key(name), Yaml::Sequence(vec![value]));
        }
    }
}

/// A step: `{name: options}`.
fn step(node: Node<'_, '_>) -> CamelResult<Yaml> {
    let mut m = Mapping::new();
    m.insert(key(node.tag_name().name()), eip(node)?);
    Ok(Yaml::Mapping(m))
}

/// A `<route>` element as a YAML `route` (the steps after `<from>` are the steps of `from`).
fn route(node: Node<'_, '_>) -> CamelResult<Yaml> {
    let mut route = Mapping::new();
    attributes(node, &mut route);
    let mut from: Option<Mapping> = None;
    let mut steps = Vec::new();
    for c in elements(node) {
        match c.tag_name().name() {
            "from" => {
                let mut f = Mapping::new();
                attributes(c, &mut f);
                from = Some(f);
            }
            "description" => {
                route.insert(key("description"), Yaml::String(text_of(c)));
            }
            "errorHandler" => {
                route.insert(key("errorHandler"), eip(c)?);
            }
            _ => steps.push(step(c)?),
        }
    }
    let mut from = from.ok_or_else(|| error("route: the from element is required"))?;
    from.insert(key("steps"), Yaml::Sequence(steps));
    route.insert(key("from"), Yaml::Mapping(from));
    let mut m = Mapping::new();
    m.insert(key("route"), Yaml::Mapping(route));
    Ok(Yaml::Mapping(m))
}

/// The routes of an XML document (a [`RouteBuilder`]).
pub struct XmlRoutes {
    routes: YamlRoutes,
}

impl XmlRoutes {
    /// Parses the XML document.
    ///
    /// # Errors
    ///
    /// Invalid XML, an unknown element or an invalid option (an `IllegalArgumentException`).
    pub fn parse(xml: &str) -> CamelResult<Self> {
        let doc =
            roxmltree::Document::parse(xml).map_err(|e| error(format!("Invalid XML: {e}")))?;
        let root = doc.root_element();
        let mut items = Vec::new();
        match root.tag_name().name() {
            "route" => items.push(route(root)?),
            "routes" | "camel" => {
                for c in elements(root) {
                    match c.tag_name().name() {
                        "route" => items.push(route(c)?),
                        "routes" => {
                            for r in elements(c).filter(|r| r.tag_name().name() == "route") {
                                items.push(route(r)?);
                            }
                        }
                        "dataFormats" => {
                            let mut formats = Vec::new();
                            for f in elements(c) {
                                let mut options = Mapping::new();
                                attributes(f, &mut options);
                                let mut m = Mapping::new();
                                m.insert(key(f.tag_name().name()), Yaml::Mapping(options));
                                formats.push(Yaml::Mapping(m));
                            }
                            let mut m = Mapping::new();
                            m.insert(key("dataFormats"), Yaml::Sequence(formats));
                            items.push(Yaml::Mapping(m));
                        }
                        "onException"
                        | "intercept"
                        | "interceptFrom"
                        | "interceptSendToEndpoint"
                        | "errorHandler" => items.push(step(c)?),
                        other => {
                            return Err(error(format!(
                                "Unknown or not yet supported XML DSL element: {other}"
                            )))
                        }
                    }
                }
            }
            other => {
                return Err(error(format!(
                    "Unknown or not yet supported XML DSL root element: {other}"
                )))
            }
        }
        Ok(XmlRoutes {
            routes: YamlRoutes::from_value(Yaml::Sequence(items))?,
        })
    }

    /// The routes as the YAML DSL routes they were converted to.
    pub fn yaml_routes(&self) -> &YamlRoutes {
        &self.routes
    }
}

impl RouteBuilder for XmlRoutes {
    fn configure(&self, routes: &mut Routes) {
        self.routes.configure(routes);
    }
}
