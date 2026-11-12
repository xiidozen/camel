//! The exclusive read locks of the file consumer (Java `GenericFileExclusiveReadLockStrategy`
//! and its file implementations): a file is only taken once its read lock is acquired, and the
//! lock is released when the file is done (commit, rollback or abort).
//!
//! - `none`: no lock.
//! - `markerFile`: a `<file>.camelLock` marker created next to the file, failing if it exists
//!   (Java `MarkerFileExclusiveReadLockStrategy`).
//! - `changed`: the marker file, then waits until the length and the last modified of the file
//!   no longer change between two checks (or the file is older than `readLockMinAge`), the
//!   file being at least `readLockMinLength` long (Java `FileChangedExclusiveReadLockStrategy`).
//! - `rename`: the marker file, then renames the file to itself (with a temporary name) until
//!   it succeeds (Java `FileRenameExclusiveReadLockStrategy`).
//! - `idempotent`: the key of the file (its path, or the `idempotentKey`) added to the
//!   `idempotentRepository` of the endpoint, failing if present (for clustered consumers
//!   sharing the repository); `idempotent-changed` and `idempotent-rename` also wait as
//!   `changed` and `rename` (without marker file). On commit the key is confirmed (removed with
//!   `readLockRemoveOnCommit`), on rollback removed (confirmed with
//!   `readLockRemoveOnRollback=false`), after `readLockIdempotentReleaseDelay` millis (Java
//!   `FileIdempotentRepositoryReadLockStrategy` and its variants).
//!
//! The waiting locks give up after `readLockTimeout` millis (`0`: no limit), checking every
//! `readLockCheckInterval` millis. With `readLockMarkerFile=false` no marker file is used, and
//! the orphan marker files of the starting directory are deleted when the consumer starts
//! (`readLockDeleteOrphanLockFiles`, default `true`).

use crate::{operation_failed, FileEndpoint, GenericFile};
use camel_api::error::{CamelError, CamelResult};
use camel_api::spi::idempotent_repository::SharedIdempotentRepository;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// The postfix of the marker files (Java `FileComponent.DEFAULT_LOCK_FILE_POSTFIX`).
pub const DEFAULT_LOCK_FILE_POSTFIX: &str = ".camelLock";

/// The kind of read lock (the `readLock` option).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadLockKind {
    /// No read lock (the default).
    #[default]
    None,
    /// A marker file.
    MarkerFile,
    /// The marker file and a file no longer changing.
    Changed,
    /// The marker file and a rename of the file.
    Rename,
    /// The key of the file in the idempotent repository.
    Idempotent,
    /// The idempotent repository and a file no longer changing.
    IdempotentChanged,
    /// The idempotent repository and a rename of the file.
    IdempotentRename,
}

impl ReadLockKind {
    /// Whether the lock is a key in the idempotent repository.
    pub fn is_idempotent(self) -> bool {
        matches!(
            self,
            ReadLockKind::Idempotent
                | ReadLockKind::IdempotentChanged
                | ReadLockKind::IdempotentRename
        )
    }
}

impl std::str::FromStr for ReadLockKind {
    type Err = CamelError;

    fn from_str(s: &str) -> CamelResult<Self> {
        match s {
            "none" => Ok(ReadLockKind::None),
            "markerFile" => Ok(ReadLockKind::MarkerFile),
            "changed" => Ok(ReadLockKind::Changed),
            "rename" => Ok(ReadLockKind::Rename),
            "idempotent" => Ok(ReadLockKind::Idempotent),
            "idempotent-changed" => Ok(ReadLockKind::IdempotentChanged),
            "idempotent-rename" => Ok(ReadLockKind::IdempotentRename),
            "fileLock" => Err(CamelError::illegal_argument(
                "The readLock fileLock is not available in the Rust port yet",
            )),
            other => Err(CamelError::illegal_argument(format!(
                "Invalid readLock option: {other}"
            ))),
        }
    }
}

/// The read lock options of an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadLockOptions {
    /// The kind (`readLock`).
    pub kind: ReadLockKind,
    /// The millis to wait for the lock, `0` without limit (`readLockTimeout`, default 10000).
    pub timeout: u64,
    /// The millis between two checks (`readLockCheckInterval`, default 1000).
    pub check_interval: u64,
    /// The minimum length of a changed file (`readLockMinLength`, default 1).
    pub min_length: u64,
    /// The minimum age in millis of a changed file, `0` to compare two checks
    /// (`readLockMinAge`, default 0).
    pub min_age: u64,
    /// Whether the marker file is used (`readLockMarkerFile`, default `true`).
    pub marker_file: bool,
    /// Whether the orphan marker files are deleted on start (`readLockDeleteOrphanLockFiles`,
    /// default `true`).
    pub delete_orphan_lock_files: bool,
    /// The level of the logs of a lock not acquired (`readLockLoggingLevel`, default `DEBUG`;
    /// `None` for `OFF`).
    pub logging_level: Option<log::Level>,
    /// Whether an idempotent lock is removed on commit, else confirmed
    /// (`readLockRemoveOnCommit`, default `false`).
    pub remove_on_commit: bool,
    /// Whether an idempotent lock is removed on rollback, else confirmed
    /// (`readLockRemoveOnRollback`, default `true`).
    pub remove_on_rollback: bool,
    /// The millis before an idempotent lock is released (`readLockIdempotentReleaseDelay`,
    /// default 0); the consumer waits them unless `readLockIdempotentReleaseAsync`.
    pub idempotent_release_delay: u64,
    /// Whether the delayed release does not hold the consumer
    /// (`readLockIdempotentReleaseAsync`, default `false`).
    pub idempotent_release_async: bool,
}

