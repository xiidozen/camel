//! # camel-gson
//!
//! The Gson data format (Java `GsonDataFormat`): `marshal` writes the body as JSON the way
//! Gson does (a body that already is JSON text or bytes as it is), `unmarshal` reads JSON
//! leniently into maps, lists and values, the numbers as doubles. It is also the `json` data
//! format of the `Gson` library. The [`json`](mod@json) module is the reader and writer. This
//! crate is the Rust port of the Java `components/camel-gson` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `unmarshalType` | | Class name of the java type to use when unmarshalling (`java.lang.Object`, the map, list and `java.lang.String` types in Rust). |
//! | `prettyPrint` | `false` | To enable pretty printing output nicely formatted. |
//! | `serializeNulls` | `false` | To enable serialization of null values (the null map entries are left out otherwise). |
//! | `longSerializationPolicy` | | The policy of the long values (`DEFAULT`, `STRING` to write them as JSON strings). |
//! | `dateFormatPattern` | | To configure the date format while serialize or deserialize (no dates among the Rust values). |
//! | `fieldNamingPolicy` | | To use a specific field naming policy (the map keys are kept, as Gson). |
//! | `contentTypeHeader` | `true` | Whether the data format should set the Content-Type header with the type from the data format. |
//!
//! The `CamelGsonUnmarshalType` header overrides the unmarshal type.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod json;

use crate::json::{from_json, to_json, WriteOptions};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{CHARSET_NAME, CONTENT_TYPE};
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::charset::Charset;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "gson";

/// The header with the unmarshal type (Java `GsonConstants.UNMARSHAL_TYPE`).
pub const UNMARSHAL_TYPE: &str = "CamelGsonUnmarshalType";

/// The kind of value an unmarshal type asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnmarshalType {
    /// Any value (`java.lang.Object`).
    #[default]
    Object,
    /// An object (`java.util.Map` and its implementations).
    Map,
    /// An array (`java.util.List` and its implementations).
    List,
    /// A string (`java.lang.String`).
    String,
}

impl UnmarshalType {
    /// The kind of a Java class name.
    ///
    /// # Errors
    ///
    /// A class that has no Rust equivalent.
    pub fn of_class(name: &str) -> CamelResult<Self> {
        Ok(match name.trim() {
            "java.lang.Object" | "Object" => UnmarshalType::Object,
            "java.util.Map"
            | "java.util.HashMap"
            | "java.util.LinkedHashMap"
            | "java.util.TreeMap"
            | "Map" => UnmarshalType::Map,
            "java.util.List"
            | "java.util.ArrayList"
            | "java.util.LinkedList"
            | "java.util.Collection"
            | "List" => UnmarshalType::List,
            "java.lang.String" | "String" => UnmarshalType::String,
            other => {
                return Err(CamelError::illegal_argument(format!(
                    "Cannot find class: {other}"
                )))
            }
        })
    }

    fn check(self, v: Value) -> CamelResult<Value> {
        let (ok, expected) = match self {
            UnmarshalType::Object => (true, ""),
            UnmarshalType::Map => (matches!(v, Value::Map(_) | Value::Null), "BEGIN_OBJECT"),
            UnmarshalType::List => (matches!(v, Value::List(_) | Value::Null), "BEGIN_ARRAY"),
            UnmarshalType::String => return Ok(match v {
                Value::Map(_) | Value::List(_) => return Err(CamelError::illegal_argument(
                    "com.google.gson.JsonSyntaxException: Expected a string but was BEGIN_OBJECT",
                )),
                Value::Null => Value::Null,
                Value::Float(f) => Value::from(camel_util::value::java_double_to_string(f)),
                other => Value::from(other.to_string()),
            }),
        };
        if ok {
            Ok(v)
        } else {
            Err(CamelError::illegal_argument(format!(
                "com.google.gson.JsonSyntaxException: Expected {expected} but was {}",
                match v {
                    Value::Map(_) => "BEGIN_OBJECT",
                    Value::List(_) => "BEGIN_ARRAY",
                    Value::Text(_) => "STRING",
                    Value::Bool(_) => "BOOLEAN",
                    _ => "NUMBER",
                }
            )))
        }
    }
}

