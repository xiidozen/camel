//! # camel-zipfile
//!
//! The `zipFile` data format (Java `ZipFileDataFormat`): `marshal` puts the body in a zip
//! archive of one entry named after the `CamelFileName` header (without its path, unless
//! `preservePathElements`; the message id without the header), and renames the file
//! `name.zip`; `unmarshal` reads the only entry of an archive (its name in the `CamelFileName`
//! header), or with `usingIterator` gives the entries as a list of messages to split (see
//! [`zip_splitter`]). The [`ZipAggregationStrategy`] adds the aggregated exchanges to a zip
//! file. This crate is the Rust port of the Java `components/camel-zipfile` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `usingIterator` | `false` | If the zip file has more than one entry, the setting this option to true, allows working with the splitter EIP, to split the data using an iterator in a streaming mode. |
//! | `allowEmptyDirectory` | `false` | If the zip file has more than one entry, setting this option to true, allows to get the iterator even if the directory is empty. |
//! | `preservePathElements` | `false` | If the file name contains path elements, setting this option to true, allows the path to be maintained in the zip file. |
//! | `maxDecompressedSize` | `1073741824` | Set the maximum decompressed size of a zip file (in bytes). |

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod aggregation;

pub use aggregation::{entry_names, ZipAggregationStrategy};

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::FILE_NAME;
use camel_api::message::Message;
use camel_api::processor::{expression_fn, SharedExpression};
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::{ObjectRef, Value};
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "zipFile";

/// The default maximum decompressed size (1 GB).
pub const DEFAULT_MAXIMUM_DECOMPRESSED_SIZE: u64 = 1_073_741_824;

/// The header of the name (with path) of an entry of the iterator.
pub const ZIP_FILE_NAME: &str = "zipFileName";

fn io_error(e: impl std::fmt::Display) -> CamelError {
    CamelError::new(ErrorKind::Io, e.to_string())
}

/// The file name without its path (Java `FileUtil.stripPath`).
fn strip_path(name: &str) -> &str {
    let name = name.trim_end_matches(['/', '\\']);
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Reads the entry, failing beyond the maximum size (Java `IOHelper.copy` with a maximum).
fn read_limited(reader: &mut impl Read, max: u64) -> CamelResult<Vec<u8>> {
    let mut out = Vec::new();
    reader
        .take(max.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(io_error)?;
    if out.len() as u64 > max {
        return Err(io_error(format!(
            "The InputStream entry being copied exceeds the maximum allowed size: {max}"
        )));
    }
    Ok(out)
}

fn corrupted() -> CamelError {
    CamelError::illegal_state("Unable to unzip the file, it may be corrupted.")
}

/// Creates a zip archive of the entries (name, content); the names ending with `/` are
/// directories.
///
/// # Errors
///
/// An IO error.
pub fn zip_entries<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> CamelResult<Vec<u8>> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, content) in entries {
        if name.ends_with('/') {
            w.add_directory(name, options).map_err(io_error)?;
        } else {
            w.start_file(name, options).map_err(io_error)?;
            w.write_all(content).map_err(io_error)?;
        }
    }
    Ok(w.finish().map_err(io_error)?.into_inner())
}

/// The entries of a zip archive (name, content, directory), the content of each limited to the
/// maximum size.
///
/// # Errors
///
/// The data is not a zip archive, or an entry is too big.
pub fn unzip_entries(data: &[u8], max_size: u64) -> CamelResult<Vec<(String, Vec<u8>, bool)>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data)).map_err(|_| corrupted())?;
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(io_error)?;
        let name = f.name().to_string();
        let dir = f.is_dir();
        let content = if dir {
            Vec::new()
        } else {
            read_limited(&mut f, max_size)?
        };
        entries.push((name, content, dir));
    }
    Ok(entries)
}

/// The messages of the entries of the archive (Java `ZipIterator`): the headers of the
/// message, the `zipFileName` and `CamelFileName` (without path) headers, the content as body;
/// the directories are skipped unless allowed (with an empty body).
///
/// # Errors
///
/// The archive is corrupted, or an entry is too big.
pub fn zip_iterator(
    exchange: &Exchange,
    data: &[u8],
    allow_empty_directory: bool,
    max_size: u64,
) -> CamelResult<Vec<Value>> {
    let entries = unzip_entries(data, max_size)?;
    let mut messages = Vec::new();
    for (name, content, dir) in entries {
        if dir && !allow_empty_directory {
            continue;
        }
        let mut m: Message = exchange.message().clone();
        m.set_header(ZIP_FILE_NAME, name.as_str());
        m.set_header(FILE_NAME, strip_path(&name));
        m.set_body(Value::Bytes(content));
        messages.push(Value::Object(ObjectRef::new(m)));
    }
    Ok(messages)
}

