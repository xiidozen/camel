//! The file consumer (Java `FileConsumer`, `GenericFileConsumer` and the process strategies
//! of `GenericFileOnCompletion`): polls the starting directory and routes each file.

use crate::read_lock::{self, ReadLock};
use crate::{operation_failed, FileEndpoint, GenericFile, SortBy};
use camel_api::endpoint::{Consumer, Endpoint, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, SharedProcessor};
use camel_api::service::Service;
use camel_api::spi::idempotent_repository::IdempotentRepository;
use camel_util::value::Value;
use regex::{Regex, RegexBuilder};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// The state shared by the consumer and its polling task.
#[derive(Default)]
struct State {
    stopped: AtomicBool,
    wake: Notify,
}

/// The file consumer.
pub struct FileConsumer {
    endpoint: Arc<FileEndpoint>,
    processor: SharedProcessor,
    state: Arc<State>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl FileConsumer {
    pub(crate) fn new(endpoint: Arc<FileEndpoint>, processor: SharedProcessor) -> Self {
        FileConsumer {
            endpoint,
            processor,
            state: Arc::new(State::default()),
            task: Mutex::new(None),
        }
    }
}

/// The compiled filters of the endpoint.
pub(crate) struct Filters {
    include: Option<Regex>,
    exclude: Option<Regex>,
    include_ext: Vec<String>,
    exclude_ext: Vec<String>,
}

fn pattern(p: &Option<String>) -> CamelResult<Option<Regex>> {
    p.as_ref()
        .map(|p| {
            RegexBuilder::new(&format!("^(?:{p})$"))
                .case_insensitive(true)
                .build()
                .map_err(|e| CamelError::illegal_argument(format!("Invalid pattern {p}: {e}")))
        })
        .transpose()
}

fn extensions(e: &Option<String>) -> Vec<String> {
    e.as_deref()
        .map(|e| {
            e.split(',')
                .map(|x| x.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl Filters {
    pub(crate) fn of(endpoint: &FileEndpoint) -> CamelResult<Self> {
        let c = &endpoint.config;
        Ok(Filters {
            include: pattern(&c.include)?,
            exclude: pattern(&c.exclude)?,
            include_ext: extensions(&c.include_ext),
            exclude_ext: extensions(&c.exclude_ext),
        })
    }

    /// Whether the file name passes the include and exclude filters (Java `isMatched`).
    fn matches(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        let has_ext = |exts: &[String]| exts.iter().any(|e| lower.ends_with(&format!(".{e}")));
        if let Some(r) = &self.exclude {
            if r.is_match(name) {
                return false;
            }
        }
        if !self.exclude_ext.is_empty() && has_ext(&self.exclude_ext) {
            return false;
        }
        if let Some(r) = &self.include {
            if !r.is_match(name) {
                return false;
            }
        }
        if !self.include_ext.is_empty() && !has_ext(&self.include_ext) {
            return false;
        }
        true
    }
}

/// Lists the candidate files of the directory (sorted by name), descending into the sub
/// directories with `recursive`.
fn list_files(
    endpoint: &FileEndpoint,
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let c = &endpoint.config;
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let dir = path.is_dir();
        // hidden files and directories are skipped unless included (Java isValidFile)
        let included = if dir {
            c.include_hidden_dirs
        } else {
            c.include_hidden_files
        };
        if name.starts_with('.') && !included {
            continue;
        }
        if dir {
            if c.recursive && depth < c.max_depth {
                list_files(endpoint, &path, depth + 1, out)?;
            }
        } else if !name.ends_with(".camelLock") && depth >= c.min_depth && depth <= c.max_depth {
            out.push(path);
        }
    }
    Ok(())
}

/// The content of the file as the body: a text when valid UTF-8 (or decoded from ISO-8859-1),
/// else bytes.
fn read_body(path: &Path, charset: Option<&str>) -> CamelResult<Value> {
    let bytes = std::fs::read(path)
        .map_err(|e| operation_failed(format!("Cannot read file: {}: {e}", path.display())))?;
    let latin = charset.is_some_and(|c| {
        let c = c.to_ascii_uppercase();
        c == "ISO-8859-1" || c == "LATIN1"
    });
    if latin {
        return Ok(Value::Text(bytes.iter().map(|b| *b as char).collect()));
    }
    Ok(match String::from_utf8(bytes) {
        Ok(t) => Value::Text(t),
        Err(e) => Value::Bytes(e.into_bytes()),
    })
}

/// The path of a move or preMove expression (Java `configureMoveOrPreMoveExpression` and
/// `GenericFileExpressionRenamer`): a plain directory is relative to the directory of the file
/// (and keeps the file name); an expression result is relative to the starting directory.
fn rename_target(
    endpoint: &FileEndpoint,
    exchange: &mut Exchange,
    file: &GenericFile,
    expression: &str,
) -> CamelResult<PathBuf> {
    if !expression.contains('$') {
        let dir = PathBuf::from(expression.trim_end_matches('/'));
        let base = if dir.is_absolute() {
            dir
        } else {
            file.file_path.parent().map(|p| p.join(&dir)).unwrap_or(dir)
        };
        return Ok(base.join(file.file_name_only()));
    }
    let name = endpoint
        .expression(expression)?
        .evaluate_mut(exchange)?
        .to_string();
    let path = PathBuf::from(&name);
    Ok(if path.is_absolute() {
        path
    } else {
        Path::new(&endpoint.config.directory).join(path)
    })
}

/// Renames the file, replacing an existing one (Java `renameFile` with `copyAndDeleteOnRenameFail`).
fn rename(from: &Path, to: &Path) -> CamelResult<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            operation_failed(format!(
                "Cannot create directory: {}: {e}",
                parent.display()
            ))
        })?;
    }
    if to.exists() {
        std::fs::remove_file(to)
            .map_err(|e| operation_failed(format!("Cannot delete file: {}: {e}", to.display())))?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to).map_err(|e| {
            operation_failed(format!(
                "Cannot rename file from: {} to: {}: {e}",
                from.display(),
                to.display()
            ))
        })?;
        std::fs::remove_file(from).map_err(|e| {
            operation_failed(format!("Cannot delete file: {}: {e}", from.display()))
        })?;
    }
    Ok(())
}

