//! The file component (port of `components/camel-file`): reads files from a directory and
//! writes messages to files.
//!
//! ```text
//! file:inbox?noop=true&include=.*\.txt&delay=500    (a consumer)
//! file:outbox?fileName=${date:now:yyyyMMdd}.txt     (a producer)
//! ```
//!
//! The consumer polls the starting directory (and its sub directories with `recursive`) and
//! routes each file, its content as body (a text when it is valid UTF-8, else bytes) with the
//! `CamelFile*` headers of the Java `GenericFile` (which the simple `${file:..}` functions
//! read). After the routing the file is moved to `.camel` (the `move` option), deleted
//! (`delete`) or left (`noop`, then remembered in an idempotent repository); a failed exchange
//! leaves the file (or moves it to `moveFailed`).
//!
//! The producer writes the body to the file named by the `fileName` option, the
//! `CamelOverruleFileName` or `CamelFileName` header, or the message id, in the starting
//! directory; `fileExist` tells what to do when the file exists (`Override`, `Append`, `Fail`,
//! `Ignore`, `Move`, `TryRename`), `tempPrefix`/`tempFileName` write to a temporary file then
//! rename it.
//!
//! Not available yet: the read locks (`readLock` other than `none`), done files, sorting
//! (`sortBy`), the ant filters, checksum files, file permissions, the resume API.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod consumer;
mod producer;

pub use consumer::FileConsumer;
pub use producer::FileProducer;

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{Component, Endpoint, SharedConsumer, SharedEndpoint, SharedProducer};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    FILE_LAST_MODIFIED, FILE_LENGTH, FILE_NAME, FILE_NAME_CONSUMED, FILE_NAME_ONLY, FILE_PARENT,
    FILE_PATH,
};
use camel_api::processor::{SharedExpression, SharedProcessor};
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

/// The `CamelFileAbsolute` header: whether the file path is absolute.
pub const FILE_ABSOLUTE: &str = "CamelFileAbsolute";
/// The `CamelFileAbsolutePath` header: the absolute path of the file.
pub const FILE_ABSOLUTE_PATH: &str = "CamelFileAbsolutePath";
/// The `CamelFileRelativePath` header: the path relative to the starting directory.
pub const FILE_RELATIVE_PATH: &str = "CamelFileRelativePath";

/// What the producer does when the file exists (Java `GenericFileExist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileExist {
    /// Replaces the file (the default).
    Override,
    /// Appends to the file.
    Append,
    /// Fails with a `GenericFileOperationFailedException`.
    Fail,
    /// Keeps the existing file, silently.
    Ignore,
    /// Moves the existing file away first (with `moveExisting`).
    Move,
    /// Renames the temporary file only (with `tempFileName`), without checking.
    TryRename,
}

impl std::str::FromStr for FileExist {
    type Err = CamelError;

    fn from_str(s: &str) -> CamelResult<Self> {
        Ok(match s {
            "Override" => FileExist::Override,
            "Append" => FileExist::Append,
            "Fail" => FileExist::Fail,
            "Ignore" => FileExist::Ignore,
            "Move" => FileExist::Move,
            "TryRename" => FileExist::TryRename,
            other => {
                return Err(CamelError::illegal_argument(format!(
                    "Unknown fileExist option: {other}"
                )))
            }
        })
    }
}

