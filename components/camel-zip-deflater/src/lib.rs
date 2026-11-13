//! # camel-zip-deflater
//!
//! The `zipDeflater` and `gzipDeflater` data formats (Java `ZipDeflaterDataFormat` and
//! `GzipDeflaterDataFormat`): `marshal` compresses the body (a zlib stream as
//! `java.util.zip.Deflater`, or gzip), `unmarshal` decompresses it (gzip with the concatenated
//! members). This crate is the Rust port of the Java `components/camel-zip-deflater` module.
//!
//! # Options
//!
//! | Data format | Option | Default | Description |
//! |---|---|---|---|
//! | `zipDeflater` | `compressionLevel` | `-1` | To specify a specific compression between 0-9. -1 is default compression, 0 is no compression, and 9 is the best compression. |
//!
//! # Examples
//!
//! ```
//! use camel_zip_deflater::{deflate, inflate};
//!
//! let zipped = deflate(b"Hello World", -1).unwrap();
//! assert_eq!(b"Hello World".to_vec(), inflate(&zipped).unwrap());
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::Value;
use flate2::read::{MultiGzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use std::io::{Read, Write};
use std::sync::Arc;

/// The name of the zlib data format.
pub const ZIP_DEFLATER: &str = "zipDeflater";
/// The name of the gzip data format.
pub const GZIP_DEFLATER: &str = "gzipDeflater";

/// Java `Deflater.DEFAULT_COMPRESSION`.
pub const DEFAULT_COMPRESSION: i32 = -1;
/// Java `Deflater.BEST_SPEED`.
pub const BEST_SPEED: i32 = 1;
/// Java `Deflater.BEST_COMPRESSION`.
pub const BEST_COMPRESSION: i32 = 9;

fn io_error(e: std::io::Error) -> CamelError {
    CamelError::new(ErrorKind::Io, e.to_string())
}

fn compression(level: i32) -> CamelResult<Compression> {
    match level {
        -1 => Ok(Compression::default()),
        0..=9 => Ok(Compression::new(level as u32)),
        _ => Err(CamelError::illegal_argument(format!(
            "Invalid compression level: {level} (-1 to 9)"
        ))),
    }
}

/// Compresses the data in a zlib stream (Java `DeflaterOutputStream` with a `Deflater` of the
/// level, -1 for the default).
///
/// # Errors
///
/// An invalid level.
pub fn deflate(data: &[u8], level: i32) -> CamelResult<Vec<u8>> {
    let mut e = ZlibEncoder::new(Vec::new(), compression(level)?);
    e.write_all(data).map_err(io_error)?;
    e.finish().map_err(io_error)
}

/// Decompresses a zlib stream (Java `InflaterInputStream`).
///
/// # Errors
///
/// The data is not a valid zlib stream.
pub fn inflate(data: &[u8]) -> CamelResult<Vec<u8>> {
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(io_error)?;
    Ok(out)
}

/// Compresses the data in gzip (Java `GzipCompressorOutputStream`).
///
/// # Errors
///
/// An IO error.
pub fn gzip(data: &[u8]) -> CamelResult<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).map_err(io_error)?;
    e.finish().map_err(io_error)
}

/// Decompresses gzip data, with its concatenated members (Java `GzipCompressorInputStream`
/// with `decompressConcatenated`).
///
/// # Errors
///
/// The data is not valid gzip.
pub fn gunzip(data: &[u8]) -> CamelResult<Vec<u8>> {
    let mut out = Vec::new();
    MultiGzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(io_error)?;
    Ok(out)
}

/// The `zipDeflater` data format.
#[derive(Debug, Clone)]
pub struct ZipDeflaterDataFormat {
    compression_level: i32,
}

impl Default for ZipDeflaterDataFormat {
    fn default() -> Self {
        ZipDeflaterDataFormat {
            compression_level: DEFAULT_COMPRESSION,
        }
    }
}

impl ZipDeflaterDataFormat {
    /// The data format with the compression level (-1: default, 0 to 9).
    pub fn new(compression_level: i32) -> Self {
        ZipDeflaterDataFormat { compression_level }
    }

    /// The compression level.
    pub fn compression_level(&self) -> i32 {
        self.compression_level
    }
}

impl DataFormat for ZipDeflaterDataFormat {
    fn name(&self) -> &str {
        ZIP_DEFLATER
    }

    fn marshal(
        &self,
        _exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        out.extend(deflate(&input_bytes(graph)?, self.compression_level)?);
        Ok(())
    }

    fn unmarshal(&self, _exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        Ok(Value::Bytes(inflate(&input_bytes(body)?)?))
    }
}

/// The `gzipDeflater` data format.
#[derive(Debug, Clone, Default)]
pub struct GzipDeflaterDataFormat;

impl DataFormat for GzipDeflaterDataFormat {
    fn name(&self) -> &str {
        GZIP_DEFLATER
    }

    fn marshal(
        &self,
        _exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        out.extend(gzip(&input_bytes(graph)?)?);
        Ok(())
    }

    fn unmarshal(&self, _exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        Ok(Value::Bytes(gunzip(&input_bytes(body)?)?))
    }
}

/// The factory of `zipDeflater` (option `compressionLevel`).
pub fn zip_deflater_factory() -> DataFormatFactory {
    Arc::new(|options| {
        let mut level = DEFAULT_COMPRESSION;
        for (k, v) in options {
            match k.as_str() {
                "compressionLevel" => {
                    level = v.trim().parse().map_err(|_| {
                        CamelError::illegal_argument(format!("Invalid compressionLevel: {v}"))
                    })?;
                    compression(level)?;
                }
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the zipDeflater data format"
                    )))
                }
            }
        }
        Ok(Arc::new(ZipDeflaterDataFormat::new(level)) as SharedDataFormat)
    })
}

/// The factory of `gzipDeflater` (no options).
pub fn gzip_deflater_factory() -> DataFormatFactory {
    Arc::new(|options| match options.first() {
        Some((k, _)) => Err(CamelError::illegal_argument(format!(
            "Unknown option {k} of the gzipDeflater data format"
        ))),
        None => Ok(Arc::new(GzipDeflaterDataFormat) as SharedDataFormat),
    })
}

/// Registers both data formats on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(ZIP_DEFLATER, zip_deflater_factory());
    context.register_data_format_factory(GZIP_DEFLATER, gzip_deflater_factory());
}
