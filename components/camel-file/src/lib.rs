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
pub mod read_lock;

pub use consumer::{FileConsumer, FilePollingConsumer};
pub use producer::{generated_file_name, FileProducer};
pub use read_lock::{ReadLockKind, ReadLockOptions};

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
use camel_util::value::Value;
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
    /// The name of the done file (`${file:name}` and `${file:name.noext}` are the only
    /// dynamic parts): the producer writes it after the file, the consumer only takes the
    /// files with their done file, and deletes it with the file.
    pub done_file_name: Option<String>,
    /// Whether the consumer moves the files by copy and delete (`renameUsingCopy`).
    pub rename_using_copy: bool,
    /// Whether a failed rename of the consumer falls back to copy and delete
    /// (`copyAndDeleteOnRenameFail`, default `true`).
    pub copy_and_delete_on_rename_fail: bool,
    /// Whether the errors of the polls are routed to the error handler of the route as an
    /// exchange with the exception (`bridgeErrorHandler`), else logged.
    pub bridge_error_handler: bool,
    /// The Ant patterns of the files to include (`antInclude`, comma separated), matched on the
    /// path relative to the starting directory.
    pub ant_include: Option<Vec<String>>,
    /// The Ant patterns of the files to exclude (`antExclude`, comma separated).
    pub ant_exclude: Option<Vec<String>>,
    /// Whether the Ant patterns are case sensitive (`antFilterCaseSensitive`, default `true`).
    pub ant_filter_case_sensitive: bool,
    /// The algorithm of the checksum of the written files (`checksumFileAlgorithm`, such as
    /// `md5` or `sha256`), set as the `CamelFileChecksum` header.
    pub checksum_file_algorithm: Option<String>,
    /// Whether the checksum is written to `<file>.<algorithm>` (`checksumWriteFile`, default
    /// `true`).
    pub checksum_write_file: bool,
    /// The permissions of the written files (`chmod`, octal such as `644`).
    pub chmod: Option<u32>,
    /// The permissions of the directories the producer creates (`chmodDirectory`).
    pub chmod_directory: Option<u32>,
    /// A simple predicate the directories must match to be polled (`filterDirectory`, with
    /// the `CamelFile*` headers of the directory).
    pub filter_directory: Option<String>,
    /// A simple predicate the files must match to be consumed (`filterFile`, with the
    /// `CamelFile*` headers and the content of the file as the body).
    pub filter_file: Option<String>,
    /// Whether the written files are synced to the disk (default `true`).
    pub force_writes: bool,
    /// Whether a written file keeps the `CamelFileLastModified` of the message (default
    /// `false`).
    pub keep_last_modified: bool,
    /// Whether a poll without files routes an empty message (default `false`).
    pub send_empty_message_when_idle: bool,
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
    /// The key of a file in the idempotent repository (a simple expression; default the
    /// absolute path).
    pub idempotent_key: Option<String>,
    /// Whether the key is added to the idempotent repository when the file is polled (and
    /// confirmed when done), else added when done (`idempotentEager`, default `true`).
    pub idempotent_eager: bool,
    /// Whether the starting directory must exist (default `false`).
    pub starting_directory_must_exist: bool,
    /// Whether the directory must exist when polling (default `false`).
    pub directory_must_exist: bool,
    /// Whether hidden files (starting with `.`) are consumed (default `false`).
    pub include_hidden_files: bool,
    /// Whether the hidden directories (starting with a dot) are polled (default `false`).
    pub include_hidden_dirs: bool,
    /// The read lock of the consumed files (`readLock` and the `readLock*` options).
    pub read_lock: ReadLockOptions,
    /// The sort groups of the files of a poll (`sortBy`; the listing is by name).
    pub sort_by: Vec<SortBy>,
    /// Whether `maxMessagesPerPoll` limits the files before sorting them (default `true`).
    pub eager_max_messages_per_poll: bool,
}

