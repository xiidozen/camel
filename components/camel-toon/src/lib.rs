//! # camel-toon
//!
//! The TOON data format (Java `ToonDataFormat`, on JToon): `marshal` encodes the body as
//! TOON (Token-Oriented Object Notation, a compact text form of JSON data) and `unmarshal`
//! decodes TOON into maps, lists and primitive values. A text body to marshal is a JSON
//! document. The [`codec`] module is the encoder and decoder. This crate is the Rust port of
//! the Java `components/camel-toon` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `indent` | `2` | Number of spaces per indentation level. |
//! | `delimiter` | `COMMA` | Delimiter used for tabular array rows and inline primitive arrays (`COMMA`, `TAB`, `PIPE`). |
//! | `lengthMarker` | `false` | Whether to prefix array lengths with a hash marker so arrays render as hash-prefixed lengths instead of plain lengths. |
//! | `strict` | `true` | Whether to enable strict validation when unmarshalling TOON. When false, JToon uses best-effort parsing. |
//! | `contentTypeHeader` | `true` | Whether the data format should set the Content-Type header to text/toon when marshalling. |
//!
//! # Examples
//!
//! ```
//! use camel_toon::codec::{decode, encode, DecodeOptions, EncodeOptions};
//! use camel_util::value::Value;
//!
//! let v = Value::List(vec![Value::from("red"), Value::from("green")]);
//! let toon = encode(&v, &EncodeOptions::default());
//! assert_eq!("[2]: red,green", toon);
//! assert_eq!(v, decode(&toon, &DecodeOptions::default()).unwrap());
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod codec;

use crate::codec::{decode, encode, DecodeOptions, Delimiter, EncodeOptions};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{CHARSET_NAME, CONTENT_TYPE};
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::charset::Charset;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "toon";

/// The content type of TOON (Java `ToonDataFormat.CONTENT_TYPE`).
pub const TOON_CONTENT_TYPE: &str = "text/toon";

/// The TOON data format.
#[derive(Debug, Clone)]
pub struct ToonDataFormat {
    indent: usize,
    delimiter: Delimiter,
    length_marker: bool,
    strict: bool,
    content_type_header: bool,
}

impl Default for ToonDataFormat {
    fn default() -> Self {
        ToonDataFormat {
            indent: 2,
            delimiter: Delimiter::Comma,
            length_marker: false,
            strict: true,
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

impl ToonDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// With the spaces per indentation level.
    pub fn with_indent(mut self, indent: usize) -> Self {
        self.indent = indent;
        self
    }

    /// With the delimiter of the arrays.
    pub fn with_delimiter(mut self, delimiter: Delimiter) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// Whether the array lengths are prefixed by `#`.
    pub fn with_length_marker(mut self, length_marker: bool) -> Self {
        self.length_marker = length_marker;
        self
    }

    /// Whether the decoding is strict.
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Whether marshalling sets the `Content-Type` header to `text/toon`.
    pub fn with_content_type_header(mut self, content_type_header: bool) -> Self {
        self.content_type_header = content_type_header;
        self
    }

    /// The options of the encoder.
    pub fn encode_options(&self) -> EncodeOptions {
        EncodeOptions {
            indent: self.indent,
            delimiter: self.delimiter,
            length_marker: self.length_marker,
        }
    }

    /// The options of the decoder.
    pub fn decode_options(&self) -> DecodeOptions {
        DecodeOptions {
            indent: self.indent,
            strict: self.strict,
        }
    }

    /// The data format of the options (`indent`, `delimiter`, `lengthMarker`, `strict`,
    /// `contentTypeHeader`).
    ///
    /// # Errors
    ///
    /// An unknown option or an invalid value.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true");
        for (k, v) in options {
            match k.as_str() {
                "indent" => {
                    df.indent = v.trim().parse().map_err(|_| {
                        CamelError::illegal_argument(format!("Invalid indent: {v}"))
                    })?;
                }
                "delimiter" => df.delimiter = Delimiter::parse(v)?,
                "lengthMarker" => df.length_marker = flag(v),
                "strict" => df.strict = flag(v),
                "contentTypeHeader" => df.content_type_header = flag(v),
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the toon data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for ToonDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        // a text body is a JSON document (Java JToon.encodeJson)
        let toon = match graph {
            Value::Text(json) => encode(&camel_support::json::parse(json)?, &self.encode_options()),
            other => encode(other, &self.encode_options()),
        };
        out.extend(charset_of(exchange)?.encode(&toon));
        if self.content_type_header {
            exchange
                .message_mut()
                .set_header(CONTENT_TYPE, TOON_CONTENT_TYPE);
        }
        Ok(())
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let text = match body {
            Value::Text(t) => t.clone(),
            other => charset_of(exchange)?.decode(&input_bytes(other)?),
        };
        decode(&text, &self.decode_options())
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(ToonDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