/// The expression splitting a zip archive in its entries (Java `ZipSplitter`).
pub fn zip_splitter(max_decompressed_size: u64) -> SharedExpression {
    Arc::new(expression_fn(move |e: &Exchange| {
        let data = input_bytes(e.message().body())?;
        Ok(Value::List(zip_iterator(
            e,
            &data,
            false,
            max_decompressed_size,
        )?))
    }))
}

/// The `zipFile` data format.
#[derive(Debug, Clone)]
pub struct ZipFileDataFormat {
    using_iterator: bool,
    allow_empty_directory: bool,
    preserve_path_elements: bool,
    max_decompressed_size: u64,
}

impl Default for ZipFileDataFormat {
    fn default() -> Self {
        ZipFileDataFormat {
            using_iterator: false,
            allow_empty_directory: false,
            preserve_path_elements: false,
            max_decompressed_size: DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
        }
    }
}

impl ZipFileDataFormat {
    /// The data format with the default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// With the entries given as a list of messages to split.
    pub fn with_using_iterator(mut self, using_iterator: bool) -> Self {
        self.using_iterator = using_iterator;
        self
    }

    /// With the directories as entries (of the iterator).
    pub fn with_allow_empty_directory(mut self, allow: bool) -> Self {
        self.allow_empty_directory = allow;
        self
    }

    /// With the path of the file name kept in the archive.
    pub fn with_preserve_path_elements(mut self, preserve: bool) -> Self {
        self.preserve_path_elements = preserve;
        self
    }

    /// With the maximum decompressed size of an entry.
    pub fn with_max_decompressed_size(mut self, max: u64) -> Self {
        self.max_decompressed_size = max;
        self
    }

    /// The data format of the options.
    ///
    /// # Errors
    ///
    /// An unknown option or an invalid value.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true");
        for (k, v) in options {
            match k.as_str() {
                "usingIterator" => df.using_iterator = flag(v),
                "allowEmptyDirectory" => df.allow_empty_directory = flag(v),
                "preservePathElements" => df.preserve_path_elements = flag(v),
                "maxDecompressedSize" => {
                    df.max_decompressed_size = v.trim().parse().map_err(|_| {
                        CamelError::illegal_argument(format!("Invalid maxDecompressedSize: {v}"))
                    })?
                }
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the zipFile data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for ZipFileDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        let file_path = exchange
            .message()
            .header(FILE_NAME)
            .map(|v| v.to_string())
            .filter(|p| !p.is_empty());
        let (filename, file_path) = match file_path {
            Some(p) => (strip_path(&p).to_string(), p),
            None => {
                let id = exchange
                    .message()
                    .message_id()
                    .unwrap_or(exchange.exchange_id())
                    .to_string();
                let id = camel_util::string_helper::sanitize(&id);
                (id.clone(), id)
            }
        };
        let data = input_bytes(graph)?;
        let entries: Vec<(String, &[u8])> = if self.preserve_path_elements {
            // the parent directories, then the file (Java createZipEntries)
            let parts: Vec<&str> = file_path
                .split(['/', '\\'])
                .filter(|p| !p.is_empty())
                .collect();
            let mut prefix = String::new();
            let mut entries = Vec::new();
            for (i, part) in parts.iter().enumerate() {
                if i + 1 < parts.len() {
                    prefix.push_str(part);
                    prefix.push('/');
                    entries.push((prefix.clone(), &[][..]));
                } else {
                    entries.push((format!("{prefix}{part}"), &data[..]));
                }
            }
            entries
        } else {
            vec![(filename.clone(), &data[..])]
        };
        out.extend(zip_entries(entries.iter().map(|(n, c)| (n.as_str(), *c)))?);
        exchange
            .message_mut()
            .set_header(FILE_NAME, format!("{filename}.zip"));
        Ok(())
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let data = input_bytes(body)?;
        if self.using_iterator {
            // an empty archive has no entries to split (a corrupted one fails)
            let messages = zip_iterator(
                exchange,
                &data,
                self.allow_empty_directory,
                self.max_decompressed_size,
            )?;
            return Ok(Value::List(messages));
        }
        let mut entries = unzip_entries(&data, self.max_decompressed_size)?
            .into_iter()
            .filter(|(_, _, dir)| !dir);
        let (name, content, _) = entries.next().ok_or_else(corrupted)?;
        if entries.next().is_some() {
            return Err(CamelError::illegal_state("Zip file has more than 1 entry."));
        }
        exchange
            .message_mut()
            .set_header(FILE_NAME, strip_path(&name));
        Ok(Value::Bytes(content))
    }
}

/// The factory of the data format.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(ZipFileDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
