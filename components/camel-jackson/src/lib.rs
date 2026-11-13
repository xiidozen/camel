//! # camel-jackson
//!
//! The `jackson` data format (Java `JacksonDataFormat`, also the default `json` data format):
//! `marshal` writes the body as JSON (maps as objects, lists as arrays; a text that already is
//! a JSON object or array, and bytes, are written as is), `unmarshal` reads JSON into maps,
//! lists and simple values. Rust has no Java classes, so the unmarshal types (`unmarshalType`,
//! the `CamelJacksonUnmarshalType` header) and the Jackson modules have no equivalent. This
//! crate is the Rust port of the Java `components/camel-jackson` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `prettyPrint` | `false` | To enable pretty printing output nicely formatted. |
//! | `contentTypeHeader` | `true` | Whether the data format should set the Content-Type header with the type from the data format. |
//! | `include` | | If you want to marshal a pojo to JSON, and the pojo has some fields with null values. And you want to skip these null values, you can set this option to `NON_NULL`. |
//! | `useList` | `false` | To unmarshal to a List of Map or a List of Pojo. |
//! | `allowUnmarshallType` | `false` | If enabled then Jackson is allowed to attempt to use the CamelJacksonUnmarshalType header during the unmarshalling (accepted). |
//! | `library` | `Jackson` | The JSON library of the `json` data format (only Jackson). |

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::CONTENT_TYPE;
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "jackson";

/// The content type of the marshalled JSON.
pub const CONTENT_TYPE_JSON: &str = "application/json";

/// The `jackson` data format.
#[derive(Debug, Clone)]
pub struct JacksonDataFormat {
    name: String,
    pretty_print: bool,
    content_type_header: bool,
    include_non_null: bool,
    use_list: bool,
}

impl Default for JacksonDataFormat {
    fn default() -> Self {
        JacksonDataFormat {
            name: NAME.to_string(),
            pretty_print: false,
            content_type_header: true,
            include_non_null: false,
            use_list: false,
        }
    }
}

/// Writes the value as Jackson's `DefaultPrettyPrinter` does: an object per line indented by
/// two spaces with ` : ` separators, arrays on one line (`[ 1, 2 ]`), empty ones as `{ }` and
/// `[ ]`.
pub fn jackson_pretty(value: &Value) -> String {
    fn write(v: &Value, indent: usize, out: &mut String) {
        match v {
            Value::Map(m) if m.is_empty() => out.push_str("{ }"),
            Value::Map(m) => {
                out.push('{');
                for (i, (k, val)) in m.iter().enumerate() {
                    out.push_str(if i == 0 { "\n" } else { ",\n" });
                    out.push_str(&" ".repeat(indent + 2));
                    out.push_str(&serde_json::Value::String(k.clone()).to_string());
                    out.push_str(" : ");
                    write(val, indent + 2, out);
                }
                out.push('\n');
                out.push_str(&" ".repeat(indent));
                out.push('}');
            }
            Value::List(l) if l.is_empty() => out.push_str("[ ]"),
            Value::List(l) => {
                out.push_str("[ ");
                for (i, item) in l.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write(item, indent, out);
                }
                out.push_str(" ]");
            }
            other => out.push_str(&camel_support::json::stringify(other, false)),
        }
    }
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

impl JacksonDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// With pretty printed output.
    pub fn with_pretty_print(mut self, pretty: bool) -> Self {
        self.pretty_print = pretty;
        self
    }

    /// With the `Content-Type` header set when marshalling (default `true`).
    pub fn with_content_type_header(mut self, header: bool) -> Self {
        self.content_type_header = header;
        self
    }

    /// With the null values of the maps skipped (Jackson `Include.NON_NULL`).
    pub fn with_include_non_null(mut self, non_null: bool) -> Self {
        self.include_non_null = non_null;
        self
    }

    /// With the unmarshalled JSON always a list.
    pub fn with_use_list(mut self, use_list: bool) -> Self {
        self.use_list = use_list;
        self
    }

    /// The data format of the options.
    ///
    /// # Errors
    ///
    /// An unknown option or an invalid value.
    pub fn of_options(name: &str, options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = JacksonDataFormat {
            name: name.to_string(),
            ..Self::default()
        };
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true");
        for (k, v) in options {
            match k.as_str() {
                "prettyPrint" => df.pretty_print = flag(v),
                "contentTypeHeader" => df.content_type_header = flag(v),
                "include" => df.include_non_null = v.trim().eq_ignore_ascii_case("NON_NULL"),
                "useList" => df.use_list = flag(v),
                "allowUnmarshallType" | "allowJmsType" => {}
                "library" if v.trim().eq_ignore_ascii_case("jackson") => {}
                "unmarshalType" | "unmarshalTypeName"
                    if matches!(
                        v.trim(),
                        "java.util.Map"
                            | "java.util.HashMap"
                            | "java.util.LinkedHashMap"
                            | "java.util.List"
                            | "java.util.ArrayList"
                            | "java.lang.Object"
                            | "Map"
                            | "List"
                    ) => {}
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unsupported option {k}={v} of the {name} data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for JacksonDataFormat {
    fn name(&self) -> &str {
        &self.name
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        // the JSON text and bytes as is (Java JsonPayloadHelper.writeIfAlreadyJson)
        match graph {
            g if camel_support::json::write_if_already_json(g, out) => {}
            other => {
                let v = if self.include_non_null {
                    camel_support::json::without_null_entries(other)
                } else {
                    other.clone()
                };
                let text = if self.pretty_print {
                    jackson_pretty(&v)
                } else {
                    camel_support::json::stringify(&v, false)
                };
                out.extend(text.into_bytes());
            }
        }
        if self.content_type_header {
            exchange
                .message_mut()
                .set_header(CONTENT_TYPE, CONTENT_TYPE_JSON);
        }
        Ok(())
    }

    fn unmarshal(&self, _exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let v = camel_support::json::parse_bytes(&input_bytes(body)?)?;
        Ok(match v {
            Value::List(_) => v,
            other if self.use_list => Value::List(vec![other]),
            other => other,
        })
    }
}

/// The factory of the data format of the name (`jackson` or `json`).
pub fn factory(name: &'static str) -> DataFormatFactory {
    Arc::new(move |options| {
        Ok(Arc::new(JacksonDataFormat::of_options(name, options)?) as SharedDataFormat)
    })
}

/// Registers the `jackson` and `json` data formats on the context.
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory(NAME));
    context.register_data_format_factory("json", factory("json"));
}