/// Whether the file matches the `fileName` option (Java `FileConsumer.isMatched`): the
/// expression is evaluated with the headers of the file, and must give its name.
fn named(endpoint: &FileEndpoint, path: &Path) -> CamelResult<bool> {
    let Some(expr) = &endpoint.config.file_name else {
        return Ok(true);
    };
    let Ok(file) = GenericFile::of(&endpoint.config.directory, path) else {
        return Ok(false);
    };
    let mut e = endpoint.create_exchange();
    file.populate_headers(&mut e);
    let name = endpoint.expression(expr)?.evaluate_mut(&mut e)?.to_string();
    Ok(name == file.file_name_only() || name == file.relative_file_path)
}

/// A file taken for routing: the file, the key in the idempotent repository.
pub(crate) struct Taken {
    file: GenericFile,
    key: String,
    idempotent_key: String,
    idempotent: bool,
    lock: ReadLock,
}

/// Takes the file for routing (Java `GenericFileConsumer.processExchange` before the routing):
/// the idempotent check, the preMove, the exchange with the headers and the body; `None` when
/// the file was consumed before.
pub(crate) async fn begin(
    endpoint: &Arc<FileEndpoint>,
    path: &Path,
    stopped: Option<&AtomicBool>,
) -> CamelResult<Option<(Taken, Exchange)>> {
    let c = &endpoint.config;
    let file = GenericFile::of(&c.directory, path)
        .map_err(|e| operation_failed(format!("Cannot read file: {}: {e}", path.display())))?;
    let key = file.absolute_file_path().to_string_lossy().into_owned();
    let idempotent = c.idempotent.unwrap_or(c.noop);
    // a file in progress (taken by a polling consumer, not done yet) is skipped (Java
    // inProgressRepository)
    if !endpoint.in_progress.lock().unwrap().insert(key.clone()) {
        return Ok(None);
    }
    // the key in the idempotent repository: the idempotentKey expression, or the path
    let idempotent_key = match &c.idempotent_key {
        Some(expr) if idempotent => {
            let mut e = endpoint.create_exchange();
            file.populate_headers(&mut e);
            match endpoint
                .expression(expr)
                .and_then(|x| x.evaluate_mut(&mut e))
            {
                Ok(v) => v.to_string(),
                Err(err) => {
                    endpoint.in_progress.lock().unwrap().remove(&key);
                    return Err(err);
                }
            }
        }
        _ => key.clone(),
    };
    if idempotent && !endpoint.idempotent.add(&idempotent_key) {
        endpoint.in_progress.lock().unwrap().remove(&key);
        log::trace!(
            "This consumer is idempotent and the file has been consumed before: {idempotent_key}"
        );
        return Ok(None);
    }
    let abort = || {
        endpoint.in_progress.lock().unwrap().remove(&key);
        if idempotent {
            endpoint.idempotent.remove(&idempotent_key);
        }
    };
    // the exclusive read lock (Java processStrategy.begin)
    let lock = match read_lock::acquire(endpoint, &file, stopped).await {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            log::debug!(
                "{} cannot begin processing file: {} as the read lock was not acquired",
                endpoint.uri,
                path.display()
            );
            abort();
            return Ok(None);
        }
        Err(e) => {
            abort();
            return Err(e);
        }
    };
    // the file as locked (a changed file has its final length)
    let prepared = GenericFile::of(&c.directory, path)
        .map_err(|e| operation_failed(format!("Cannot read file: {}: {e}", path.display())))
        .and_then(|file| prepare(endpoint, file));
    match prepared {
        Ok((file, exchange)) => Ok(Some((
            Taken {
                file,
                key,
                idempotent_key,
                idempotent,
                lock,
            },
            exchange,
        ))),
        Err(e) => {
            lock.release();
            abort();
            Err(e)
        }
    }
}