/// The options of a file endpoint (Java `GenericFileEndpoint` and `FileEndpoint`).
#[derive(Debug, Clone)]
pub struct FileConfiguration {
    /// The starting directory.
    pub directory: String,
    /// The file name (a simple expression): the file written by the producer, the only file
    /// consumed by the consumer.
    pub file_name: Option<String>,
    /// The charset of the texts (default `UTF-8`).
    pub charset: Option<String>,
    /// Whether the directories are created (default `true`).
    pub auto_create: bool,
    /// Whether the path of the file names is removed (default `false`).
    pub flatten: bool,
    /// Whether the producer only writes within the starting directory (default `true`).
    pub jail_starting_directory: bool,
    /// What the producer does when the file exists.
    pub file_exist: FileExist,
    /// The prefix of the temporary file the producer writes then renames.
    pub temp_prefix: Option<String>,
    /// The name of the temporary file (a simple expression).
    pub temp_file_name: Option<String>,
    /// Where an existing file is moved to with `fileExist=Move` (a simple expression).
    pub move_existing: Option<String>,
    /// Whether the producer deletes the existing file before writing the temporary file
    /// (default `true`).
    pub eager_delete_target_file: bool,
    /// The characters appended after the content.
    pub append_chars: Option<String>,
    /// Whether a null body writes an empty file (default `false`: fails).
    pub allow_null_body: bool,
    /// The delay before the first poll in millis (default 1000).
    pub initial_delay: u64,
    /// The delay between the polls in millis (default 500).
    pub delay: u64,
    /// Whether the consumed files are left as they are (default `false`).
    pub noop: bool,
    /// Whether the consumed files are deleted (default `false`).
    pub delete: bool,
    /// Where the consumed files are moved to (a simple expression, default `.camel`).
    pub move_to: Option<String>,
    /// Where the files are moved to before being routed.
    pub pre_move: Option<String>,
    /// Where the files that failed are moved to.
    pub move_failed: Option<String>,
    /// The regular expression of the file names to consume.
    pub include: Option<String>,
    /// The regular expression of the file names not to consume.
    pub exclude: Option<String>,
    /// The extensions of the files to consume (comma separated).
    pub include_ext: Option<String>,
    /// The extensions of the files not to consume (comma separated).
    pub exclude_ext: Option<String>,
    /// Whether the sub directories are polled too.
    pub recursive: bool,
    /// The minimum depth of the files consumed (with `recursive`).
    pub min_depth: usize,
    /// The maximum depth of the files consumed (with `recursive`).
    pub max_depth: usize,
    /// The maximum number of files of a poll (0: no limit).
    pub max_messages_per_poll: usize,
    /// Whether the consumed files are remembered (default: with `noop`).
    pub idempotent: Option<bool>,
    /// Whether the starting directory must exist (default `false`).
    pub starting_directory_must_exist: bool,
    /// Whether the directory must exist when polling (default `false`).
    pub directory_must_exist: bool,
    /// Whether hidden files (starting with `.`) are consumed (default `false`).
    pub include_hidden_files: bool,
    /// The read lock (only `none` is available).
    pub read_lock: String,
}

impl FileConfiguration {
    fn new(directory: &str) -> Self {
        FileConfiguration {
            directory: directory.to_string(),
            file_name: None,
            charset: None,
            auto_create: true,
            flatten: false,
            jail_starting_directory: true,
            file_exist: FileExist::Override,
            temp_prefix: None,
            temp_file_name: None,
            move_existing: None,
            eager_delete_target_file: true,
            append_chars: None,
            allow_null_body: false,
            initial_delay: 1000,
            delay: 500,
            noop: false,
            delete: false,
            move_to: None,
            pre_move: None,
            move_failed: None,
            include: None,
            exclude: None,
            include_ext: None,
            exclude_ext: None,
            recursive: false,
            min_depth: 0,
            max_depth: usize::MAX,
            max_messages_per_poll: 0,
            idempotent: None,
            starting_directory_must_exist: false,
            directory_must_exist: false,
            include_hidden_files: false,
            read_lock: "none".into(),
        }
    }
}

/// The file component (Java `FileComponent`).
#[derive(Debug, Default)]
pub struct FileComponent;

impl FileComponent {
    /// Creates the component.
    pub fn new() -> Self {
        FileComponent
    }
}

impl Service for FileComponent {}

fn param<T: camel_api::convert::FromValue>(
    parameters: &mut Parameters,
    name: &str,
) -> CamelResult<Option<T>> {
    get_and_remove_parameter::<T>(parameters, name)
}

