//! # camel-tarfile
//!
//! The `tarFile` data format (Java `TarFileDataFormat`): `marshal` puts the body in a tar
//! archive of one entry named after the `CamelFileName` header (without its path, unless
//! `preservePathElements`; the message id without the header), and renames the file
//! `name.tar`; `unmarshal` reads the only entry of an archive (its name in the `CamelFileName`
//! header), or with `usingIterator` gives the entries as a list of messages to split (see
//! [`tar_splitter`]). The [`TarAggregationStrategy`] adds the aggregated exchanges to a tar
//! archive. This crate is the Rust port of the Java `components/camel-tarfile` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `usingIterator` | `false` | If the tar file has more than one entry, the setting this option to true, allows to work with the splitter EIP, to split the data using an iterator in a streaming mode. |
//! | `allowEmptyDirectory` | `false` | If the tar file has more than one entry, setting this option to true, allows to get the iterator even if the directory is empty. |
//! | `preservePathElements` | `false` | If the file name contains path elements, setting this option to true, allows the path to be maintained in the tar file. |
//! | `maxDecompressedSize` | `1073741824` | Set the maximum decompressed size of a tar file (in bytes). |

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod aggregation;

pub use aggregation::{entry_names, TarAggregationStrategy};

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::FILE_NAME;
use camel_api::message::Message;
use camel_api::processor::{expression_fn, SharedExpression};
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::value::{ObjectRef, Value};
use std::io::{Cursor, Read};
use std::sync::Arc;

/// The name of the data format.
pub const NAME: &str = "tarFile";

/// The default maximum decompressed size (1 GB).
pub const DEFAULT_MAXIMUM_DECOMPRESSED_SIZE: u64 = 1_073_741_824;

/// The header of the name (with path) of an entry of the iterator (Java
/// `TarIterator.TARFILE_ENTRY_NAME_HEADER`).
pub const TARFILE_ENTRY_NAME_HEADER: &str = "CamelTarFileEntryName";

fn io_error(e: impl std::fmt::Display) -> CamelError {
    CamelError::new(ErrorKind::Io, e.to_string())
}

/// The file name without its path (Java `FileUtil.stripPath`).
fn strip_path(name: &str) -> &str {
    let name = name.trim_end_matches(['/', '\\']);
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

fn corrupted() -> CamelError {
    CamelError::illegal_state("Unable to untar the file, it may be corrupted.")
}

/// Creates a tar archive of the entries (name, content); the names ending with `/` are
/// directories.
///
/// # Errors
///
/// An IO error.
pub fn tar_entries<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> CamelResult<Vec<u8>> {
    let mut b = tar::Builder::new(Vec::new());
    for (name, content) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_mode(0o644);
        if name.ends_with('/') {
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o755);
            b.append_data(&mut h, name, std::io::empty())
                .map_err(io_error)?;
        } else {
            h.set_size(content.len() as u64);
            b.append_data(&mut h, name, content).map_err(io_error)?;
        }
    }
    b.into_inner().map_err(io_error)
}

/// The entries of a tar archive (name, content, directory), the content of each limited to the
/// maximum size.
///
/// # Errors
///
/// The data is not a tar archive, or an entry is too big.
pub fn untar_entries(data: &[u8], max_size: u64) -> CamelResult<Vec<(String, Vec<u8>, bool)>> {
    let mut archive = tar::Archive::new(Cursor::new(data));
    let mut entries = Vec::new();
    for entry in archive.entries().map_err(|_| corrupted())? {
        let mut entry = entry.map_err(|_| corrupted())?;
        let name = entry
            .path()
            .map_err(io_error)?
            .to_string_lossy()
            .replace('\\', "/");
        let dir = entry.header().entry_type().is_dir();
        let name = if dir && !name.ends_with('/') {
            format!("{name}/")
        } else {
            name
        };
        let mut content = Vec::new();
        if !dir {
            (&mut entry)
                .take(max_size.saturating_add(1))
                .read_to_end(&mut content)
                .map_err(io_error)?;
            if content.len() as u64 > max_size {
                return Err(io_error(format!(
                    "The InputStream entry being copied exceeds the maximum allowed size: {max_size}"
                )));
            }
        }
        entries.push((name, content, dir));
    }
    Ok(entries)
}

/// The messages of the entries of the archive (Java `TarIterator`): the headers of the
/// message, the `CamelTarFileEntryName` and `CamelFileName` (without path) headers, the
/// content as body; the directories are skipped unless allowed (with an empty body).
///
/// # Errors
///
/// The archive is corrupted, or an entry is too big.
pub fn tar_iterator(
    exchange: &Exchange,
    data: &[u8],
    allow_empty_directory: bool,
    max_size: u64,
) -> CamelResult<Vec<Value>> {
    let entries = untar_entries(data, max_size)?;
    let mut messages = Vec::new();
    for (name, content, dir) in entries {
        if dir && !allow_empty_directory {
            continue;
        }
        let mut m: Message = exchange.message().clone();
        m.set_header(TARFILE_ENTRY_NAME_HEADER, name.as_str());
        m.set_header(FILE_NAME, strip_path(&name));
        m.set_body(Value::Bytes(content));
        messages.push(Value::Object(ObjectRef::new(m)));
    }
    Ok(messages)
}

/// The expression splitting a tar archive in its entries (Java `TarSplitter`).
pub fn tar_splitter(max_decompressed_size: u64) -> SharedExpression {
    Arc::new(expression_fn(move |e: &Exchange| {
        let data = input_bytes(e.message().body())?;
        Ok(Value::List(tar_iterator(
            e,
            &data,
            false,
            max_decompressed_size,
        )?))
    }))
}

/// The `tarFile` data format.
#[derive(Debug, Clone)]
pub struct TarFileDataFormat {
    using_iterator: bool,
    allow_empty_directory: bool,
    preserve_path_elements: bool,
    max_decompressed_size: u64,
}

impl Default for TarFileDataFormat {
    fn default() -> Self {
        TarFileDataFormat {
            using_iterator: false,
            allow_empty_directory: false,
            preserve_path_elements: false,
            max_decompressed_size: DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
        }
    }
}

impl TarFileDataFormat {
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
                        "Unknown option {k} of the tarFile data format"
                    )))
                }
            }
        }
        Ok(df)
    }
}

impl DataFormat for TarFileDataFormat {
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
        out.extend(tar_entries(entries.iter().map(|(n, c)| (n.as_str(), *c)))?);
        exchange
            .message_mut()
            .set_header(FILE_NAME, format!("{filename}.tar"));
        Ok(())
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let data = input_bytes(body)?;
        if self.using_iterator {
            let messages = tar_iterator(
                exchange,
                &data,
                self.allow_empty_directory,
                self.max_decompressed_size,
            )?;
            return Ok(Value::List(messages));
        }
        let mut entries = untar_entries(&data, self.max_decompressed_size)?
            .into_iter()
            .filter(|(_, _, dir)| !dir);
        let (name, content, _) = entries.next().ok_or_else(corrupted)?;
        if entries.next().is_some() {
            return Err(CamelError::illegal_state("Tar file has more than 1 entry."));
        }
        exchange
            .message_mut()
            .set_header(FILE_NAME, strip_path(&name));
        Ok(Value::Bytes(content))
    }
}

/// The factory of the data format.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(TarFileDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