/// The exchange of the taken file (with the preMove).
fn prepare(
    endpoint: &Arc<FileEndpoint>,
    mut file: GenericFile,
) -> CamelResult<(GenericFile, Exchange)> {
    let c = &endpoint.config;
    let mut exchange = endpoint.create_exchange();
    file.populate_headers(&mut exchange);
    if let Some(pre) = &c.pre_move {
        let to = rename_target(endpoint, &mut exchange, &file, pre)?;
        rename(&file.file_path, &to)?;
        file = GenericFile::of(&c.directory, &to)
            .map_err(|e| operation_failed(format!("Cannot read file: {}: {e}", to.display())))?;
        file.populate_headers(&mut exchange);
    }
    let body = read_body(&file.file_path, c.charset.as_deref())?;
    exchange.message_mut().set_body(body);
    Ok((file, exchange))
}

/// Applies the process strategy after the routing (Java `GenericFileOnCompletion`): on
/// failure the file stays (or is moved to `moveFailed`), else it is left (`noop`), deleted or
/// moved.
pub(crate) fn commit(
    endpoint: &FileEndpoint,
    taken: &Taken,
    exchange: &mut Exchange,
) -> CamelResult<()> {
    let result = complete(endpoint, taken, exchange);
    // the read lock is released last (Java GenericFileProcessStrategySupport)
    taken.lock.release();
    result
}

/// The process strategy of the done file: commit or rollback.
fn complete(endpoint: &FileEndpoint, taken: &Taken, exchange: &mut Exchange) -> CamelResult<()> {
    let c = &endpoint.config;
    let file = &taken.file;
    endpoint.in_progress.lock().unwrap().remove(&taken.key);
    let failed = exchange.is_failed() || exchange.is_rollback_only();
    if failed {
        if taken.idempotent {
            endpoint.idempotent.remove(&taken.idempotent_key);
        }
        if let Some(e) = exchange.exception() {
            log::warn!(
                "Error processing exchange. Exchange[{}]. Caused by: [{} - {e}]",
                exchange.exchange_id(),
                camel_api::error::error_type_name(e)
            );
        }
        if let Some(failed_to) = &c.move_failed {
            // the done file is kept for a retry unless the file is moved away
            delete_done_file(endpoint, taken, exchange);
            let to = rename_target(endpoint, exchange, file, failed_to)?;
            rename(&file.file_path, &to)?;
        }
        return Ok(());
    }
    delete_done_file(endpoint, taken, exchange);
    if c.noop {
        return Ok(());
    }
    if c.delete {
        if file.file_path.exists() {
            std::fs::remove_file(&file.file_path).map_err(|e| {
                operation_failed(format!(
                    "Cannot delete file: {}: {e}",
                    file.file_path.display()
                ))
            })?;
        }
        return Ok(());
    }
    let move_to = c.move_to.clone().unwrap_or_else(|| ".camel".to_string());
    // the headers of the file again, as the route may have changed them
    file.populate_headers(exchange);
    let to = rename_target(endpoint, exchange, file, &move_to)?;
    rename(&file.file_path, &to)
}

