//! # camel-fastjson
//!
//! The Fastjson data format (Java `FastjsonDataFormat`): `marshal` writes the body as JSON
//! (a body that already is JSON text or bytes as it is; the null map entries left out unless
//! `serializeNulls`) and sets the `Content-Type` and `Content-Length` headers, `unmarshal`
//! reads JSON into maps, lists and values. It is also the `json` data format of the
//! `Fastjson` library. This crate is the Rust port of the Java `components/camel-fastjson`
//! module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `unmarshalType` | | Class name of the java type to use when unmarshalling (`java.lang.Object`, the map and list types in Rust). |
//! | `prettyPrint` | `false` | To enable pretty printing output nicely formatted (a tab per level, as Fastjson). |
//! | `serializeNulls` | `false` | To enable serialization of null values. |
//! | `dateFormatPattern` | | To configure the date format while serialize or deserialize (no dates among the Rust values). |
//! | `contentTypeHeader` | `true` | Whether the data format should set the Content-Type header with the type from the data format. |
//!
//! # Examples
//!
//! ```
//! use camel_fastjson::pretty;
//! use camel_util::value::{Value, ValueMap};
//!
//! let mut m = ValueMap::new();
//! m.insert("name".into(), Value::from("Camel"));
//! m.insert("tags".into(), Value::List(vec![Value::Int(1)]));
//! assert_eq!("{\n\t\"name\":\"Camel\",\n\t\"tags\":[\n\t\t1\n\t]\n}", pretty(&Value::Map(m)));
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{CONTENT_LENGTH, CONTENT_TYPE};
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_support::json::stringify;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "fastjson";

/// Writes the value as the Fastjson `PrettyFormat` feature does: a tab per level, no space
/// after the colons, empty objects and arrays as `{}` and `[]`.
pub fn pretty(value: &Value) -> String {
    fn write(v: &Value, depth: usize, out: &mut String) {
        let newline = |out: &mut String, depth: usize| {
            out.push('\n');
            out.push_str(&"\t".repeat(depth));
        };
        match v {
            Value::Map(m) if !m.is_empty() => {
                out.push('{');
                for (i, (k, item)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    out.push_str(&stringify(&Value::from(k.as_str()), false));
                    out.push(':');
                    write(item, depth + 1, out);
                }
                newline(out, depth);
                out.push('}');
            }
            Value::List(l) if !l.is_empty() => {
                out.push('[');
                for (i, item) in l.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    write(item, depth + 1, out);
                }
                newline(out, depth);
                out.push(']');
            }
            other => out.push_str(&stringify(other, false)),
        }
    }
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

/// The Fastjson data format.
#[derive(Debug, Clone)]
pub struct FastjsonDataFormat {
    unmarshal_type: Option<String>,
    serialize_nulls: bool,
    pretty_print: bool,
    date_format_pattern: Option<String>,
    content_type_header: bool,
}

impl Default for FastjsonDataFormat {
    fn default() -> Self {
        FastjsonDataFormat {
            unmarshal_type: None,
            serialize_nulls: false,
            pretty_print: false,
            date_format_pattern: None,
            content_type_header: true,
        }
    }
}

impl FastjsonDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the null map entries are written.
    pub fn with_serialize_nulls(mut self, serialize_nulls: bool) -> Self {
        self.serialize_nulls = serialize_nulls;
        self
    }

    /// Whether the JSON is pretty printed.
    pub fn with_pretty_print(mut self, pretty_print: bool) -> Self {
        self.pretty_print = pretty_print;
        self
    }

    /// Whether marshalling sets the `Content-Type` and `Content-Length` headers.
    pub fn with_content_type_header(mut self, content_type_header: bool) -> Self {
        self.content_type_header = content_type_header;
        self
    }

    /// The date format pattern.
    pub fn date_format_pattern(&self) -> Option<&str> {
        self.date_format_pattern.as_deref()
    }

    /// The JSON text of a value.
    pub fn to_json(&self, value: &Value) -> String {
        let v = if self.serialize_nulls {
            value.clone()
        } else {
            camel_support::json::without_null_entries(value)
        };
        if self.pretty_print {
            pretty(&v)
        } else {
            stringify(&v, false)
        }
    }

    /// The data format of the options (see the options of the crate).
    ///
    /// # Errors
    ///
    /// An unknown option or an unknown unmarshal type.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true");
        for (k, v) in options {
            match k.as_str() {
                "unmarshalType" | "unmarshalTypeName" => {
                    match v.trim() {
                        "java.lang.Object"
                        | "java.util.Map"
                        | "java.util.HashMap"
                        | "java.util.LinkedHashMap"
                        | "java.util.List"
                        | "java.util.ArrayList" => {}
                        other => {
                            return Err(CamelError::illegal_argument(format!(
                                "Cannot find class: {other}"
                            )))
                        }
                    }
                    df.unmarshal_type = Some(v.trim().to_string());
                }
                "serializeNulls" => df.serialize_nulls = flag(v),
                "prettyPrint" => df.pretty_print = flag(v),
                "dateFormatPattern" => df.date_format_pattern = Some(v.clone()),
                "contentTypeHeader" => df.content_type_header = flag(v),
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the fastjson data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for FastjsonDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        let before = out.len();
        if !camel_support::json::write_if_already_json(graph, out) {
            out.extend(self.to_json(graph).into_bytes());
        }
        if self.content_type_header {
            let len = (out.len() - before) as i64;
            let m = exchange.message_mut();
            m.set_header(CONTENT_TYPE, "application/json");
            m.set_header(CONTENT_LENGTH, Value::Int(len));
        }
        Ok(())
    }

    fn unmarshal(&self, _exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let v = match body {
            Value::Text(t) => camel_support::json::parse(t)?,
            other => camel_support::json::parse_bytes(&input_bytes(other)?)?,
        };
        match self.unmarshal_type.as_deref() {
            Some(t) if t.contains("Map") && !matches!(v, Value::Map(_)) => {
                Err(CamelError::illegal_argument(format!(
                    "com.alibaba.fastjson.JSONException: can not cast to {t}, value : {v}"
                )))
            }
            Some(t) if (t.contains("List")) && !matches!(v, Value::List(_)) => {
                Err(CamelError::illegal_argument(format!(
                    "com.alibaba.fastjson.JSONException: can not cast to {t}, value : {v}"
                )))
            }
            _ => Ok(v),
        }
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(FastjsonDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