/// The millis of a duration option (`500`, `1s`, `2m`).
fn millis(parameters: &mut Parameters, name: &str, default: u64) -> CamelResult<u64> {
    match param::<String>(parameters, name)? {
        Some(t) => camel_util::time_utils::to_milli_seconds(&t)
            .map(|m| m.max(0) as u64)
            .map_err(|e| CamelError::illegal_argument(format!("Invalid {name}: {e}"))),
        None => Ok(default),
    }
}

impl Component for FileComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let directory = remaining.strip_prefix("//").unwrap_or(remaining);
        let mut c = FileConfiguration::new(directory);
        c.file_name = param(parameters, "fileName")?;
        c.charset = param(parameters, "charset")?;
        if let Some(v) = param(parameters, "autoCreate")? {
            c.auto_create = v;
        }
        if let Some(v) = param(parameters, "flatten")? {
            c.flatten = v;
        }
        if let Some(v) = param(parameters, "jailStartingDirectory")? {
            c.jail_starting_directory = v;
        }
        if let Some(v) = param::<String>(parameters, "fileExist")? {
            c.file_exist = v.parse()?;
        }
        c.temp_prefix = param(parameters, "tempPrefix")?;
        c.temp_file_name = param(parameters, "tempFileName")?;
        c.move_existing = param(parameters, "moveExisting")?;
        if let Some(v) = param(parameters, "eagerDeleteTargetFile")? {
            c.eager_delete_target_file = v;
        }
        // the escapes \n, \r and \t of the uri (Java setAppendChars)
        c.append_chars = param::<String>(parameters, "appendChars")?.map(|a| {
            a.replace("\\n", "\n")
                .replace("\\r", "\r")
                .replace("\\t", "\t")
        });
        if let Some(v) = param(parameters, "allowNullBody")? {
            c.allow_null_body = v;
        }
        c.initial_delay = millis(parameters, "initialDelay", 1000)?;
        c.delay = millis(parameters, "delay", 500)?;
        if let Some(v) = param(parameters, "noop")? {
            c.noop = v;
        }
        if let Some(v) = param(parameters, "delete")? {
            c.delete = v;
        }
        c.move_to = param(parameters, "move")?;
        c.pre_move = param(parameters, "preMove")?;
        c.move_failed = param(parameters, "moveFailed")?;
        c.include = param(parameters, "include")?;
        c.exclude = param(parameters, "exclude")?;
        c.include_ext = param(parameters, "includeExt")?;
        c.exclude_ext = param(parameters, "excludeExt")?;
        if let Some(v) = param(parameters, "recursive")? {
            c.recursive = v;
        }
        if let Some(v) = param::<i64>(parameters, "minDepth")? {
            c.min_depth = v.max(0) as usize;
        }
        if let Some(v) = param::<i64>(parameters, "maxDepth")? {
            c.max_depth = v.max(0) as usize;
        }
        if let Some(v) = param::<i64>(parameters, "maxMessagesPerPoll")? {
            c.max_messages_per_poll = v.max(0) as usize;
        }
        c.idempotent = param(parameters, "idempotent")?;
        if let Some(v) = param(parameters, "startingDirectoryMustExist")? {
            c.starting_directory_must_exist = v;
        }
        if let Some(v) = param(parameters, "directoryMustExist")? {
            c.directory_must_exist = v;
        }
        if let Some(v) = param(parameters, "includeHiddenFiles")? {
            c.include_hidden_files = v;
        }
        if let Some(v) = param::<String>(parameters, "readLock")? {
            if v != "none" {
                return Err(CamelError::illegal_argument(format!(
                    "The readLock {v} is not available in the Rust port yet (only none)"
                )));
            }
            c.read_lock = v;
        }
        if c.delete && c.move_to.is_some() {
            return Err(CamelError::illegal_argument(
                "You cannot set both delete=true and move options",
            ));
        }
        if c.noop && c.delete {
            return Err(CamelError::illegal_argument(
                "You cannot set both noop=true and delete=true options",
            ));
        }
        // accepted without effect (the Java scheduling, logging and cache options)
        for name in [
            "runLoggingLevel",
            "useFixedDelay",
            "synchronous",
            "bridgeErrorHandler",
            "lazyStartProducer",
            "readLockMinLength",
            "readLockTimeout",
            "readLockCheckInterval",
        ] {
            let _: Option<String> = param(parameters, name)?;
        }
        Ok(Arc::new(FileEndpoint {
            uri: uri.to_string(),
            config: c,
            context: Arc::downgrade(context),
            idempotent: Arc::new(camel_support::idempotent::MemoryIdempotentRepository::default()),
        }))
    }

    fn default_name(&self) -> Option<&str> {
        Some("file")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A file endpoint (Java `FileEndpoint`).
pub struct FileEndpoint {
    uri: String,
    config: FileConfiguration,
    context: Weak<dyn CamelContext>,
    idempotent: Arc<camel_support::idempotent::MemoryIdempotentRepository>,
}

impl FileEndpoint {
    /// The options.
    pub fn configuration(&self) -> &FileConfiguration {
        &self.config
    }

    /// The simple expression of the text.
    pub(crate) fn expression(&self, text: &str) -> CamelResult<SharedExpression> {
        self.camel_context()
            .resolve_language("simple")?
            .create_expression(text)
    }
}

impl Service for FileEndpoint {}

impl Endpoint for FileEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        self.context
            .upgrade()
            .unwrap_or_else(StandaloneContext::shared)
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(FileProducer::new(self)))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        Ok(Arc::new(FileConsumer::new(self, processor)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A file being consumed (Java `GenericFile`).
#[derive(Debug, Clone)]
pub struct GenericFile {
    /// The starting directory.
    pub endpoint_path: String,
    /// The file name relative to the starting directory (with `/`).
    pub relative_file_path: String,
    /// The path of the file (the starting directory with the relative name).
    pub file_path: PathBuf,
    /// The length.
    pub length: u64,
    /// The last modification time (millis since epoch).
    pub last_modified: i64,
}

impl GenericFile {
    pub(crate) fn of(endpoint_path: &str, path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        let relative = path
            .strip_prefix(endpoint_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        Ok(GenericFile {
            endpoint_path: endpoint_path.to_string(),
            relative_file_path: relative,
            file_path: path.to_path_buf(),
            length: meta.len(),
            last_modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default(),
        })
    }

    /// The name without the path.
    pub fn file_name_only(&self) -> String {
        self.file_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The absolute path.
    pub fn absolute_file_path(&self) -> PathBuf {
        std::path::absolute(&self.file_path).unwrap_or_else(|_| self.file_path.clone())
    }

    /// Sets the `CamelFile*` headers of the file (Java `GenericFile.populateHeaders`).
    pub fn populate_headers(&self, exchange: &mut Exchange) {
        let path = self.file_path.to_string_lossy().replace('\\', "/");
        let parent = self
            .file_path
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let absolute = self.file_path.is_absolute();
        let m = exchange.message_mut();
        m.set_header(FILE_LENGTH, self.length as i64);
        m.set_header(FILE_LAST_MODIFIED, self.last_modified);
        m.set_header(FILE_NAME_ONLY, self.file_name_only());
        m.set_header(FILE_NAME, self.relative_file_path.as_str());
        m.set_header(FILE_NAME_CONSUMED, self.relative_file_path.as_str());
        m.set_header(FILE_ABSOLUTE, absolute);
        m.set_header(
            FILE_ABSOLUTE_PATH,
            self.absolute_file_path()
                .to_string_lossy()
                .replace('\\', "/"),
        );
        m.set_header(FILE_PATH, path);
        m.set_header(FILE_RELATIVE_PATH, self.relative_file_path.as_str());
        m.set_header(FILE_PARENT, parent);
    }
}

/// A `GenericFileOperationFailedException`.
pub(crate) fn operation_failed(message: impl Into<String>) -> CamelError {
    CamelError::new(
        camel_api::error::ErrorKind::GenericFileOperationFailed,
        message.into(),
    )
}