/// Routes one file and applies the process strategy.
async fn process_file(
    endpoint: &Arc<FileEndpoint>,
    processor: &SharedProcessor,
    path: PathBuf,
    batch: (usize, usize),
    state: &State,
) -> CamelResult<()> {
    let Some((taken, mut exchange)) = begin(endpoint, &path, Some(&state.stopped)).await? else {
        return Ok(());
    };
    // the batch of the poll (Java ScheduledBatchPollingConsumer)
    let (index, size) = batch;
    exchange.set_property(camel_api::exchange_constants::BATCH_INDEX, index as i64);
    exchange.set_property(camel_api::exchange_constants::BATCH_SIZE, size as i64);
    exchange.set_property(
        camel_api::exchange_constants::BATCH_COMPLETE,
        index + 1 == size,
    );
    if let Err(e) = processor.process(&mut exchange).await {
        exchange.set_exception(Some(e));
    }
    commit(endpoint, &taken, &mut exchange)
}

/// The candidate files of a poll, filtered (Java `pollDirectory` with `isValidFile`).
/// With `doneFileName`: whether the file is not a done file and its done file exists (Java
/// `isValidFile`).
fn done(endpoint: &FileEndpoint, path: &Path, name: &str) -> CamelResult<bool> {
    if endpoint.config.done_file_name.is_none() {
        return Ok(true);
    }
    if endpoint.is_done_file(name)? {
        log::trace!("Skipping done file: {}", path.display());
        return Ok(false);
    }
    let done = endpoint.create_done_file_name(&path.to_string_lossy())?;
    if Path::new(&done).exists() {
        return Ok(true);
    }
    log::trace!("Done file: {done} does not exist");
    Ok(false)
}

/// Deletes the done file of the taken file (Java `handleDoneFile`): with a dynamic done file
/// name, or for the last file of the batch.
fn delete_done_file(endpoint: &FileEndpoint, taken: &Taken, exchange: &Exchange) {
    let c = &endpoint.config;
    let Some(pattern) = &c.done_file_name else {
        return;
    };
    if c.noop {
        return;
    }
    let complete = matches!(
        exchange.property(camel_api::exchange_constants::BATCH_COMPLETE),
        Some(Value::Bool(true))
    );
    if !pattern.contains("{file:name") && !complete {
        return;
    }
    match endpoint.create_done_file_name(&taken.key) {
        Ok(done) => {
            if let Err(e) = std::fs::remove_file(&done) {
                log::warn!("Done file: {done} could not be deleted: {e}");
            }
        }
        Err(e) => log::warn!("Error deleting done file: {e}"),
    }
}

pub(crate) fn candidates(endpoint: &FileEndpoint, filters: &Filters) -> CamelResult<Vec<PathBuf>> {
    let c = &endpoint.config;
    let dir = Path::new(&c.directory);
    if !dir.is_dir() {
        if c.directory_must_exist {
            return Err(CamelError::new(
                ErrorKind::FileNotFound,
                format!("Directory does not exist: {}", c.directory),
            ));
        }
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    list_files(endpoint, dir, 1, &mut files)
        .map_err(|e| operation_failed(format!("Cannot list directory: {}: {e}", c.directory)))?;
    let mut answer = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if filters.matches(&name) && named(endpoint, &path)? && done(endpoint, &path, &name)? {
            answer.push(path);
        }
    }
    // the eager limit before sorting, else after (Java eagerMaxMessagesPerPoll)
    let limit = c.max_messages_per_poll;
    if limit > 0 && c.eager_max_messages_per_poll {
        answer.truncate(limit);
    }
    let mut answer = sort_files(endpoint, answer)?;
    if limit > 0 {
        answer.truncate(limit);
    }
    Ok(answer)
}