impl Default for ReadLockOptions {
    fn default() -> Self {
        ReadLockOptions {
            kind: ReadLockKind::None,
            timeout: 10000,
            check_interval: 1000,
            min_length: 1,
            min_age: 0,
            marker_file: true,
            delete_orphan_lock_files: true,
            logging_level: Some(log::Level::Debug),
            remove_on_commit: false,
            remove_on_rollback: true,
            idempotent_release_delay: 0,
            idempotent_release_async: false,
        }
    }
}

impl ReadLockOptions {
    /// Whether the kind uses the marker file.
    fn uses_marker(&self) -> bool {
        matches!(
            self.kind,
            ReadLockKind::MarkerFile | ReadLockKind::Changed | ReadLockKind::Rename
        ) && self.marker_file
    }

    /// The millis the consumer waits after a file for a delayed release (Java sleeps before
    /// releasing an idempotent lock).
    pub(crate) fn release_wait(&self) -> u64 {
        if self.kind.is_idempotent() && !self.idempotent_release_async {
            self.idempotent_release_delay
        } else {
            0
        }
    }
}

/// How the file was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Routed.
    Commit,
    /// Failed.
    Rollback,
    /// Not routed (the begin failed).
    Abort,
}

/// An acquired read lock: the marker file to delete, the idempotent key to release.
#[derive(Default)]
pub(crate) struct ReadLock {
    marker: Option<PathBuf>,
    idempotent: Option<(SharedIdempotentRepository, String)>,
    options: Option<ReadLockOptions>,
}

impl ReadLock {
    /// Releases the lock: deletes the marker file, removes or confirms the idempotent key
    /// (after the release delay, on a task).
    pub(crate) fn release(&self, outcome: Outcome) {
        if let Some(marker) = &self.marker {
            if marker.exists() {
                log::trace!("Unlocking file: {}", marker.display());
                let _ = std::fs::remove_file(marker);
            }
        }
        let (Some((repository, key)), Some(o)) = (&self.idempotent, &self.options) else {
            return;
        };
        let remove = match outcome {
            Outcome::Commit => o.remove_on_commit,
            Outcome::Rollback => o.remove_on_rollback,
            Outcome::Abort => true,
        };
        let (repository, key) = (repository.clone(), key.clone());
        let release = move || {
            if remove {
                repository.remove(&key);
            } else {
                repository.confirm(&key);
            }
        };
        let delay = o.idempotent_release_delay;
        if delay > 0 && outcome != Outcome::Abort && tokio::runtime::Handle::try_current().is_ok() {
            log::debug!("Scheduling read lock release task to run asynchronous delayed after {delay} millis");
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                release();
            });
        } else {
            release();
        }
    }
}

/// The marker file of the file.
fn marker_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(DEFAULT_LOCK_FILE_POSTFIX);
    PathBuf::from(name)
}

/// Acquires the read lock of the file (Java `acquireExclusiveReadLock`).
///
/// # Returns
///
/// The lock, `None` when it could not be acquired (the file is skipped).
/// The waiting stops when the `stopped` flag of the consumer is set (Java interrupts the
/// sleep).
pub(crate) async fn acquire(
    endpoint: &FileEndpoint,
    file: &GenericFile,
    key: &str,
    stopped: Option<&AtomicBool>,
) -> CamelResult<Option<ReadLock>> {
    let o = &endpoint.config.read_lock;
    let mut lock = ReadLock {
        options: Some(o.clone()),
        ..ReadLock::default()
    };
    if o.kind.is_idempotent() {
        if !file.file_path.exists() {
            return Ok(None);
        }
        let repository = endpoint.idempotent.clone();
        if !repository.add(key) {
            if let Some(level) = o.logging_level {
                log::log!(
                    level,
                    "Cannot acquire read lock. Will skip the file: {}",
                    file.file_path.display()
                );
            }
            return Ok(None);
        }
        lock.idempotent = Some((repository, key.to_string()));
    }
    if o.uses_marker() {
        let marker = marker_of(&file.file_path);
        log::trace!(
            "Locking the file: {} using the lock file name: {}",
            file.file_path.display(),
            marker.display()
        );
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
            Ok(_) => lock.marker = Some(marker),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
            Err(e) => {
                return Err(operation_failed(format!(
                    "Cannot create lock file: {}: {e}",
                    marker.display()
                )))
            }
        }
    }
    let acquired = match o.kind {
        ReadLockKind::None | ReadLockKind::MarkerFile | ReadLockKind::Idempotent => Ok(true),
        ReadLockKind::Changed | ReadLockKind::IdempotentChanged => {
            Ok(changed(o, &file.file_path, stopped).await)
        }
        ReadLockKind::Rename | ReadLockKind::IdempotentRename => {
            renamed(o, &file.file_path, stopped).await
        }
    };
    match acquired {
        Ok(true) => Ok(Some(lock)),
        Ok(false) => {
            lock.release(Outcome::Abort);
            Ok(None)
        }
        Err(e) => {
            lock.release(Outcome::Abort);
            Err(e)
        }
    }
}

