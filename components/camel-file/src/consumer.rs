//! The file consumer (Java `FileConsumer`, `GenericFileConsumer` and the process strategies
//! of `GenericFileOnCompletion`): polls the starting directory and routes each file.

use crate::{operation_failed, FileEndpoint, GenericFile};
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
        if name.starts_with('.') && !c.include_hidden_files {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
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
    idempotent: bool,
}

/// Takes the file for routing (Java `GenericFileConsumer.processExchange` before the routing):
/// the idempotent check, the preMove, the exchange with the headers and the body; `None` when
/// the file was consumed before.
pub(crate) fn begin(
    endpoint: &Arc<FileEndpoint>,
    path: &Path,
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
    if idempotent && !endpoint.idempotent.add(&key) {
        endpoint.in_progress.lock().unwrap().remove(&key);
        log::trace!("This consumer is idempotent and the file has been consumed before: {key}");
        return Ok(None);
    }
    match prepare(endpoint, file, &key, idempotent) {
        Ok(r) => Ok(Some(r)),
        Err(e) => {
            endpoint.in_progress.lock().unwrap().remove(&key);
            if idempotent {
                endpoint.idempotent.remove(&key);
            }
            Err(e)
        }
    }
}

/// The exchange of the taken file (with the preMove).
fn prepare(
    endpoint: &Arc<FileEndpoint>,
    mut file: GenericFile,
    key: &str,
    idempotent: bool,
) -> CamelResult<(Taken, Exchange)> {
    let c = &endpoint.config;
    let key = key.to_string();
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
    Ok((
        Taken {
            file,
            key,
            idempotent,
        },
        exchange,
    ))
}

/// Applies the process strategy after the routing (Java `GenericFileOnCompletion`): on
/// failure the file stays (or is moved to `moveFailed`), else it is left (`noop`), deleted or
/// moved.
pub(crate) fn commit(
    endpoint: &FileEndpoint,
    taken: &Taken,
    exchange: &mut Exchange,
) -> CamelResult<()> {
    let c = &endpoint.config;
    let file = &taken.file;
    endpoint.in_progress.lock().unwrap().remove(&taken.key);
    let failed = exchange.is_failed() || exchange.is_rollback_only();
    if failed {
        if taken.idempotent {
            endpoint.idempotent.remove(&taken.key);
        }
        if let Some(e) = exchange.exception() {
            log::warn!(
                "Error processing exchange. Exchange[{}]. Caused by: [{} - {e}]",
                exchange.exchange_id(),
                camel_api::error::error_type_name(e)
            );
        }
        if let Some(failed_to) = &c.move_failed {
            let to = rename_target(endpoint, exchange, file, failed_to)?;
            rename(&file.file_path, &to)?;
        }
        return Ok(());
    }
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
) -> CamelResult<()> {
    let Some((taken, mut exchange)) = begin(endpoint, &path)? else {
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
    if c.sort_reverse {
        files.reverse();
    }
    let mut answer = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if filters.matches(&name) && named(endpoint, &path)? {
            answer.push(path);
        }
    }
    Ok(answer)
}

/// One poll: routes the files of the starting directory (Java `GenericFileConsumer.poll`).
async fn poll(
    endpoint: &Arc<FileEndpoint>,
    processor: &SharedProcessor,
    filters: &Filters,
    state: &State,
) -> CamelResult<usize> {
    let c = &endpoint.config;
    let mut files = candidates(endpoint, filters)?;
    if c.max_messages_per_poll > 0 {
        files.truncate(c.max_messages_per_poll);
    }
    let size = files.len();
    let mut count = 0;
    for (index, path) in files.into_iter().enumerate() {
        if state.stopped.load(Ordering::SeqCst) {
            break;
        }
        count += 1;
        if let Err(e) = process_file(endpoint, processor, path, (index, size)).await {
            log::warn!("Error processing file: {e}");
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

    fn poll_one(&self) -> CamelResult<Option<Exchange>> {
        let filters = Filters::of(&self.endpoint)?;
        for path in candidates(&self.endpoint, &filters)? {
            if let Some((taken, mut exchange)) = begin(&self.endpoint, &path)? {
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
            if let Some(e) = self.poll_one()? {
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
        Box::pin(std::future::ready(self.poll_one()))
    }

    fn receive_timeout(&self, timeout: Duration) -> BoxFuture<'_, CamelResult<Option<Exchange>>> {
        Box::pin(self.poll_until(Some(tokio::time::Instant::now() + timeout)))
    }
}