/// Sorts the files by the `sortBy` groups, evaluated on an exchange with the headers of each
/// file (Java `GenericFileDefaultSorter`).
fn sort_files(endpoint: &FileEndpoint, files: Vec<PathBuf>) -> CamelResult<Vec<PathBuf>> {
    let groups = &endpoint.config.sort_by;
    if groups.is_empty() || files.len() < 2 {
        return Ok(files);
    }
    let expressions = groups
        .iter()
        .map(|g| endpoint.expression(&g.expression))
        .collect::<CamelResult<Vec<_>>>()?;
    let mut keyed = Vec::with_capacity(files.len());
    for path in files {
        let file = GenericFile::of(&endpoint.config.directory, &path)
            .map_err(|e| operation_failed(format!("Cannot read file: {}: {e}", path.display())))?;
        let mut exchange = endpoint.create_exchange();
        file.populate_headers(&mut exchange);
        let keys = expressions
            .iter()
            .map(|e| e.evaluate_mut(&mut exchange))
            .collect::<CamelResult<Vec<_>>>()?;
        keyed.push((keys, path));
    }
    keyed.sort_by(|(a, _), (b, _)| compare_groups(groups, a, b, 0));
    Ok(keyed.into_iter().map(|(_, p)| p).collect())
}

/// Compares the keys from the group at the index: the nested groups decide the equal keys,
/// and a reversed group reverses the nested result too (as Java).
fn compare_groups(groups: &[SortBy], a: &[Value], b: &[Value], i: usize) -> std::cmp::Ordering {
    let Some(g) = groups.get(i) else {
        return std::cmp::Ordering::Equal;
    };
    let mut o = camel_support::object_helper::compare(&a[i], &b[i], g.ignore_case);
    if o == std::cmp::Ordering::Equal {
        o = compare_groups(groups, a, b, i + 1);
    }
    if g.reverse {
        o.reverse()
    } else {
        o
    }
}

/// One poll: routes the files of the starting directory (Java `GenericFileConsumer.poll`).
async fn poll(
    endpoint: &Arc<FileEndpoint>,
    processor: &SharedProcessor,
    filters: &Filters,
    state: &State,
) -> CamelResult<usize> {
    let c = &endpoint.config;
    let files = candidates(endpoint, filters)?;
    let size = files.len();
    let mut count = 0;
    for (index, path) in files.into_iter().enumerate() {
        if state.stopped.load(Ordering::SeqCst) {
            break;
        }
        count += 1;
        if let Err(e) = process_file(endpoint, processor, path, (index, size), state).await {
            log::warn!("Error processing file: {e}");
        }
    }
    // an empty message when idle (Java sendEmptyMessageWhenIdle)
    if size == 0 && c.send_empty_message_when_idle && !state.stopped.load(Ordering::SeqCst) {
        let mut exchange = endpoint.create_exchange();
        if let Err(e) = processor.process(&mut exchange).await {
            exchange.set_exception(Some(e));
        }
        if let Some(e) = exchange.exception() {
            log::warn!("Error processing exchange: {e}");
        }
    }
    Ok(count)
}

async fn sleep(state: &State, millis: u64) -> bool {
    if state.stopped.load(Ordering::SeqCst) {
        return false;
    }
    if millis > 0 {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(millis)) => {}
            _ = state.wake.notified() => {}
        }
    }
    !state.stopped.load(Ordering::SeqCst)
}

async fn run(
    endpoint: Arc<FileEndpoint>,
    processor: SharedProcessor,
    filters: Filters,
    state: Arc<State>,
) {
    if !sleep(&state, endpoint.config.initial_delay).await {
        return;
    }
    loop {
        if let Err(e) = poll(&endpoint, &processor, &filters, &state).await {
            log::warn!("Consumer {} failed polling endpoint: {}. Will try again at next poll. Caused by: [{e}]",
                endpoint.uri, endpoint.uri);
        }
        if !sleep(&state, endpoint.config.delay.max(1)).await {
            break;
        }
    }
}

