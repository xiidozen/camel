//! # camel-lzf
//!
//! The LZF data format (Java `LZFDataFormat`, on the Ning `compress-lzf` library): `marshal`
//! compresses the body with the LZF algorithm into the chunked LZF stream format, and
//! `unmarshal` decompresses it. The chunks are those of `compress-lzf` (`ZV` signature, then
//! a compressed or an uncompressed block of at most 65535 bytes), so the data is compatible
//! with the Java data format. This crate is the Rust port of the Java `components/camel-lzf`
//! module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `usingParallelCompression` | `false` | Enable encoding (compress) using multiple processing cores (the same stream format; the chunks are compressed one after the other in Rust). |
//!
//! # Examples
//!
//! ```
//! use camel_lzf::{compress, decompress};
//!
//! let data = b"To be, or not to be: that is the question: to be, or not to be".repeat(10);
//! let compressed = compress(&data);
//! assert!(compressed.len() < data.len());
//! assert_eq!(data, decompress(&compressed).unwrap());
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::Value;
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "lzf";

/// The maximum length of a chunk (Java `LZFChunk.MAX_CHUNK_LEN`).
pub const MAX_CHUNK_LEN: usize = 0xFFFF;

/// The blocks shorter than this are not compressed (Java `ChunkEncoder.MIN_BLOCK_TO_COMPRESS`).
const MIN_BLOCK_TO_COMPRESS: usize = 16;

const BLOCK_TYPE_NON_COMPRESSED: u8 = 0;
const BLOCK_TYPE_COMPRESSED: u8 = 1;

/// The longest back reference (Java `ChunkEncoder.MAX_REF`).
const MAX_REF: usize = (1 << 8) + (1 << 3);
/// The farthest back reference (Java `ChunkEncoder.MAX_OFF`).
const MAX_OFF: usize = 1 << 13;
/// The longest literal run.
const MAX_LITERAL: usize = 1 << 5;
const HASH_BITS: u32 = 14;

fn hash(b: &[u8]) -> usize {
    let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// Compresses a block with the LZF algorithm (without the chunk header): runs of up to 32
/// literals (a control byte below 32) and back references of 3 to 264 bytes up to 8192 bytes
/// back.
pub fn compress_block(input: &[u8]) -> Vec<u8> {
    let n = input.len();
    let mut out = Vec::with_capacity(n + n / 32 + 1);
    let mut table = vec![usize::MAX; 1 << HASH_BITS];
    let mut ctrl = out.len();
    out.push(0);
    let mut literals = 0usize;
    let mut i = 0;
    while i < n {
        if i + 2 < n {
            let h = hash(&input[i..]);
            let r = table[h];
            table[h] = i;
            if r != usize::MAX && i - r <= MAX_OFF && input[r..r + 3] == input[i..i + 3] {
                let max = MAX_REF.min(n - i);
                let mut len = 3;
                while len < max && input[r + len] == input[i + len] {
                    len += 1;
                }
                if literals > 0 {
                    out[ctrl] = (literals - 1) as u8;
                } else {
                    out.pop();
                }
                let off = i - r - 1;
                let l = len - 2;
                if l < 7 {
                    out.push(((off >> 8) as u8) | ((l as u8) << 5));
                } else {
                    out.push(((off >> 8) as u8) | (7 << 5));
                    out.push((l - 7) as u8);
                }
                out.push(off as u8);
                // the positions of the reference in the table
                for k in i + 1..(i + len).min(n.saturating_sub(2)) {
                    table[hash(&input[k..])] = k;
                }
                i += len;
                ctrl = out.len();
                out.push(0);
                literals = 0;
                continue;
            }
        }
        out.push(input[i]);
        literals += 1;
        i += 1;
        if literals == MAX_LITERAL {
            out[ctrl] = (MAX_LITERAL - 1) as u8;
            ctrl = out.len();
            out.push(0);
            literals = 0;
        }
    }
    if literals > 0 {
        out[ctrl] = (literals - 1) as u8;
    } else {
        out.pop();
    }
    out
}

fn corrupt(message: String) -> CamelError {
    CamelError::new(ErrorKind::Io, message)
}

/// Decompresses an LZF block (without the chunk header) into the output.
///
/// # Errors
///
/// A corrupt block.
pub fn decompress_block(input: &[u8], out: &mut Vec<u8>) -> CamelResult<()> {
    let start = out.len();
    let mut ip = 0;
    while ip < input.len() {
        let ctrl = usize::from(input[ip]);
        ip += 1;
        if ctrl < MAX_LITERAL {
            let run = ctrl + 1;
            let lit = input.get(ip..ip + run).ok_or_else(|| {
                corrupt("Corrupt input data: literal run past the end of the block".into())
            })?;
            out.extend_from_slice(lit);
            ip += run;
        } else {
            let mut len = ctrl >> 5;
            if len == 7 {
                len += usize::from(*input.get(ip).ok_or_else(|| {
                    corrupt("Corrupt input data: truncated back reference".into())
                })?);
                ip += 1;
            }
            let low =
                usize::from(*input.get(ip).ok_or_else(|| {
                    corrupt("Corrupt input data: truncated back reference".into())
                })?);
            ip += 1;
            let off = ((ctrl & 0x1f) << 8) + low + 1;
            if off > out.len() - start {
                return Err(corrupt(
                    "Corrupt input data: back reference before the start of the block".into(),
                ));
            }
            let from = out.len() - off;
            for k in 0..len + 2 {
                let b = out[from + k];
                out.push(b);
            }
        }
    }
    Ok(())
}

/// Compresses the data into LZF chunks (Java `LZFOutputStream`): each block of up to
/// [`MAX_CHUNK_LEN`] bytes is compressed, or kept as is when compressing does not make it
/// shorter.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    for block in data.chunks(MAX_CHUNK_LEN) {
        let compressed = if block.len() >= MIN_BLOCK_TO_COMPRESS {
            Some(compress_block(block)).filter(|c| c.len() + 2 < block.len())
        } else {
            None
        };
        out.extend_from_slice(b"ZV");
        match compressed {
            Some(c) => {
                out.push(BLOCK_TYPE_COMPRESSED);
                out.extend_from_slice(&(c.len() as u16).to_be_bytes());
                out.extend_from_slice(&(block.len() as u16).to_be_bytes());
                out.extend_from_slice(&c);
            }
            None => {
                out.push(BLOCK_TYPE_NON_COMPRESSED);
                out.extend_from_slice(&(block.len() as u16).to_be_bytes());
                out.extend_from_slice(block);
            }
        }
    }
    out
}