/// A group of the `sortBy` option (Java `GenericFileDefaultSorter`): a file language
/// expression (`file:name`, `file:modified`, `date:file:yyyyMMdd`, ...) with the `reverse:`
/// and `ignoreCase:` prefixes; the groups are separated by `;`, the next one sorting the
/// files the previous one finds equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortBy {
    /// The simple expression (with the file functions).
    pub expression: String,
    /// Whether the order is reversed.
    pub reverse: bool,
    /// Whether texts are compared ignoring the case.
    pub ignore_case: bool,
}

impl SortBy {
    /// Parses the `sortBy` option (Java `createSortByComparator`).
    ///
    /// # Errors
    ///
    /// An empty group.
    pub fn parse(text: &str) -> CamelResult<Vec<SortBy>> {
        text.split(';')
            .map(|group| {
                let reverse = group.starts_with("reverse:");
                let rest = group.strip_prefix("reverse:").unwrap_or(group);
                let ignore_case = rest.starts_with("ignoreCase:");
                let rest = rest.strip_prefix("ignoreCase:").unwrap_or(rest);
                if rest.is_empty() {
                    return Err(CamelError::illegal_argument(
                        "sortBy expression must be specified and not empty",
                    ));
                }
                let mut expression = rest.to_string();
                if !expression.starts_with("${") {
                    expression.insert_str(0, "${");
                }
                if !expression.ends_with('}') {
                    expression.push('}');
                }
                Ok(SortBy {
                    expression,
                    reverse,
                    ignore_case,
                })
            })
            .collect()
    }
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
            done_file_name: None,
            rename_using_copy: false,
            copy_and_delete_on_rename_fail: true,
            bridge_error_handler: false,
            ant_include: None,
            ant_exclude: None,
            ant_filter_case_sensitive: true,
            checksum_file_algorithm: None,
            checksum_write_file: true,
            chmod: None,
            chmod_directory: None,
            filter_directory: None,
            filter_file: None,
            force_writes: true,
            keep_last_modified: false,
            send_empty_message_when_idle: false,
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
            idempotent_key: None,
            idempotent_eager: true,
            starting_directory_must_exist: false,
            directory_must_exist: false,
            include_hidden_files: false,
            include_hidden_dirs: false,
            read_lock: ReadLockOptions::default(),
            sort_by: Vec::new(),
            eager_max_messages_per_poll: true,
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

/// The mode of a chmod option (Java `chmodPermissionsAreValid`): 3 octal digits, or 4 with a
/// leading one ignored.
fn chmod(value: Option<String>, name: &str) -> CamelResult<Option<u32>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value.trim();
    let invalid = || CamelError::illegal_argument(format!("{name} option [{value}] is not valid"));
    if text.len() < 3 || text.len() > 4 {
        return Err(invalid());
    }
    let digits = &text[text.len() - 3..];
    if !digits.chars().all(|c| ('0'..='7').contains(&c)) {
        return Err(invalid());
    }
    u32::from_str_radix(digits, 8)
        .map(Some)
        .map_err(|_| invalid())
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
        // the starting directory is static (Java FileComponent.buildFileEndpoint)
        if directory.contains("${") || directory.contains("$simple{") {
            return Err(CamelError::illegal_argument(format!(
                "Invalid directory: {directory}. Dynamic expressions with ${{ }} placeholders is not allowed. Use the fileName option to set the dynamic expression."
            )));
        }
        let mut c = FileConfiguration::new(directory);
        c.file_name = param(parameters, "fileName")?;
        c.charset = param(parameters, "charset")?;
        // an unknown charset fails (Java IOHelper.validateCharset)
        if let Some(cs) = &c.charset {
            camel_util::charset::Charset::for_name(cs)
                .map_err(|e| CamelError::illegal_argument(e.to_string()))?;
        }
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
        c.done_file_name = param(parameters, "doneFileName")?;
        if let Some(v) = param(parameters, "renameUsingCopy")? {
            c.rename_using_copy = v;
        }
        if let Some(v) = param(parameters, "copyAndDeleteOnRenameFail")? {
            c.copy_and_delete_on_rename_fail = v;
        }
        if let Some(v) = param(parameters, "bridgeErrorHandler")? {
            c.bridge_error_handler = v;
        }
        let patterns = |v: String| v.split(',').map(str::to_string).collect::<Vec<_>>();
        c.ant_include = param::<String>(parameters, "antInclude")?.map(patterns);
        c.ant_exclude = param::<String>(parameters, "antExclude")?.map(patterns);
        if let Some(v) = param(parameters, "antFilterCaseSensitive")? {
            c.ant_filter_case_sensitive = v;
        }
        c.checksum_file_algorithm = param(parameters, "checksumFileAlgorithm")?;
        if let Some(v) = param(parameters, "checksumWriteFile")? {
            c.checksum_write_file = v;
        }
        c.chmod = chmod(param(parameters, "chmod")?, "chmod")?;
        c.chmod_directory = chmod(param(parameters, "chmodDirectory")?, "chmodDirectory")?;
        c.filter_directory = param(parameters, "filterDirectory")?;
        c.filter_file = param(parameters, "filterFile")?;
        if let Some(v) = param(parameters, "forceWrites")? {
            c.force_writes = v;
        }
        if let Some(v) = param(parameters, "keepLastModified")? {
            c.keep_last_modified = v;
        }
        if let Some(v) = param(parameters, "sendEmptyMessageWhenIdle")? {
            c.send_empty_message_when_idle = v;
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
        c.idempotent_key = param(parameters, "idempotentKey")?;
        if let Some(v) = param(parameters, "idempotentEager")? {
            c.idempotent_eager = v;
        }
        if let Some(v) = param(parameters, "startingDirectoryMustExist")? {
            c.starting_directory_must_exist = v;
        }
        if let Some(v) = param(parameters, "directoryMustExist")? {
            c.directory_must_exist = v;
        }
        if let Some(v) = param(parameters, "includeHiddenFiles")? {
            c.include_hidden_files = v;
        }
        if let Some(v) = param(parameters, "includeHiddenDirs")? {
            c.include_hidden_dirs = v;
        }
        if let Some(v) = param::<String>(parameters, "readLock")? {
            c.read_lock.kind = v.parse()?;
        }
        c.read_lock.timeout = millis(parameters, "readLockTimeout", c.read_lock.timeout)?;
        c.read_lock.check_interval = millis(
            parameters,
            "readLockCheckInterval",
            c.read_lock.check_interval,
        )?;
        if let Some(v) = param::<i64>(parameters, "readLockMinLength")? {
            c.read_lock.min_length = v.max(0) as u64;
        }
        c.read_lock.min_age = millis(parameters, "readLockMinAge", c.read_lock.min_age)?;
        if let Some(v) = param(parameters, "readLockMarkerFile")? {
            c.read_lock.marker_file = v;
        }
        if let Some(v) = param(parameters, "readLockDeleteOrphanLockFiles")? {
            c.read_lock.delete_orphan_lock_files = v;
        }
        if let Some(v) = param(parameters, "readLockRemoveOnCommit")? {
            c.read_lock.remove_on_commit = v;
        }
        if let Some(v) = param(parameters, "readLockRemoveOnRollback")? {
            c.read_lock.remove_on_rollback = v;
        }
        c.read_lock.idempotent_release_delay = millis(
            parameters,
            "readLockIdempotentReleaseDelay",
            c.read_lock.idempotent_release_delay,
        )?;
        if let Some(v) = param(parameters, "readLockIdempotentReleaseAsync")? {
            c.read_lock.idempotent_release_async = v;
        }
        let _: Option<String> = param(parameters, "readLockIdempotentReleaseAsyncPoolSize")?;
        // the repository of the idempotent consumer and read locks (a bean of the registry)
        let idempotent = match param::<String>(parameters, "idempotentRepository")? {
            Some(name) => {
                let name = name.strip_prefix('#').unwrap_or(&name);
                let name = name.strip_prefix("bean:").unwrap_or(name);
                context
                    .registry()
                    .and_then(|r| r.lookup_by_name(name))
                    .and_then(|v| camel_support::idempotent::repository_of(&v))
                    .ok_or_else(|| CamelError::no_such_bean(name))?
            }
            None if c.read_lock.kind == ReadLockKind::Idempotent => {
                return Err(CamelError::illegal_argument(
                    "IdempotentRepository must be configured when using readLock=idempotent",
                ))
            }
            None => Arc::new(camel_support::idempotent::MemoryIdempotentRepository::default()),
        };
        if let Some(v) = param::<String>(parameters, "readLockLoggingLevel")? {
            c.read_lock.logging_level = match v.to_ascii_uppercase().as_str() {
                "OFF" => None,
                "TRACE" => Some(log::Level::Trace),
                "DEBUG" => Some(log::Level::Debug),
                "INFO" => Some(log::Level::Info),
                "WARN" => Some(log::Level::Warn),
                "ERROR" => Some(log::Level::Error),
                other => {
                    return Err(CamelError::illegal_argument(format!(
                        "Invalid readLockLoggingLevel: {other}"
                    )))
                }
            };
        }
        if let Some(sort) = param::<String>(parameters, "sortBy")? {
            c.sort_by = SortBy::parse(&sort)?;
        }
        if let Some(v) = param(parameters, "eagerMaxMessagesPerPoll")? {
            c.eager_max_messages_per_poll = v;
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
        // accepted without effect (the Java scheduling, logging, backoff and buffer options)
        for name in [
            "runLoggingLevel",
            "bufferSize",
            "backoffMultiplier",
            "backoffIdleThreshold",
            "backoffErrorThreshold",
            "useFixedDelay",
            "synchronous",
            "lazyStartProducer",
        ] {
            let _: Option<String> = param(parameters, name)?;
        }
        let filter = bean_option::<SharedFileFilter>(context, param(parameters, "filter")?)?;
        let poll_strategy = bean_option::<camel_api::spi::poll_strategy::SharedPollStrategy>(
            context,
            param(parameters, "pollStrategy")?,
        )?;
        let sorter = bean_option::<SharedFileSorter>(context, param(parameters, "sorter")?)?;
        Ok(Arc::new(FileEndpoint {
            uri: uri.to_string(),
            config: c,
            context: Arc::downgrade(context),
            idempotent,
            filter,
            sorter,
            poll_strategy,
            in_progress: std::sync::Mutex::new(std::collections::HashSet::new()),
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
    idempotent: camel_api::spi::idempotent_repository::SharedIdempotentRepository,
    in_progress: std::sync::Mutex<std::collections::HashSet<String>>,
    filter: Option<SharedFileFilter>,
    sorter: Option<SharedFileSorter>,
    poll_strategy: Option<camel_api::spi::poll_strategy::SharedPollStrategy>,
}

impl FileEndpoint {
    /// The done file of the file (Java `createDoneFileName`): in the same directory, its name
    /// the `doneFileName` pattern with `${file:name}` and `${file:name.noext}` of the file.
    ///
    /// # Errors
    ///
    /// No `doneFileName`, or a pattern with other dynamic parts.
    pub fn create_done_file_name(&self, file_name: &str) -> CamelResult<String> {
        let pattern = self
            .config
            .done_file_name
            .as_deref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                CamelError::illegal_argument("doneFileName must be specified and not empty")
            })?;
        let (path, only) = match file_name.rfind(['/', '\\']) {
            Some(i) => (&file_name[..i], &file_name[i + 1..]),
            None => ("", file_name),
        };
        let noext = camel_util::file_util::strip_ext_mode(only, true);
        let mut name = pattern.to_string();
        for (token, value) in [
            ("${file:name}", only),
            ("$simple{file:name}", only),
            ("${file:name.noext}", noext),
            ("$simple{file:name.noext}", noext),
        ] {
            name = name.replacen(token, value, 1);
        }
        if has_simple_token(&name) {
            return Err(CamelError::expression_illegal_syntax(&format!(
                "{file_name}. Cannot resolve reminder: {name}"
            )));
        }
        Ok(if path.is_empty() || name.is_empty() {
            name
        } else {
            format!("{path}/{name}")
        })
    }

    /// Whether the file name is a done file (Java `isDoneFile`): the pattern without dynamic
    /// parts, else the static part as prefix or suffix.
    ///
    /// # Errors
    ///
    /// No `doneFileName`, or a pattern with other dynamic parts.
    pub fn is_done_file(&self, file_name: &str) -> CamelResult<bool> {
        let pattern = self
            .config
            .done_file_name
            .as_deref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                CamelError::illegal_argument("doneFileName must be specified and not empty")
            })?;
        if !has_simple_token(pattern) {
            return Ok(pattern == file_name);
        }
        let prefix = pattern.find("${").is_some_and(|i| i > 0);
        let mut rest = pattern.to_string();
        for token in [
            "${file:name}",
            "$simple{file:name}",
            "${file:name.noext}",
            "$simple{file:name.noext}",
        ] {
            rest = rest.replacen(token, "", 1);
        }
        if has_simple_token(&rest) {
            return Err(CamelError::expression_illegal_syntax(&format!(
                "{file_name}. Cannot resolve reminder: {rest}"
            )));
        }
        Ok(if prefix {
            file_name.starts_with(&rest)
        } else {
            file_name.ends_with(&rest)
        })
    }

    /// The options.
    pub fn configuration(&self) -> &FileConfiguration {
        &self.config
    }

    /// The simple predicate of the text.
    pub(crate) fn predicate(
        &self,
        text: &str,
    ) -> CamelResult<camel_api::processor::SharedPredicate> {
        self.camel_context()
            .resolve_language("simple")?
            .create_predicate(text)
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
        let c = &self.config;
        if c.file_exist == FileExist::Append
            && (c.temp_prefix.is_some() || c.temp_file_name.is_some())
        {
            return Err(CamelError::illegal_argument(
                "You cannot set both fileExist=Append and tempPrefix/tempFileName options",
            ));
        }
        Ok(Arc::new(FileProducer::new(self)))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        // a regular file as the directory polls nothing (Java CAMEL-24839)
        let dir = Path::new(&self.config.directory);
        if dir.is_file() {
            let parent = dir
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".to_string());
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(CamelError::illegal_argument(format!(
                "{} is a file, not a directory: the file component reads the files of a directory. To read this one file use file:{parent}?fileName={name} (as from:, or with pollEnrich)",
                self.config.directory
            )));
        }
        Ok(Arc::new(FileConsumer::new(self, processor)))
    }

    fn create_polling_consumer(
        self: Arc<Self>,
    ) -> CamelResult<Arc<dyn camel_api::endpoint::PollingConsumer>> {
        Ok(Arc::new(FilePollingConsumer::new(self)))
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
    /// Whether it is a directory.
    pub directory: bool,
}

/// A filter of the polled files and directories (Java `GenericFileFilter`), a bean of the
/// registry set with the `filter=#name` option (see [`filter_value`]).
pub trait GenericFileFilter: Send + Sync {
    /// Whether the file (or directory, to descend into it) is accepted.
    fn accept(&self, file: &GenericFile) -> bool;
}

/// A shared [`GenericFileFilter`].
pub type SharedFileFilter = Arc<dyn GenericFileFilter>;

impl<F> GenericFileFilter for F
where
    F: Fn(&GenericFile) -> bool + Send + Sync,
{
    fn accept(&self, file: &GenericFile) -> bool {
        self(file)
    }
}

/// A filter of Ant patterns on the path relative to the starting directory (Java
/// `AntPathMatcherGenericFileFilter`): a file is accepted when not excluded and included (any
/// file not excluded when there are only excludes); directories are accepted.
#[derive(Debug, Clone, Default)]
pub struct AntPathMatcherGenericFileFilter {
    /// The patterns of the included files.
    pub includes: Option<Vec<String>>,
    /// The patterns of the excluded files.
    pub excludes: Option<Vec<String>>,
    /// Whether the patterns are case insensitive (Java `caseSensitive=false`).
    pub case_insensitive: bool,
}

impl AntPathMatcherGenericFileFilter {
    /// The filter including the patterns.
    pub fn including<S: AsRef<str>>(patterns: &[S]) -> Self {
        AntPathMatcherGenericFileFilter {
            includes: Some(patterns.iter().map(|p| p.as_ref().to_string()).collect()),
            ..Default::default()
        }
    }

    /// Whether the relative path is accepted (Java `AntPathMatcherFileFilter.acceptPathName`).
    pub fn accept_path_name(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        let matcher = camel_util::ant_path_matcher::AntPathMatcher::new();
        let cs = !self.case_insensitive;
        if let Some(excludes) = &self.excludes {
            if excludes.iter().any(|p| matcher.matches(p, &path, cs)) {
                return false;
            }
        }
        match &self.includes {
            Some(includes) => includes.iter().any(|p| matcher.matches(p, &path, cs)),
            None => self.excludes.is_some(),
        }
    }
}

impl GenericFileFilter for AntPathMatcherGenericFileFilter {
    fn accept(&self, file: &GenericFile) -> bool {
        file.directory || self.accept_path_name(&file.relative_file_path)
    }
}

/// The comparator of the polled files (Java `Comparator<GenericFile>`), a bean of the registry
/// set with the `sorter=#name` option (see [`sorter_value`]).
pub type SharedFileSorter =
    Arc<dyn Fn(&GenericFile, &GenericFile) -> std::cmp::Ordering + Send + Sync>;

/// The filter as a value, to bind it in the registry.
pub fn filter_value(filter: SharedFileFilter) -> Value {
    Value::Object(camel_util::value::ObjectRef::new(filter))
}

/// The sorter as a value, to bind it in the registry.
pub fn sorter_value(sorter: SharedFileSorter) -> Value {
    Value::Object(camel_util::value::ObjectRef::new(sorter))
}

/// The bean of the registry referred by the option (`#name`), of the type.
fn bean_option<T: Clone + 'static>(
    context: &Arc<dyn CamelContext>,
    value: Option<String>,
) -> CamelResult<Option<T>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let name = value.strip_prefix('#').unwrap_or(&value);
    let name = name.strip_prefix("bean:").unwrap_or(name);
    context
        .registry()
        .and_then(|r| r.lookup_by_name(name))
        .and_then(|v| match v {
            Value::Object(o) => o.downcast_ref::<T>().cloned(),
            _ => None,
        })
        .map(Some)
        .ok_or_else(|| CamelError::no_such_bean(name))
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
            directory: meta.is_dir(),
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
        m.set_header(
            camel_api::exchange_constants::MESSAGE_TIMESTAMP,
            self.last_modified,
        );
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

/// Whether the text has a simple start token (`${` or `$simple{`).
fn has_simple_token(text: &str) -> bool {
    text.contains("${") || text.contains("$simple{")
}

/// A `GenericFileOperationFailedException`.
pub(crate) fn operation_failed(message: impl Into<String>) -> CamelError {
    CamelError::new(
        camel_api::error::ErrorKind::GenericFileOperationFailed,
        message.into(),
    )
}