impl Service for FileConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            if tokio::runtime::Handle::try_current().is_err() {
                return Err(CamelError::illegal_state(
                    "The file consumer must be started within a tokio runtime",
                ));
            }
            let c = &self.endpoint.config;
            let dir = Path::new(&c.directory);
            if !dir.is_dir() {
                if c.starting_directory_must_exist {
                    return Err(CamelError::new(
                        ErrorKind::FileNotFound,
                        format!("Starting directory does not exist: {}", c.directory),
                    ));
                }
                if c.auto_create {
                    std::fs::create_dir_all(dir).map_err(|e| {
                        operation_failed(format!("Cannot create directory: {}: {e}", c.directory))
                    })?;
                }
            }
            let filters = Filters::of(&self.endpoint)?;
            read_lock::delete_orphan_lock_files(&self.endpoint);
            let mut task = self.task.lock().unwrap();
            if task.is_some() {
                return Ok(());
            }
            self.state.stopped.store(false, Ordering::SeqCst);
            *task = Some(tokio::spawn(run(
                self.endpoint.clone(),
                self.processor.clone(),
                filters,
                self.state.clone(),
            )));
            Ok(())
        })
    }

    /// Stops polling, waiting for the file in flight.
    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.state.stopped.store(true, Ordering::SeqCst);
            self.state.wake.notify_waiters();
            let task = self.task.lock().unwrap().take();
            if let Some(t) = task {
                let _ = t.await;
            }
            Ok(())
        })
    }
}

impl Consumer for FileConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}

/// Applies the process strategy of the polled file when its exchange is done.
struct FileCompletion {
    endpoint: Arc<FileEndpoint>,
    taken: Taken,
}

impl camel_api::spi::synchronization::Synchronization for FileCompletion {
    fn on_complete<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Err(e) = commit(&self.endpoint, &self.taken, exchange) {
                log::warn!(
                    "Error committing the file {}: {e}",
                    self.taken.file.file_path.display()
                );
            }
        })
    }

    fn on_failure<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        self.on_complete(exchange)
    }
}

/// The polling consumer of a file endpoint (Java `GenericFilePollingConsumer`): receives one
/// file at a time; the process strategy (move, delete, ...) runs when the exchange is done.
pub struct FilePollingConsumer {
    endpoint: Arc<FileEndpoint>,
}

impl FilePollingConsumer {
    pub(crate) fn new(endpoint: Arc<FileEndpoint>) -> Self {
        FilePollingConsumer { endpoint }
    }

    async fn poll_one(&self) -> CamelResult<Option<Exchange>> {
        let filters = Filters::of(&self.endpoint)?;
        for path in candidates(&self.endpoint, &filters)? {
            if let Some((taken, mut exchange)) = begin(&self.endpoint, &path, None).await? {
                exchange.add_on_completion(Arc::new(FileCompletion {
                    endpoint: self.endpoint.clone(),
                    taken,
                }));
                return Ok(Some(exchange));
            }
        }
        Ok(None)
    }

    async fn poll_until(
        &self,
        deadline: Option<tokio::time::Instant>,
    ) -> CamelResult<Option<Exchange>> {
        loop {
            if let Some(e) = self.poll_one().await? {
                return Ok(Some(e));
            }
            let wait = Duration::from_millis(self.endpoint.config.delay.clamp(1, 100));
            match deadline {
                Some(d) if tokio::time::Instant::now() + wait > d => return Ok(None),
                _ => tokio::time::sleep(wait).await,
            }
        }
    }
}

impl Service for FilePollingConsumer {}

impl camel_api::endpoint::PollingConsumer for FilePollingConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn receive(&self) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(self.poll_until(None))
    }

    fn receive_no_wait(&self) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(self.poll_one())
    }

    fn receive_timeout(&self, timeout: Duration) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(self.poll_until(Some(tokio::time::Instant::now() + timeout)))
    }
}
