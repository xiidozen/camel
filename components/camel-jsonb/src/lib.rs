//! # camel-jsonb
//!
//! The JSON-B data format (Java `JsonbDataFormat`, on Apache Johnzon): `marshal` writes the
//! body as JSON (a body that already is JSON text or bytes as it is; the null map entries
//! left out with `skipNull`, the default), `unmarshal` reads JSON into maps, lists and
//! values. It is also the `json` data format of the `Jsonb` library. This crate is the Rust
//! port of the Java `components/camel-jsonb` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `unmarshalType` | | Class name of the java type to use when unmarshalling (`java.lang.Object`, the map, list and `java.lang.String` types in Rust). |
//! | `prettyPrint` | `false` | To enable pretty printing output nicely formatted. |
//! | `encoding` | `UTF-8` | The encoding of the JSON. |
//! | `binaryStrategy` | `BASE_64` | The strategy of the binary data (the bytes are written as they are in Rust). |
//! | `propertyOrder` | `ANY` | The order of the properties: `LEXICOGRAPHICAL` sorts the keys of the maps, `REVERSE` the other way round. |
//! | `propertyNamingStrategy` | `IDENTITY` | The naming of the properties (the map keys are kept). |
//! | `skipNull` | `true` | Do not include null values. |
//!
//! The `CamelJsonbUnmarshallType` header overrides the unmarshal type.
//!
//! # Examples
//!
//! ```
//! use camel_jsonb::JsonbDataFormat;
//! use camel_util::value::{Value, ValueMap};
//!
//! let mut m = ValueMap::new();
//! m.insert("b".into(), Value::Int(1));
//! m.insert("a".into(), Value::Null);
//! assert_eq!("{\"b\":1}", JsonbDataFormat::new().to_json(&Value::Map(m)));
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::charset::Charset;
use camel_util::value::{Value, ValueMap};
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "jsonb";

/// The header with the unmarshal type.
pub const UNMARSHAL_TYPE: &str = "CamelJsonbUnmarshallType";

/// The order of the properties (Java `PropertyOrderStrategy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PropertyOrder {
    /// The order of the maps (`ANY`).
    #[default]
    Any,
    /// The keys sorted (`LEXICOGRAPHICAL`).
    Lexicographical,
    /// The keys sorted the other way round (`REVERSE`).
    Reverse,
}

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
            "java.lang.Object" => UnmarshalType::Object,
            "java.util.Map" | "java.util.HashMap" | "java.util.LinkedHashMap" => UnmarshalType::Map,
            "java.util.List" | "java.util.ArrayList" | "java.util.Collection" => {
                UnmarshalType::List
            }
            "java.lang.String" => UnmarshalType::String,
            other => {
                return Err(CamelError::illegal_argument(format!(
                    "Cannot find class: {other}"
                )))
            }
        })
    }

    fn check(self, v: Value) -> CamelResult<Value> {
        let ok = match self {
            UnmarshalType::Object => true,
            UnmarshalType::Map => matches!(v, Value::Map(_) | Value::Null),
            UnmarshalType::List => matches!(v, Value::List(_) | Value::Null),
            UnmarshalType::String => matches!(v, Value::Text(_) | Value::Null),
        };
        if ok {
            Ok(v)
        } else {
            Err(CamelError::illegal_argument(format!(
                "jakarta.json.bind.JsonbException: Unable to deserialize {v} as {self:?}"
            )))
        }
    }
}

/// The JSON-B data format.
#[derive(Debug, Clone)]
pub struct JsonbDataFormat {
    unmarshal_type: UnmarshalType,
    pretty_print: bool,
    encoding: String,
    property_order: PropertyOrder,
    skip_null: bool,
}

impl Default for JsonbDataFormat {
    fn default() -> Self {
        JsonbDataFormat {
            unmarshal_type: UnmarshalType::Object,
            pretty_print: false,
            encoding: "UTF-8".to_string(),
            property_order: PropertyOrder::Any,
            skip_null: true,
        }
    }
}

/// The value with its maps in the order of the property order.
fn ordered(v: &Value, order: PropertyOrder) -> Value {
    match v {
        Value::Map(m) => {
            let mut entries: Vec<(String, Value)> = m
                .iter()
                .map(|(k, v)| (k.clone(), ordered(v, order)))
                .collect();
            match order {
                PropertyOrder::Any => {}
                PropertyOrder::Lexicographical => entries.sort_by(|a, b| a.0.cmp(&b.0)),
                PropertyOrder::Reverse => entries.sort_by(|a, b| b.0.cmp(&a.0)),
            }
            Value::Map(entries.into_iter().collect::<ValueMap>())
        }
        Value::List(l) => Value::List(l.iter().map(|i| ordered(i, order)).collect()),
        other => other.clone(),
    }
}

impl JsonbDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// With the kind of the unmarshalled values.
    pub fn with_unmarshal_type(mut self, unmarshal_type: UnmarshalType) -> Self {
        self.unmarshal_type = unmarshal_type;
        self
    }

    /// Whether the JSON is pretty printed.
    pub fn with_pretty_print(mut self, pretty_print: bool) -> Self {
        self.pretty_print = pretty_print;
        self
    }

    /// Whether the null values are left out.
    pub fn with_skip_null(mut self, skip_null: bool) -> Self {
        self.skip_null = skip_null;
        self
    }

    /// With the order of the properties.
    pub fn with_property_order(mut self, order: PropertyOrder) -> Self {
        self.property_order = order;
        self
    }

    /// The JSON text of a value.
    pub fn to_json(&self, value: &Value) -> String {
        let v = if self.skip_null {
            camel_support::json::without_null_entries(value)
        } else {
            value.clone()
        };
        camel_support::json::stringify(&ordered(&v, self.property_order), self.pretty_print)
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
                "prettyPrint" => df.pretty_print = flag(v),
                "skipNull" => df.skip_null = flag(v),
                "encoding" => {
                    Charset::for_name(v)
                        .map_err(|e| CamelError::illegal_argument(e.to_string()))?;
                    df.encoding = v.clone();
                }
                "propertyOrder" => {
                    df.property_order = match v.trim() {
                        "ANY" => PropertyOrder::Any,
                        "LEXICOGRAPHICAL" => PropertyOrder::Lexicographical,
                        "REVERSE" => PropertyOrder::Reverse,
                        other => {
                            return Err(CamelError::illegal_argument(format!(
                                "Unknown property order strategy: {other}"
                            )))
                        }
                    }
                }
                "binaryStrategy" | "propertyNamingStrategy" => {}
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the jsonb data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for JsonbDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        _exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        if !camel_support::json::write_if_already_json(graph, out) {
            let charset = Charset::for_name(&self.encoding)
                .map_err(|e| CamelError::illegal_argument(e.to_string()))?;
            out.extend(charset.encode(&self.to_json(graph)));
        }
        Ok(())
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let kind = match exchange.message().header(UNMARSHAL_TYPE) {
            Some(t) if !t.is_null() => UnmarshalType::of_class(&t.to_string())?,
            _ => self.unmarshal_type,
        };
        let v = match body {
            Value::Text(t) => camel_support::json::parse(t)?,
            other => camel_support::json::parse_bytes(&input_bytes(other)?)?,
        };
        kind.check(v)
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(JsonbDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
