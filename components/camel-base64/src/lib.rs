//! # camel-base64
//!
//! The base64 data format (Java `Base64DataFormat`, on Apache Commons Codec): `marshal`
//! encodes the body in base64, in lines of `lineLength` characters (76 by default, 0 for one
//! line) each ended by the `lineSeparator` (CRLF by default), optionally with the URL safe
//! alphabet without padding (`urlSafe`); `unmarshal` decodes, ignoring the characters outside
//! of the alphabets. The crate also has the `{{base64:key}}` properties function (the value of
//! the property decoded, or `{{base64:decode:text}}`) and the `${base64Encode()}` and
//! `${base64Decode()}` simple functions. This crate is the Rust port of the Java
//! `components/camel-base64` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `lineLength` | `76` | To specific a maximum line length for the encoded data. |
//! | `lineSeparator` | CRLF | The line separators to use. |
//! | `urlSafe` | `false` | Instead of emitting '+' and '/' we emit '-' and '_' respectively. urlSafe is only applied to encode operations. Decoding seamlessly handles both modes. |
//!
//! # Examples
//!
//! ```
//! use camel_base64::{decode, encode};
//!
//! assert_eq!(b"Q2FtZWw=\r\n".to_vec(), encode(b"Camel", 76, b"\r\n", false));
//! assert_eq!(b"Camel".to_vec(), decode(b"Q2FtZWw=\r\n"));
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_api::spi::properties::PropertiesFunction;
use camel_api::spi::simple_function::SimpleFunction;
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "base64";

/// The default line length (Java `Base64.MIME_CHUNK_SIZE`).
pub const MIME_CHUNK_SIZE: usize = 76;

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encodes the bytes (Java commons-codec `Base64.encode`): lines of `line_length` characters
/// (rounded down to a multiple of 4; 0 for no lines) each followed by the separator, the URL
/// safe alphabet without padding if asked.
pub fn encode(data: &[u8], line_length: usize, separator: &[u8], url_safe: bool) -> Vec<u8> {
    let alphabet = if url_safe { URL_SAFE } else { STANDARD };
    let mut encoded = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..4 {
            if i < chars {
                encoded.push(alphabet[((n >> (18 - 6 * i)) & 0x3f) as usize]);
            } else if !url_safe {
                encoded.push(b'=');
            }
        }
    }
    let line_length = line_length / 4 * 4;
    if line_length == 0 || encoded.is_empty() {
        return encoded;
    }
    let mut out = Vec::with_capacity(
        encoded.len() + encoded.len() / line_length * separator.len() + separator.len(),
    );
    for line in encoded.chunks(line_length) {
        out.extend_from_slice(line);
        out.extend_from_slice(separator);
    }
    out
}

/// The value of a base64 character of either alphabet.
fn sextet(c: u8) -> Option<u32> {
    Some(u32::from(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => return None,
    }))
}

/// Decodes the base64 text (Java commons-codec `Base64.decode`): both alphabets, the other
/// characters (such as line separators) ignored, up to the padding.
pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in data {
        if c == b'=' {
            break;
        }
        let Some(v) = sextet(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// The base64 data format.
#[derive(Debug, Clone)]
pub struct Base64DataFormat {
    line_length: usize,
    line_separator: Vec<u8>,
    url_safe: bool,
}

impl Default for Base64DataFormat {
    fn default() -> Self {
        Base64DataFormat {
            line_length: MIME_CHUNK_SIZE,
            line_separator: b"\r\n".to_vec(),
            url_safe: false,
        }
    }
}

impl Base64DataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// With the maximum line length of the encoded data (0: one line).
    pub fn with_line_length(mut self, line_length: usize) -> Self {
        self.line_length = line_length;
        self
    }

    /// With the line separator.
    pub fn with_line_separator(mut self, separator: &[u8]) -> Self {
        self.line_separator = separator.to_vec();
        self
    }

    /// With the URL safe alphabet (without padding) for encoding.
    pub fn with_url_safe(mut self, url_safe: bool) -> Self {
        self.url_safe = url_safe;
        self
    }

    /// The data format of the options (`lineLength`, `lineSeparator`, `urlSafe`).
    ///
    /// # Errors
    ///
    /// An unknown option or an invalid value.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        for (k, v) in options {
            match k.as_str() {
                "lineLength" => {
                    df.line_length = v.trim().parse().map_err(|_| {
                        CamelError::illegal_argument(format!("Invalid lineLength: {v}"))
                    })?;
                }
                "lineSeparator" => df.line_separator = v.as_bytes().to_vec(),
                "urlSafe" => df.url_safe = v.trim().eq_ignore_ascii_case("true"),
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the base64 data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for Base64DataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        _exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        let data = input_bytes(graph)?;
        out.extend(encode(
            &data,
            self.line_length,
            &self.line_separator,
            self.url_safe,
        ));
        Ok(())
    }

    fn unmarshal(&self, _exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        Ok(Value::Bytes(decode(&input_bytes(body)?)))
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(Base64DataFormat::of_options(options)?) as SharedDataFormat))
}