/// Decompresses LZF chunks (Java `LZFInputStream`).
///
/// # Errors
///
/// Corrupt data: a missing `ZV` signature, an unknown block type, a truncated chunk or a
/// block not of its announced length.
pub fn decompress(data: &[u8]) -> CamelResult<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut pos = 0;
    let mut block = 0;
    let u16_at = |p: usize, block: usize| -> CamelResult<usize> {
        data.get(p..p + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            .ok_or_else(|| {
                corrupt(format!(
                    "Corrupt input data, block #{block}: truncated header"
                ))
            })
    };
    while pos < data.len() {
        if data.get(pos..pos + 2) != Some(b"ZV") {
            return Err(corrupt(format!(
                "Corrupt input data, block #{block} (at offset {pos}): did not start with 'ZV' signature bytes"
            )));
        }
        let kind = *data.get(pos + 2).ok_or_else(|| {
            corrupt(format!(
                "Corrupt input data, block #{block}: truncated header"
            ))
        })?;
        match kind {
            BLOCK_TYPE_NON_COMPRESSED => {
                let len = u16_at(pos + 3, block)?;
                let bytes = data
                    .get(pos + 5..pos + 5 + len)
                    .ok_or_else(|| corrupt(format!("Corrupt input data, block #{block}: truncated block")))?;
                out.extend_from_slice(bytes);
                pos += 5 + len;
            }
            BLOCK_TYPE_COMPRESSED => {
                let clen = u16_at(pos + 3, block)?;
                let ulen = u16_at(pos + 5, block)?;
                let bytes = data
                    .get(pos + 7..pos + 7 + clen)
                    .ok_or_else(|| corrupt(format!("Corrupt input data, block #{block}: truncated block")))?;
                let before = out.len();
                decompress_block(bytes, &mut out)?;
                if out.len() - before != ulen {
                    return Err(corrupt(format!(
                        "Corrupt input data, block #{block}: decompressed {} bytes instead of {ulen}",
                        out.len() - before
                    )));
                }
                pos += 7 + clen;
            }
            other => {
                return Err(corrupt(format!(
                    "Corrupt input data, block #{block} (at offset {pos}): unrecognized block type {other}"
                )))
            }
        }
        block += 1;
    }
    Ok(out)
}

/// The LZF data format.
#[derive(Debug, Clone, Default)]
pub struct LzfDataFormat {
    using_parallel_compression: bool,
}

impl LzfDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the compression uses several cores (the same stream format).
    pub fn with_using_parallel_compression(mut self, parallel: bool) -> Self {
        self.using_parallel_compression = parallel;
        self
    }

    /// Whether the compression uses several cores.
    pub fn is_using_parallel_compression(&self) -> bool {
        self.using_parallel_compression
    }

    /// The data format of the options (`usingParallelCompression`).
    ///
    /// # Errors
    ///
    /// An unknown option.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        for (k, v) in options {
            match k.as_str() {
                "usingParallelCompression" => {
                    df.using_parallel_compression = v.trim().eq_ignore_ascii_case("true");
                }
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the lzf data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for LzfDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        _exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        out.extend(compress(&input_bytes(graph)?));
        Ok(())
    }

    fn unmarshal(&self, _exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        Ok(Value::Bytes(decompress(&input_bytes(body)?)?))
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(LzfDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec() {
        assert_eq!(b"ZV\0\0\x03abc".to_vec(), compress(b"abc"));
        assert!(compress(b"").is_empty());
        assert!(decompress(b"").unwrap().is_empty());
        for n in [0usize, 1, 15, 16, 31, 32, 33, 100, 1000, 70_000, 200_000] {
            let data: Vec<u8> = (0..n)
                .map(|i| ((i * 7) % 13 + (i / 1000) % 5) as u8)
                .collect();
            assert_eq!(data, decompress(&compress(&data)).unwrap(), "{n}");
            let random: Vec<u8> = (0..n as u64)
                .map(|i| {
                    (i.wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407)
                        >> 56) as u8
                })
                .collect();
            assert_eq!(random, decompress(&compress(&random)).unwrap(), "{n}");
        }
        let long = vec![b'a'; 10_000];
        assert!(compress(&long).len() < 200);
        // a block of the Java library: "aaaaaaaaaaaaaaaaaaaa" (a literal then a back reference)
        assert_eq!(
            vec![b'a'; 20],
            decompress(&[b'Z', b'V', 1, 0, 5, 0, 20, 0, b'a', 0xe0, 10, 0]).unwrap()
        );
        assert!(decompress(b"XV\0\0\x01a").is_err());
        assert!(decompress(&[b'Z', b'V', 1, 0, 2, 0, 5, 0x20, 0]).is_err());
        assert!(decompress(&[b'Z', b'V', 7]).is_err());
    }
}