/// The Gson data format.
#[derive(Debug, Clone)]
pub struct GsonDataFormat {
    unmarshal_type: UnmarshalType,
    serialize_nulls: bool,
    pretty_print: bool,
    long_as_string: bool,
    date_format_pattern: Option<String>,
    field_naming_policy: Option<String>,
    content_type_header: bool,
}

impl Default for GsonDataFormat {
    fn default() -> Self {
        GsonDataFormat {
            unmarshal_type: UnmarshalType::Object,
            serialize_nulls: false,
            pretty_print: false,
            long_as_string: false,
            date_format_pattern: None,
            field_naming_policy: None,
            content_type_header: true,
        }
    }
}

fn charset_of(exchange: &Exchange) -> CamelResult<Charset> {
    match exchange.property(CHARSET_NAME) {
        Some(v) if !v.is_null() => Charset::for_name(&v.to_string())
            .map_err(|e| CamelError::illegal_argument(e.to_string())),
        _ => Ok(Charset::Utf8),
    }
}

/// The value with its integers as strings (Java `LongSerializationPolicy.STRING`).
fn longs_as_strings(v: &Value) -> Value {
    match v {
        Value::Int(i) => Value::from(i.to_string()),
        Value::List(l) => Value::List(l.iter().map(longs_as_strings).collect()),
        Value::Map(m) => Value::Map(
            m.iter()
                .map(|(k, v)| (k.clone(), longs_as_strings(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

impl GsonDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// With the kind of the unmarshalled values.
    pub fn with_unmarshal_type(mut self, unmarshal_type: UnmarshalType) -> Self {
        self.unmarshal_type = unmarshal_type;
        self
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

    /// Whether marshalling sets the `Content-Type` header to `application/json`.
    pub fn with_content_type_header(mut self, content_type_header: bool) -> Self {
        self.content_type_header = content_type_header;
        self
    }

    /// The date format pattern.
    pub fn date_format_pattern(&self) -> Option<&str> {
        self.date_format_pattern.as_deref()
    }

    /// The field naming policy.
    pub fn field_naming_policy(&self) -> Option<&str> {
        self.field_naming_policy.as_deref()
    }

    /// The data format of the options (see the options of the crate).
    ///
    /// # Errors
    ///
    /// An unknown option or an invalid value.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true");
        for (k, v) in options {
            match k.as_str() {
                "unmarshalType" | "unmarshalTypeName" => {
                    df.unmarshal_type = UnmarshalType::of_class(v)?
                }
                "serializeNulls" => df.serialize_nulls = flag(v),
                "prettyPrint" => df.pretty_print = flag(v),
                "contentTypeHeader" => df.content_type_header = flag(v),
                "dateFormatPattern" => df.date_format_pattern = Some(v.clone()),
                "fieldNamingPolicy" => df.field_naming_policy = Some(v.clone()),
                "longSerializationPolicy" => {
                    df.long_as_string = match v.trim() {
                        "STRING" => true,
                        "DEFAULT" => false,
                        other => {
                            return Err(CamelError::illegal_argument(format!(
                                "No enum constant com.google.gson.LongSerializationPolicy.{other}"
                            )))
                        }
                    }
                }
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the gson data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for GsonDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        // a body that already is the JSON text is written as it is (CAMEL-25329)
        if !camel_support::json::write_if_already_json(graph, out) {
            let options = WriteOptions {
                serialize_nulls: self.serialize_nulls,
                pretty_print: self.pretty_print,
            };
            let text = if self.long_as_string {
                to_json(&longs_as_strings(graph), &options)?
            } else {
                to_json(graph, &options)?
            };
            out.extend(charset_of(exchange)?.encode(&text));
        }
        if self.content_type_header {
            exchange
                .message_mut()
                .set_header(CONTENT_TYPE, "application/json");
        }
        Ok(())
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let kind = match exchange.message().header(UNMARSHAL_TYPE) {
            Some(t) if !t.is_null() => UnmarshalType::of_class(&t.to_string())?,
            _ => self.unmarshal_type,
        };
        let text = match body {
            Value::Text(t) => t.clone(),
            other => String::from_utf8_lossy(&input_bytes(other)?).into_owned(),
        };
        kind.check(from_json(&text)?)
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(GsonDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