/// The `{{base64:key}}` properties function (Java `Base64PropertiesFunction`): the value of the
/// property decoded, or the text after `decode:` decoded.
#[derive(Debug, Default)]
pub struct Base64PropertiesFunction;

impl PropertiesFunction for Base64PropertiesFunction {
    fn name(&self) -> &str {
        NAME
    }

    fn apply(&self, remainder: &str) -> Option<String> {
        let remainder = remainder.strip_prefix("decode:").unwrap_or(remainder);
        Some(String::from_utf8_lossy(&decode(remainder.as_bytes())).to_string())
    }

    fn lookup_first(&self, remainder: &str) -> bool {
        !remainder.starts_with("decode:")
    }
}

/// The input of a simple function as bytes (`None` for null).
fn function_input(input: &Value) -> CamelResult<Option<Vec<u8>>> {
    if input.is_null() {
        Ok(None)
    } else {
        input_bytes(input).map(Some)
    }
}

/// The `${base64Encode()}` simple function (Java `SimpleBase64Function`): the body, or the
/// argument, encoded in lines of 76 characters.
#[derive(Debug, Default)]
pub struct Base64EncodeFunction;

impl SimpleFunction for Base64EncodeFunction {
    fn name(&self) -> &str {
        "base64Encode"
    }

    fn apply(&self, _exchange: &Exchange, input: Value) -> CamelResult<Value> {
        Ok(match function_input(&input)? {
            Some(b) => Value::from(
                String::from_utf8_lossy(&encode(&b, MIME_CHUNK_SIZE, b"\r\n", false)).to_string(),
            ),
            None => Value::Null,
        })
    }
}

/// The `${base64Decode()}` simple function: the body, or the argument, decoded.
#[derive(Debug, Default)]
pub struct Base64DecodeFunction;

impl SimpleFunction for Base64DecodeFunction {
    fn name(&self) -> &str {
        "base64Decode"
    }

    fn apply(&self, _exchange: &Exchange, input: Value) -> CamelResult<Value> {
        Ok(match function_input(&input)? {
            Some(b) => Value::Bytes(decode(&b)),
            None => Value::Null,
        })
    }
}

/// Registers the data format, the properties function and the simple functions on the
/// context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
    context
        .properties()
        .add_properties_function(Arc::new(Base64PropertiesFunction));
    if let Some(r) = camel_api::context::context_plugin::<
        camel_api::spi::simple_function::SimpleFunctionRegistry,
    >(context)
    {
        r.add_function(Arc::new(Base64EncodeFunction));
        r.add_function(Arc::new(Base64DecodeFunction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec() {
        assert_eq!(b"Q2FtZWw=".to_vec(), encode(b"Camel", 0, b"", false));
        assert_eq!(b"Q2FtZWw".to_vec(), encode(b"Camel", 0, b"", true));
        assert_eq!(b"Camel".to_vec(), decode(b"Q2FtZWw=="));
        assert_eq!(vec![0xfb, 0xff], decode(b"-_8"));
        assert_eq!(vec![0xfb, 0xff], decode(b"+/8="));
        assert!(encode(b"", 76, b"\r\n", false).is_empty());
        for n in 0..200 {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();
            assert_eq!(data, decode(&encode(&data, 64, b"\n", false)));
            assert_eq!(data, decode(&encode(&data, 76, b"\r\n", true)));
        }
    }
}