/// Whether the timeout is reached.
fn timed_out(o: &ReadLockOptions, start: Instant, path: &Path) -> bool {
    if o.timeout > 0 && start.elapsed() > Duration::from_millis(o.timeout) {
        if let Some(level) = o.logging_level {
            log::log!(
                level,
                "Cannot acquire read lock within {} millis. Will skip the file: {}",
                o.timeout,
                path.display()
            );
        }
        return true;
    }
    false
}

/// Waits until the file no longer changes (Java `FileChangedExclusiveReadLockStrategy`).
async fn changed(o: &ReadLockOptions, path: &Path, stopped: Option<&AtomicBool>) -> bool {
    let start = Instant::now();
    let now_millis = || {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };
    let (mut last_modified, mut length) = (i64::MIN, i64::MIN);
    loop {
        if timed_out(o, start, path) {
            return false;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            if let Some(level) = o.logging_level {
                log::log!(
                    level,
                    "Cannot acquire read lock as file no longer exists. Will skip the file: {}",
                    path.display()
                );
            }
            return false;
        };
        let new_modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let new_length = meta.len() as i64;
        let older_than = now_millis() - o.min_age as i64;
        if new_length >= o.min_length as i64
            && ((o.min_age == 0 && new_modified == last_modified && new_length == length)
                || (o.min_age != 0 && new_modified < older_than))
        {
            log::trace!("Read lock acquired.");
            return true;
        }
        last_modified = new_modified;
        length = new_length;
        log::trace!(
            "Exclusive read lock not granted. Sleeping for {} millis.",
            o.check_interval
        );
        if !sleep(o.check_interval, stopped).await {
            return false;
        }
    }
}

/// Sleeps the millis, in steps checking the stopped flag.
///
/// # Returns
///
/// `false` if interrupted.
async fn sleep(millis: u64, stopped: Option<&AtomicBool>) -> bool {
    let is_stopped = || stopped.is_some_and(|s| s.load(Ordering::SeqCst));
    let mut remaining = millis;
    loop {
        if is_stopped() {
            log::debug!("Sleep interrupted while waiting for exclusive read lock, so breaking out");
            return false;
        }
        if remaining == 0 {
            return true;
        }
        let step = remaining.min(100);
        tokio::time::sleep(Duration::from_millis(step)).await;
        remaining -= step;
    }
}

/// Renames the file to a temporary name and back until it succeeds (Java
/// `GenericFileRenameExclusiveReadLockStrategy`).
async fn renamed(
    o: &ReadLockOptions,
    path: &Path,
    stopped: Option<&AtomicBool>,
) -> CamelResult<bool> {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".camelExclusiveReadLock");
    let temp = PathBuf::from(temp);
    let start = Instant::now();
    loop {
        if timed_out(o, start, path) {
            return Ok(false);
        }
        if std::fs::rename(path, &temp).is_ok() {
            log::trace!("Acquired exclusive read lock to file: {}", path.display());
            std::fs::rename(&temp, path).map_err(|e| {
                operation_failed(format!(
                    "Cannot rename file from: {} to: {}: {e}",
                    temp.display(),
                    path.display()
                ))
            })?;
            return Ok(true);
        }
        if !sleep(o.check_interval, stopped).await {
            return Ok(false);
        }
    }
}

/// Deletes the orphan marker files of the starting directory (Java `prepareOnStartup`).
pub(crate) fn delete_orphan_lock_files(endpoint: &FileEndpoint) {
    let c = &endpoint.config;
    if !c.read_lock.uses_marker() || !c.read_lock.delete_orphan_lock_files {
        return;
    }
    log::debug!(
        "Prepare on startup by deleting orphaned lock files from: {}",
        c.directory
    );
    delete_lock_files(endpoint, Path::new(&c.directory), 1);
}

fn delete_lock_files(endpoint: &FileEndpoint, dir: &Path, depth: usize) {
    let c = &endpoint.config;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !c.include_hidden_files && name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if name.ends_with(DEFAULT_LOCK_FILE_POSTFIX) && depth >= c.min_depth {
            log::warn!("Deleting orphaned lock file: {}", path.display());
            let _ = std::fs::remove_file(&path);
        } else if c.recursive && path.is_dir() && depth < c.max_depth {
            delete_lock_files(endpoint, &path, depth + 1);
        }
    }
}
