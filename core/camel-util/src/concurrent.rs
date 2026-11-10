//! Thread pools, schedulers and thread-scoped values. Port of the
//! `org.apache.camel.util.concurrent` package.
//!
//! Java's `java.util.concurrent` executors are replaced by small `std::thread` based
//! implementations with the same semantics Camel relies on:
//!
//! * [`ThreadPoolExecutor`] — core/max pool size, bounded or unbounded queue and the
//!   [`ThreadPoolRejectedPolicy`] (`Abort`, `CallerRuns`, `Block`), including tasks that are
//!   notified when rejected (the Java `Rejectable` / `RejectableThreadPoolExecutor`)
//! * [`ScheduledThreadPoolExecutor`] — delayed, fixed-rate and fixed-delay tasks with
//!   cancellable [`ScheduledFuture`]s, and the queue limited [`SizedScheduledExecutorService`]
//! * [`SynchronousExecutorService`] — runs every task in the caller thread
//! * [`BoundedExecutorService`] — a semaphore that caps the number of concurrently delegated
//!   tasks
//! * [`AsyncCompletionService`] — returns completed results in submission order
//! * [`ContextValue`] — a thread-scoped value with scoped binding (`where_`)
//!
//! Not ported: `LockHelper` (Rust lock guards release on drop), the deprecated
//! `NamedThreadLocal`, `RejectableFutureTask` (see [`submit`]) and `ThreadFactoryTypeAware`
//! (merged into [`CamelThreadFactory::is_virtual`]). Rust has no virtual threads; the
//! [`ThreadType`] setting is kept for configuration compatibility.

use crate::string_helper::{before, replace_first};
use regex::Regex;
use std::any::Any;
use std::cell::RefCell;
use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A unit of work for an executor.
pub type Task = Box<dyn FnOnce() + Send + 'static>;

/// Error when an executor cannot accept a task (Java `RejectedExecutionException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedExecutionError(pub String);

impl fmt::Display for RejectedExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RejectedExecutionError {}

/// An object that executes submitted tasks (Java `ExecutorService`).
pub trait Executor: Send + Sync {
    /// Executes the task at some time in the future.
    ///
    /// # Errors
    ///
    /// Returns an error if the task cannot be accepted (for example after shutdown).
    fn execute(&self, task: Task) -> Result<(), RejectedExecutionError>;

    /// Initiates an orderly shutdown: previously submitted tasks are executed, new tasks are
    /// rejected.
    fn shutdown(&self);

    /// Whether this executor has been shut down.
    fn is_shutdown(&self) -> bool;

    /// Whether all tasks have completed following shut down.
    fn is_terminated(&self) -> bool {
        self.is_shutdown()
    }

    /// Executes the task, calling `reject` instead of returning an error if the task is
    /// rejected (the Java `Rejectable` contract).
    fn execute_rejectable(&self, task: Task, reject: Box<dyn FnOnce() + Send>) {
        if self.execute(task).is_err() {
            reject();
        }
    }
}

/// The result of a task submitted with [`submit`] (Java `Future`).
pub struct FutureResult<T> {
    rx: mpsc::Receiver<thread::Result<T>>,
    done: Arc<AtomicBool>,
}

impl<T> FutureResult<T> {
    /// Waits up to the timeout for the result.
    ///
    /// # Returns
    ///
    /// `Some(Ok(value))`, `Some(Err(panic))` if the task panicked, or `None` on timeout.
    pub fn get(&self, timeout: Duration) -> Option<thread::Result<T>> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Whether the task has completed.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

/// Submits a task returning a value to the executor.
///
/// # Errors
///
/// Returns an error if the executor rejects the task.
pub fn submit<E, T, F>(executor: &E, f: F) -> Result<FutureResult<T>, RejectedExecutionError>
where
    E: Executor + ?Sized,
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let d = done.clone();
    executor.execute(Box::new(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        d.store(true, Ordering::SeqCst);
        let _ = tx.send(r);
    }))?;
    Ok(FutureResult { rx, done })
}

// ---------------------------------------------------------------------------------------
// thread naming

/// The default thread name pattern.
pub const DEFAULT_PATTERN: &str = "Camel Thread ##counter# - #name#";

static THREAD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Creates a new thread name with the given pattern (Java `ThreadHelper.resolveThreadName`).
///
/// The pattern supports `#name#` (the name without any `?query`), `#longName#` (the full name)
/// and `#counter#` (a unique, increasing number).
///
/// # Errors
///
/// Returns an error if any other `#word#` token is left in the resolved name.
///
/// # Examples
///
/// ```
/// use camel_util::concurrent::resolve_thread_name;
/// let name = resolve_thread_name(None, "seda://foo?size=10").unwrap();
/// assert!(name.starts_with("Camel Thread #") && name.ends_with(" - seda://foo"));
/// assert!(resolve_thread_name(Some("#foo# #name#"), "x").is_err());
/// ```
pub fn resolve_thread_name(pattern: Option<&str>, name: &str) -> Result<String, String> {
    static INVALID: OnceLock<Regex> = OnceLock::new();
    let invalid = INVALID.get_or_init(|| Regex::new(r"(?s)^.*#\w+#.*$").unwrap());
    let pattern = pattern.unwrap_or(DEFAULT_PATTERN);
    let short_name = if name.contains('?') {
        before(name, "?").unwrap()
    } else {
        name.to_string()
    };
    let mut answer = replace_first(pattern, "#longName#", name);
    answer = replace_first(&answer, "#name#", &short_name);
    let next = (THREAD_COUNTER.fetch_add(1, Ordering::SeqCst) + 1).to_string();
    answer = replace_first(&answer, "#counter#", &next);
    if invalid.is_match(&answer) {
        return Err(format!(
            "Pattern is invalid: [{pattern}] in resolved thread name: [{answer}]"
        ));
    }
    Ok(answer)
}

/// The type of threads (Java platform or virtual threads).
///
/// Rust only has platform (OS) threads; the setting is kept so `camel.threads.virtual.enabled`
/// / `camel.main.virtualThreadsEnabled` keep being accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadType {
    /// Operating system threads.
    Platform,
    /// Virtual threads (not available in Rust; configuring it has no effect on the threads).
    Virtual,
}

static VIRTUAL_ENABLED: OnceLock<Mutex<Option<ThreadType>>> = OnceLock::new();

impl ThreadType {
    /// The configured thread type (from the `camel.threads.virtual.enabled` system property,
    /// resolved lazily on first access).
    pub fn current() -> ThreadType {
        let cell = VIRTUAL_ENABLED.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        *guard.get_or_insert_with(|| {
            if crate::object_helper::get_system_property_bool(
                "camel.threads.virtual.enabled",
                false,
            ) {
                ThreadType::Virtual
            } else {
                ThreadType::Platform
            }
        })
    }

    /// Directly enables virtual threads by setting the cached type to [`ThreadType::Virtual`].
    pub fn enable() {
        let cell = VIRTUAL_ENABLED.get_or_init(|| Mutex::new(None));
        *cell.lock().unwrap() = Some(ThreadType::Virtual);
    }
}

/// Thread factory which creates threads with names from a naming pattern.
#[derive(Debug, Clone)]
pub struct CamelThreadFactory {
    pattern: Option<String>,
    name: String,
    daemon: bool,
}

impl CamelThreadFactory {
    /// Creates the factory.
    ///
    /// # Arguments
    ///
    /// * `pattern` - the thread name pattern (see [`resolve_thread_name`])
    /// * `name` - the name used in the pattern
    /// * `daemon` - kept for compatibility: Rust threads never keep the process alive
    pub fn new(pattern: Option<&str>, name: &str, daemon: bool) -> Self {
        CamelThreadFactory {
            pattern: pattern.map(String::from),
            name: name.to_string(),
            daemon,
        }
    }

    /// The name used in the thread names.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the threads are daemon threads (informational).
    pub fn is_daemon(&self) -> bool {
        self.daemon
    }

    /// Whether the created threads are virtual (always `false` in Rust).
    pub fn is_virtual(&self) -> bool {
        false
    }

    /// Creates and starts a thread running the closure.
    ///
    /// # Errors
    ///
    /// Returns an error if the name pattern is invalid or the OS cannot create the thread.
    pub fn new_thread<F, T>(&self, f: F) -> std::io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let name = resolve_thread_name(self.pattern.as_deref(), &self.name)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        thread::Builder::new().name(name).spawn(f)
    }
}

impl fmt::Display for CamelThreadFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CamelThreadFactory[{}]", self.name)
    }
}

// ---------------------------------------------------------------------------------------
// thread pool

/// The kinds of options for rejection handlers for thread pools. Camel uses `CallerRuns` by
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThreadPoolRejectedPolicy {
    /// Reject the task (a rejectable task is notified, otherwise an error is returned).
    Abort,
    /// Run the task in the caller thread.
    #[default]
    CallerRuns,
    /// Block the caller until there is room in the queue.
    Block,
}

impl std::str::FromStr for ThreadPoolRejectedPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Abort" => Ok(Self::Abort),
            "CallerRuns" => Ok(Self::CallerRuns),
            "Block" => Ok(Self::Block),
            _ => Err(format!("Unknown ThreadPoolRejectedPolicy: {s}")),
        }
    }
}

impl fmt::Display for ThreadPoolRejectedPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Abort => "Abort",
            Self::CallerRuns => "CallerRuns",
            Self::Block => "Block",
        })
    }
}

struct PoolState {
    queue: VecDeque<Task>,
    threads: usize,
    idle: usize,
    active: usize,
    completed: u64,
    largest: usize,
    shutdown: bool,
}

struct PoolInner {
    core: usize,
    max: usize,
    keep_alive: Duration,
    capacity: Option<usize>,
    policy: ThreadPoolRejectedPolicy,
    factory: CamelThreadFactory,
    state: Mutex<PoolState>,
    work: Condvar,
    space: Condvar,
    terminated: Condvar,
}

/// A thread pool with core and maximum pool size, a bounded or unbounded work queue and a
/// rejection policy (Java `ThreadPoolExecutor` / `RejectableThreadPoolExecutor`).
///
/// As in Java, new tasks first start core threads, then go to the queue, and when the queue is
/// full more threads (up to the maximum) are started; beyond that the rejection policy applies.
/// Threads above the core size stop after being idle for the keep alive time.
///
/// # Examples
///
/// ```
/// use camel_util::concurrent::{submit, ThreadPoolExecutor, ThreadPoolRejectedPolicy};
/// use std::time::Duration;
/// let pool = ThreadPoolExecutor::new(2, 4, Duration::from_secs(60), Some(10), ThreadPoolRejectedPolicy::CallerRuns, "test");
/// let f = submit(&pool, || 21 * 2).unwrap();
/// assert_eq!(42, f.get(Duration::from_secs(5)).unwrap().unwrap());
/// ```
#[derive(Clone)]
pub struct ThreadPoolExecutor {
    inner: Arc<PoolInner>,
}

impl ThreadPoolExecutor {
    /// Creates the pool.
    ///
    /// # Arguments
    ///
    /// * `core` - the number of threads to keep in the pool
    /// * `max` - the maximum number of threads
    /// * `keep_alive` - how long threads above the core size wait for work before stopping
    /// * `queue_capacity` - the work queue size (`None` for unbounded)
    /// * `policy` - what to do when the pool and queue are full
    /// * `name` - the name used for the thread names
    pub fn new(
        core: usize,
        max: usize,
        keep_alive: Duration,
        queue_capacity: Option<usize>,
        policy: ThreadPoolRejectedPolicy,
        name: &str,
    ) -> Self {
        Self::with_factory(
            core,
            max,
            keep_alive,
            queue_capacity,
            policy,
            CamelThreadFactory::new(None, name, true),
        )
    }

    /// Creates the pool with the thread factory.
    pub fn with_factory(
        core: usize,
        max: usize,
        keep_alive: Duration,
        queue_capacity: Option<usize>,
        policy: ThreadPoolRejectedPolicy,
        factory: CamelThreadFactory,
    ) -> Self {
        ThreadPoolExecutor {
            inner: Arc::new(PoolInner {
                core,
                max: max.max(core).max(1),
                keep_alive,
                capacity: queue_capacity,
                policy,
                factory,
                state: Mutex::new(PoolState {
                    queue: VecDeque::new(),
                    threads: 0,
                    idle: 0,
                    active: 0,
                    completed: 0,
                    largest: 0,
                    shutdown: false,
                }),
                work: Condvar::new(),
                space: Condvar::new(),
                terminated: Condvar::new(),
            }),
        }
    }

    /// A fixed size pool with an unbounded queue (Java `Executors.newFixedThreadPool`).
    pub fn fixed(threads: usize, name: &str) -> Self {
        Self::new(
            threads,
            threads,
            Duration::from_secs(60),
            None,
            ThreadPoolRejectedPolicy::Abort,
            name,
        )
    }

    /// A pool that starts threads as needed and reuses idle ones (Java
    /// `Executors.newCachedThreadPool`).
    pub fn cached(name: &str) -> Self {
        Self::new(
            0,
            usize::MAX,
            Duration::from_secs(60),
            Some(0),
            ThreadPoolRejectedPolicy::Abort,
            name,
        )
    }

    fn spawn_worker(&self, state: &mut PoolState, first: Option<Task>) -> bool {
        let inner = self.inner.clone();
        state.threads += 1;
        state.largest = state.largest.max(state.threads);
        let spawned = self
            .inner
            .factory
            .new_thread(move || worker_loop(inner, first));
        if spawned.is_err() {
            state.threads -= 1;
            return false;
        }
        true
    }

    /// The number of threads currently executing tasks.
    pub fn active_count(&self) -> usize {
        self.inner.state.lock().unwrap().active
    }

    /// The number of threads in the pool.
    pub fn pool_size(&self) -> usize {
        self.inner.state.lock().unwrap().threads
    }

    /// The largest number of threads that have ever been in the pool.
    pub fn largest_pool_size(&self) -> usize {
        self.inner.state.lock().unwrap().largest
    }

    /// The number of completed tasks.
    pub fn completed_task_count(&self) -> u64 {
        self.inner.state.lock().unwrap().completed
    }

    /// The number of tasks waiting in the queue.
    pub fn queue_size(&self) -> usize {
        self.inner.state.lock().unwrap().queue.len()
    }

    /// The core pool size.
    pub fn core_pool_size(&self) -> usize {
        self.inner.core
    }

    /// The maximum pool size.
    pub fn maximum_pool_size(&self) -> usize {
        self.inner.max
    }

    /// The keep alive time of threads above the core size.
    pub fn keep_alive_time(&self) -> Duration {
        self.inner.keep_alive
    }

    /// The rejection policy.
    pub fn rejected_policy(&self) -> ThreadPoolRejectedPolicy {
        self.inner.policy
    }

    /// Shuts down and removes the queued tasks, returning them (Java `shutdownNow`).
    pub fn shutdown_now(&self) -> Vec<Task> {
        let mut s = self.inner.state.lock().unwrap();
        s.shutdown = true;
        let tasks: Vec<Task> = s.queue.drain(..).collect();
        self.inner.work.notify_all();
        self.inner.space.notify_all();
        tasks
    }

    /// Waits up to the timeout for all threads to finish after shutdown.
    ///
    /// # Returns
    ///
    /// `true` if terminated, `false` on timeout.
    pub fn await_termination(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = self.inner.state.lock().unwrap();
        while !(s.shutdown && s.threads == 0) {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            s = self
                .inner
                .terminated
                .wait_timeout(s, deadline - now)
                .unwrap()
                .0;
        }
        true
    }
}

fn worker_loop(inner: Arc<PoolInner>, first: Option<Task>) {
    let mut next = first;
    loop {
        let task = match next.take() {
            Some(t) => t,
            None => {
                let mut s = inner.state.lock().unwrap();
                loop {
                    if let Some(t) = s.queue.pop_front() {
                        inner.space.notify_one();
                        break t;
                    }
                    if s.shutdown {
                        s.threads -= 1;
                        if s.threads == 0 {
                            inner.terminated.notify_all();
                        }
                        return;
                    }
                    s.idle += 1;
                    let (guard, timeout) = inner.work.wait_timeout(s, inner.keep_alive).unwrap();
                    s = guard;
                    s.idle -= 1;
                    if timeout.timed_out() && s.queue.is_empty() && s.threads > inner.core {
                        s.threads -= 1;
                        if s.threads == 0 && s.shutdown {
                            inner.terminated.notify_all();
                        }
                        return;
                    }
                }
            }
        };
        inner.state.lock().unwrap().active += 1;
        // a panicking task must not kill the worker
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
        let mut s = inner.state.lock().unwrap();
        s.active -= 1;
        s.completed += 1;
    }
}

impl Executor for ThreadPoolExecutor {
    fn execute(&self, task: Task) -> Result<(), RejectedExecutionError> {
        let mut s = self.inner.state.lock().unwrap();
        if s.shutdown {
            return Err(RejectedExecutionError("Executor has been shut down".into()));
        }
        // 1) start a core thread
        if s.threads < self.inner.core {
            if self.spawn_worker(&mut s, Some(task)) {
                return Ok(());
            }
            return Err(RejectedExecutionError("Cannot create thread".into()));
        }
        // 2) queue it (a zero capacity queue only hands over to an idle thread, like Java's
        //    SynchronousQueue)
        let has_room = match self.inner.capacity {
            None => true,
            Some(0) => s.idle > s.queue.len(),
            Some(c) => s.queue.len() < c,
        };
        if has_room {
            s.queue.push_back(task);
            self.inner.work.notify_one();
            return Ok(());
        }
        // 3) start a non-core thread
        if s.threads < self.inner.max {
            if self.spawn_worker(&mut s, Some(task)) {
                return Ok(());
            }
            return Err(RejectedExecutionError("Cannot create thread".into()));
        }
        // 4) saturated
        match self.inner.policy {
            ThreadPoolRejectedPolicy::Abort => Err(RejectedExecutionError(format!(
                "Task rejected from ThreadPoolExecutor[{}]",
                self.inner.factory.name()
            ))),
            ThreadPoolRejectedPolicy::CallerRuns => {
                drop(s);
                task();
                Ok(())
            }
            ThreadPoolRejectedPolicy::Block => {
                while !s.shutdown
                    && self
                        .inner
                        .capacity
                        .map(|c| s.queue.len() >= c.max(1))
                        .unwrap_or(false)
                {
                    s = self.inner.space.wait(s).unwrap();
                }
                if s.shutdown {
                    return Err(RejectedExecutionError("Executor has been shut down".into()));
                }
                s.queue.push_back(task);
                self.inner.work.notify_one();
                Ok(())
            }
        }
    }

    fn shutdown(&self) {
        let mut s = self.inner.state.lock().unwrap();
        s.shutdown = true;
        self.inner.work.notify_all();
        self.inner.space.notify_all();
        if s.threads == 0 {
            self.inner.terminated.notify_all();
        }
    }

    fn is_shutdown(&self) -> bool {
        self.inner.state.lock().unwrap().shutdown
    }

    fn is_terminated(&self) -> bool {
        let s = self.inner.state.lock().unwrap();
        s.shutdown && s.threads == 0
    }
}

impl fmt::Display for ThreadPoolExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.inner.state.lock().unwrap();
        write!(
            f,
            "ThreadPoolExecutor[pool size = {}, active threads = {}, queued tasks = {}, completed tasks = {}][{}]",
            s.threads,
            s.active,
            s.queue.len(),
            s.completed,
            self.inner.factory.name()
        )
    }
}

// ---------------------------------------------------------------------------------------
// synchronous

/// A synchronous executor which always invokes the task in the caller thread (just a thread
/// pool facade). There is no task queue and no thread pool.
#[derive(Debug, Default)]
pub struct SynchronousExecutorService {
    shutdown: AtomicBool,
}

impl SynchronousExecutorService {
    /// Creates the executor.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Executor for SynchronousExecutorService {
    fn execute(&self, task: Task) -> Result<(), RejectedExecutionError> {
        task();
        Ok(())
    }

    fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------------------
// bounded

struct Semaphore {
    permits: Mutex<usize>,
    cond: Condvar,
    waiting: AtomicUsize,
}

impl Semaphore {
    fn acquire(&self, timeout: Option<Duration>) -> bool {
        let mut p = self.permits.lock().unwrap();
        let deadline = timeout.map(|t| Instant::now() + t);
        self.waiting.fetch_add(1, Ordering::SeqCst);
        let acquired = loop {
            if *p > 0 {
                *p -= 1;
                break true;
            }
            match deadline {
                None => p = self.cond.wait(p).unwrap(),
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        break false;
                    }
                    p = self.cond.wait_timeout(p, d - now).unwrap().0;
                }
            }
        };
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        acquired
    }

    fn release(&self) {
        *self.permits.lock().unwrap() += 1;
        self.cond.notify_one();
    }
}

/// An executor wrapper that enforces bounded concurrency via a semaphore.
///
/// Limits the maximum number of tasks delegated to the underlying executor. When no permit is
/// available the behaviour depends on the [`ThreadPoolRejectedPolicy`]:
///
/// * `CallerRuns`: waits up to the timeout for a permit; on timeout, runs the task in the
///   caller thread (outside the semaphore accounting). Tasks are never lost.
/// * `Abort`: waits up to the timeout; on timeout, rejects the task.
/// * `Block`: waits indefinitely until a permit becomes available.
pub struct BoundedExecutorService {
    delegate: Arc<dyn Executor>,
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
    timeout: Duration,
    policy: ThreadPoolRejectedPolicy,
    caller_runs: AtomicU64,
    rejected: AtomicU64,
    delegated: Arc<AtomicU64>,
}

impl BoundedExecutorService {
    /// Creates the executor.
    ///
    /// # Arguments
    ///
    /// * `delegate` - the underlying executor
    /// * `max_concurrent` - the maximum number of tasks delegated concurrently
    /// * `acquire_timeout` - the maximum time to wait for a permit (ignored for `Block`)
    /// * `policy` - the policy to apply when no permit is available
    pub fn new(
        delegate: Arc<dyn Executor>,
        max_concurrent: usize,
        acquire_timeout: Duration,
        policy: ThreadPoolRejectedPolicy,
    ) -> Self {
        BoundedExecutorService {
            delegate,
            semaphore: Arc::new(Semaphore {
                permits: Mutex::new(max_concurrent),
                cond: Condvar::new(),
                waiting: AtomicUsize::new(0),
            }),
            max_concurrent,
            timeout: acquire_timeout,
            policy,
            caller_runs: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            delegated: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The maximum number of concurrently delegated tasks.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// The number of tasks currently delegated (running).
    pub fn active_count(&self) -> usize {
        self.max_concurrent - self.available_permits()
    }

    /// The number of free permits.
    pub fn available_permits(&self) -> usize {
        *self.semaphore.permits.lock().unwrap()
    }

    /// The number of callers waiting for a permit.
    pub fn waiting_count(&self) -> usize {
        self.semaphore.waiting.load(Ordering::SeqCst)
    }

    /// How many tasks ran in the caller thread because no permit was available in time.
    pub fn caller_runs_count(&self) -> u64 {
        self.caller_runs.load(Ordering::SeqCst)
    }

    /// How many tasks were rejected.
    pub fn rejected_count(&self) -> u64 {
        self.rejected.load(Ordering::SeqCst)
    }

    /// How many delegated tasks have completed.
    pub fn delegated_task_count(&self) -> u64 {
        self.delegated.load(Ordering::SeqCst)
    }
}

impl Executor for BoundedExecutorService {
    fn execute(&self, task: Task) -> Result<(), RejectedExecutionError> {
        if self.delegate.is_shutdown() {
            return Err(RejectedExecutionError("Executor has been shut down".into()));
        }
        let timeout = (self.policy != ThreadPoolRejectedPolicy::Block).then_some(self.timeout);
        if !self.semaphore.acquire(timeout) {
            if self.policy == ThreadPoolRejectedPolicy::CallerRuns {
                self.caller_runs.fetch_add(1, Ordering::SeqCst);
                task();
                return Ok(());
            }
            self.rejected.fetch_add(1, Ordering::SeqCst);
            return Err(RejectedExecutionError(
                "Executor saturated: timed out waiting for a permit".into(),
            ));
        }
        let sem = self.semaphore.clone();
        let delegated = self.delegated.clone();
        let result = self.delegate.execute(Box::new(move || {
            struct Release(Arc<Semaphore>, Arc<AtomicU64>);
            impl Drop for Release {
                fn drop(&mut self) {
                    self.1.fetch_add(1, Ordering::SeqCst);
                    self.0.release();
                }
            }
            let _release = Release(sem, delegated);
            task();
        }));
        if result.is_err() {
            self.semaphore.release();
        }
        result
    }

    fn shutdown(&self) {
        self.delegate.shutdown();
    }

    fn is_shutdown(&self) -> bool {
        self.delegate.is_shutdown()
    }

    fn is_terminated(&self) -> bool {
        self.delegate.is_terminated()
    }
}

// ---------------------------------------------------------------------------------------
// scheduled

enum Repeat {
    Once,
    FixedRate(Duration),
    FixedDelay(Duration),
}

struct ScheduledEntry {
    task: Option<Arc<Mutex<dyn FnMut() + Send>>>,
    once: Option<Task>,
    repeat: Repeat,
    state: Arc<FutureState>,
}

#[derive(Default)]
struct FutureState {
    cancelled: AtomicBool,
    done: AtomicBool,
}

/// A handle to a scheduled task (Java `ScheduledFuture`).
#[derive(Clone)]
pub struct ScheduledFuture {
    state: Arc<FutureState>,
    id: u64,
    scheduler: Arc<SchedInner>,
}

impl fmt::Debug for ScheduledFuture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledFuture")
            .field("id", &self.id)
            .field("cancelled", &self.is_cancelled())
            .field("done", &self.is_done())
            .finish()
    }
}

impl ScheduledFuture {
    /// Cancels the task (a running execution is not interrupted).
    ///
    /// # Returns
    ///
    /// `false` if the task already completed or was already cancelled.
    pub fn cancel(&self) -> bool {
        if self.state.done.load(Ordering::SeqCst)
            || self.state.cancelled.swap(true, Ordering::SeqCst)
        {
            return false;
        }
        let mut s = self.scheduler.state.lock().unwrap();
        s.entries.remove(&self.id);
        self.scheduler.cond.notify_all();
        true
    }

    /// Whether the task was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    /// Whether the task completed (or was cancelled).
    pub fn is_done(&self) -> bool {
        self.state.done.load(Ordering::SeqCst) || self.is_cancelled()
    }
}

#[derive(PartialEq, Eq)]
struct Due(Instant, u64);

impl Ord for Due {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other.0.cmp(&self.0).then_with(|| other.1.cmp(&self.1))
    }
}

impl PartialOrd for Due {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

struct SchedState {
    heap: BinaryHeap<Due>,
    entries: HashMap<u64, ScheduledEntry>,
    next_id: u64,
    shutdown: bool,
    timer_started: bool,
}

struct SchedInner {
    state: Mutex<SchedState>,
    cond: Condvar,
    workers: ThreadPoolExecutor,
}

/// A scheduled thread pool (Java `ScheduledThreadPoolExecutor` /
/// `RejectableScheduledThreadPoolExecutor`): a timer thread hands due tasks to a fixed pool of
/// worker threads.
///
/// # Examples
///
/// ```
/// use camel_util::concurrent::ScheduledThreadPoolExecutor;
/// use std::sync::mpsc;
/// use std::time::Duration;
/// let scheduler = ScheduledThreadPoolExecutor::new(1, "test");
/// let (tx, rx) = mpsc::channel();
/// scheduler.schedule(Duration::from_millis(10), Box::new(move || tx.send("done").unwrap())).unwrap();
/// assert_eq!("done", rx.recv_timeout(Duration::from_secs(5)).unwrap());
/// ```
#[derive(Clone)]
pub struct ScheduledThreadPoolExecutor {
    inner: Arc<SchedInner>,
}

impl ScheduledThreadPoolExecutor {
    /// Creates the scheduler with the given number of worker threads.
    pub fn new(core_pool_size: usize, name: &str) -> Self {
        ScheduledThreadPoolExecutor {
            inner: Arc::new(SchedInner {
                state: Mutex::new(SchedState {
                    heap: BinaryHeap::new(),
                    entries: HashMap::new(),
                    next_id: 0,
                    shutdown: false,
                    timer_started: false,
                }),
                cond: Condvar::new(),
                workers: ThreadPoolExecutor::fixed(core_pool_size.max(1), name),
            }),
        }
    }

    fn ensure_timer(&self, s: &mut SchedState) {
        if s.timer_started {
            return;
        }
        s.timer_started = true;
        let weak = Arc::downgrade(&self.inner);
        let _ = thread::Builder::new()
            .name("Camel Scheduler Timer".into())
            .spawn(move || timer_loop(weak));
    }

    fn add(
        &self,
        delay: Duration,
        entry: ScheduledEntry,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        let mut s = self.inner.state.lock().unwrap();
        if s.shutdown {
            return Err(RejectedExecutionError("Executor has been shut down".into()));
        }
        self.ensure_timer(&mut s);
        let id = s.next_id;
        s.next_id += 1;
        let state = entry.state.clone();
        s.entries.insert(id, entry);
        s.heap.push(Due(Instant::now() + delay, id));
        self.inner.cond.notify_all();
        Ok(ScheduledFuture {
            state,
            id,
            scheduler: self.inner.clone(),
        })
    }

    /// Schedules the task to run once after the delay.
    ///
    /// # Errors
    ///
    /// Returns an error if the scheduler is shut down.
    pub fn schedule(
        &self,
        delay: Duration,
        task: Task,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        self.add(
            delay,
            ScheduledEntry {
                task: None,
                once: Some(task),
                repeat: Repeat::Once,
                state: Arc::default(),
            },
        )
    }

    /// Schedules the task to run periodically at a fixed rate.
    ///
    /// # Errors
    ///
    /// Returns an error if the scheduler is shut down.
    pub fn schedule_at_fixed_rate(
        &self,
        initial_delay: Duration,
        period: Duration,
        task: impl FnMut() + Send + 'static,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        self.add(
            initial_delay,
            ScheduledEntry {
                task: Some(Arc::new(Mutex::new(task))),
                once: None,
                repeat: Repeat::FixedRate(period),
                state: Arc::default(),
            },
        )
    }

    /// Schedules the task to run periodically with a fixed delay between the end of one run and
    /// the start of the next.
    ///
    /// # Errors
    ///
    /// Returns an error if the scheduler is shut down.
    pub fn schedule_with_fixed_delay(
        &self,
        initial_delay: Duration,
        delay: Duration,
        task: impl FnMut() + Send + 'static,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        self.add(
            initial_delay,
            ScheduledEntry {
                task: Some(Arc::new(Mutex::new(task))),
                once: None,
                repeat: Repeat::FixedDelay(delay),
                state: Arc::default(),
            },
        )
    }

    /// The number of scheduled tasks waiting to run.
    pub fn queue_size(&self) -> usize {
        self.inner.state.lock().unwrap().entries.len()
    }

    /// The worker thread pool.
    pub fn workers(&self) -> &ThreadPoolExecutor {
        &self.inner.workers
    }

    /// Shuts down and drops all scheduled tasks (Java `shutdownNow`).
    pub fn shutdown_now(&self) {
        let mut s = self.inner.state.lock().unwrap();
        s.shutdown = true;
        s.entries.clear();
        s.heap.clear();
        self.inner.cond.notify_all();
        drop(s);
        self.inner.workers.shutdown_now();
    }
}

fn timer_loop(weak: std::sync::Weak<SchedInner>) {
    loop {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let mut s = inner.state.lock().unwrap();
        if s.shutdown && s.entries.is_empty() {
            drop(s);
            inner.workers.shutdown();
            return;
        }
        let now = Instant::now();
        match s.heap.peek() {
            None => {
                // wait for new entries, but release the Arc regularly so the scheduler can drop
                let _ = inner
                    .cond
                    .wait_timeout(s, Duration::from_millis(200))
                    .unwrap();
                continue;
            }
            Some(Due(at, _)) if *at > now => {
                let wait = (*at - now).min(Duration::from_millis(200));
                let _ = inner.cond.wait_timeout(s, wait).unwrap();
                continue;
            }
            Some(_) => {}
        }
        let Due(at, id) = s.heap.pop().unwrap();
        let Some(mut entry) = s.entries.remove(&id) else {
            continue; // cancelled
        };
        let state = entry.state.clone();
        match entry.repeat {
            Repeat::Once => {
                let task = entry.once.take().unwrap();
                drop(s);
                let _ = inner.workers.execute(Box::new(move || {
                    if !state.cancelled.load(Ordering::SeqCst) {
                        task();
                    }
                    state.done.store(true, Ordering::SeqCst);
                }));
            }
            Repeat::FixedRate(period) => {
                let task = entry.task.clone().unwrap();
                s.entries.insert(id, entry);
                s.heap.push(Due(at + period, id));
                drop(s);
                let _ = inner.workers.execute(Box::new(move || {
                    if !state.cancelled.load(Ordering::SeqCst) {
                        (task.lock().unwrap())();
                    }
                }));
            }
            Repeat::FixedDelay(delay) => {
                let task = entry.task.clone().unwrap();
                drop(s);
                let weak2 = Arc::downgrade(&inner);
                let _ = inner.workers.execute(Box::new(move || {
                    if state.cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    (task.lock().unwrap())();
                    if let Some(inner) = weak2.upgrade() {
                        let mut s = inner.state.lock().unwrap();
                        if !s.shutdown && !state.cancelled.load(Ordering::SeqCst) {
                            s.entries.insert(id, entry);
                            s.heap.push(Due(Instant::now() + delay, id));
                            inner.cond.notify_all();
                        }
                    }
                }));
            }
        }
    }
}

impl Executor for ScheduledThreadPoolExecutor {
    fn execute(&self, task: Task) -> Result<(), RejectedExecutionError> {
        self.schedule(Duration::ZERO, task).map(|_| ())
    }

    fn shutdown(&self) {
        let mut s = self.inner.state.lock().unwrap();
        s.shutdown = true;
        // periodic tasks stop at shutdown; delayed one-shot tasks still run
        s.entries.retain(|_, e| matches!(e.repeat, Repeat::Once));
        let ids: Vec<u64> = s.entries.keys().copied().collect();
        if ids.is_empty() {
            drop(s);
            self.inner.workers.shutdown();
        }
        self.inner.cond.notify_all();
    }

    fn is_shutdown(&self) -> bool {
        self.inner.state.lock().unwrap().shutdown
    }

    fn is_terminated(&self) -> bool {
        let s = self.inner.state.lock().unwrap();
        s.shutdown && s.entries.is_empty()
    }
}

/// The error message when a [`SizedScheduledExecutorService`] is full.
pub const QUEUE_SIZE_LIMIT_REACHED: &str = "Task rejected due queue size limit reached";

/// A sized scheduled executor which rejects tasks if the task queue is full.
///
/// The default scheduled thread pool has an unbounded task queue, which means you can keep
/// scheduling tasks which may cause the system to run out of memory. This wrapper rejects tasks
/// when an upper limit of the task queue has been reached.
#[derive(Clone)]
pub struct SizedScheduledExecutorService {
    delegate: ScheduledThreadPoolExecutor,
    queue_size: usize,
}

impl SizedScheduledExecutorService {
    /// Creates the executor.
    ///
    /// # Arguments
    ///
    /// * `delegate` - the actual scheduled thread pool
    /// * `queue_size` - the upper queue size, `0` for unlimited
    pub fn new(delegate: ScheduledThreadPoolExecutor, queue_size: usize) -> Self {
        SizedScheduledExecutorService {
            delegate,
            queue_size,
        }
    }

    /// The wrapped scheduled thread pool.
    pub fn scheduled_thread_pool_executor(&self) -> &ScheduledThreadPoolExecutor {
        &self.delegate
    }

    fn check(&self) -> Result<(), RejectedExecutionError> {
        if self.queue_size > 0 && self.delegate.queue_size() >= self.queue_size {
            Err(RejectedExecutionError(QUEUE_SIZE_LIMIT_REACHED.into()))
        } else {
            Ok(())
        }
    }

    /// Schedules the task to run once after the delay.
    ///
    /// # Errors
    ///
    /// Returns [`QUEUE_SIZE_LIMIT_REACHED`] if the queue is full, or an error after shutdown.
    pub fn schedule(
        &self,
        delay: Duration,
        task: Task,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        self.check()?;
        self.delegate.schedule(delay, task)
    }

    /// Schedules the task periodically at a fixed rate.
    ///
    /// # Errors
    ///
    /// Returns [`QUEUE_SIZE_LIMIT_REACHED`] if the queue is full, or an error after shutdown.
    pub fn schedule_at_fixed_rate(
        &self,
        initial_delay: Duration,
        period: Duration,
        task: impl FnMut() + Send + 'static,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        self.check()?;
        self.delegate
            .schedule_at_fixed_rate(initial_delay, period, task)
    }

    /// Schedules the task periodically with a fixed delay.
    ///
    /// # Errors
    ///
    /// Returns [`QUEUE_SIZE_LIMIT_REACHED`] if the queue is full, or an error after shutdown.
    pub fn schedule_with_fixed_delay(
        &self,
        initial_delay: Duration,
        delay: Duration,
        task: impl FnMut() + Send + 'static,
    ) -> Result<ScheduledFuture, RejectedExecutionError> {
        self.check()?;
        self.delegate
            .schedule_with_fixed_delay(initial_delay, delay, task)
    }

    /// Shuts down and drops all scheduled tasks.
    pub fn shutdown_now(&self) {
        self.delegate.shutdown_now();
    }
}

impl Executor for SizedScheduledExecutorService {
    fn execute(&self, task: Task) -> Result<(), RejectedExecutionError> {
        self.check()?;
        self.delegate.execute(task)
    }

    fn shutdown(&self) {
        self.delegate.shutdown();
    }

    fn is_shutdown(&self) -> bool {
        self.delegate.is_shutdown()
    }
}

// ---------------------------------------------------------------------------------------
// completion service

struct Completed<V> {
    id: usize,
    result: V,
}

impl<V> PartialEq for Completed<V> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<V> Eq for Completed<V> {}

impl<V> Ord for Completed<V> {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other.id.cmp(&self.id)
    }
}

impl<V> PartialOrd for Completed<V> {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

struct CompletionState<V> {
    queue: BinaryHeap<Completed<V>>,
    index: usize,
}

/// A completion service that returns the completed results in the same order as the tasks
/// were submitted (or in completion order when not ordered).
///
/// Each task receives a callback to deliver its result, so it can complete asynchronously.
pub struct AsyncCompletionService<V> {
    executor: Arc<dyn Executor>,
    ordered: bool,
    next_id: AtomicUsize,
    state: Arc<(Mutex<CompletionState<V>>, Condvar)>,
}

impl<V: Send + 'static> AsyncCompletionService<V> {
    /// Creates the service.
    pub fn new(executor: Arc<dyn Executor>, ordered: bool) -> Self {
        AsyncCompletionService {
            executor,
            ordered,
            next_id: AtomicUsize::new(0),
            state: Arc::new((
                Mutex::new(CompletionState {
                    queue: BinaryHeap::new(),
                    index: 0,
                }),
                Condvar::new(),
            )),
        }
    }

    /// Submits a task; the task calls the given callback with its result when done.
    ///
    /// # Errors
    ///
    /// Returns an error if the executor rejects the task.
    pub fn submit<F>(&self, runner: F) -> Result<(), RejectedExecutionError>
    where
        F: FnOnce(Box<dyn FnOnce(V) + Send>) + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let state = self.state.clone();
        self.executor.execute(Box::new(move || {
            runner(Box::new(move |result| {
                let (lock, cond) = &*state;
                lock.lock().unwrap().queue.push(Completed { id, result });
                cond.notify_all();
            }))
        }))
    }

    /// Skips the next result in order (for a task that will never deliver a result).
    pub fn skip(&self) {
        self.state.0.lock().unwrap().index += 1;
    }

    fn try_take(&self, s: &mut CompletionState<V>) -> Option<V> {
        let ready = match s.queue.peek() {
            Some(t) => !self.ordered || t.id == s.index,
            None => false,
        };
        if ready {
            let t = s.queue.pop().unwrap();
            if self.ordered {
                s.index += 1;
            }
            Some(t.result)
        } else {
            None
        }
    }

    /// Returns the next completed result in order, or `None` if not available yet.
    pub fn poll(&self) -> Option<V> {
        let mut s = self.state.0.lock().unwrap();
        self.try_take(&mut s)
    }

    /// Returns any completed result regardless of the order, or `None`.
    pub fn poll_unordered(&self) -> Option<V> {
        self.state.0.lock().unwrap().queue.pop().map(|t| t.result)
    }

    /// Waits up to the timeout for the next result in order.
    pub fn poll_timeout(&self, timeout: Duration) -> Option<V> {
        let deadline = Instant::now() + timeout;
        let (lock, cond) = &*self.state;
        let mut s = lock.lock().unwrap();
        loop {
            if let Some(v) = self.try_take(&mut s) {
                return Some(v);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            s = cond.wait_timeout(s, deadline - now).unwrap().0;
        }
    }

    /// Waits for the next result in order.
    pub fn take(&self) -> V {
        let (lock, cond) = &*self.state;
        let mut s = lock.lock().unwrap();
        loop {
            if let Some(v) = self.try_take(&mut s) {
                return v;
            }
            s = cond.wait(s).unwrap();
        }
    }

    /// The number of completed results not taken yet.
    pub fn size(&self) -> usize {
        self.state.0.lock().unwrap().queue.len()
    }
}

// ---------------------------------------------------------------------------------------
// latch

/// A synchronization aid that allows threads to wait until a count of operations completes
/// (Java `CountDownLatch`).
///
/// # Examples
///
/// ```
/// use camel_util::concurrent::CountDownLatch;
/// use std::sync::Arc;
/// use std::time::Duration;
/// let latch = Arc::new(CountDownLatch::new(1));
/// let l = latch.clone();
/// std::thread::spawn(move || l.count_down());
/// assert!(latch.await_timeout(Duration::from_secs(5)));
/// ```
#[derive(Debug)]
pub struct CountDownLatch {
    count: Mutex<usize>,
    cond: Condvar,
}

impl CountDownLatch {
    /// Creates the latch with the given count.
    pub fn new(count: usize) -> Self {
        CountDownLatch {
            count: Mutex::new(count),
            cond: Condvar::new(),
        }
    }

    /// Decrements the count, releasing the waiting threads when it reaches zero.
    pub fn count_down(&self) {
        let mut c = self.count.lock().unwrap();
        if *c > 0 {
            *c -= 1;
            if *c == 0 {
                self.cond.notify_all();
            }
        }
    }

    /// The current count.
    pub fn count(&self) -> usize {
        *self.count.lock().unwrap()
    }

    /// Waits until the count reaches zero.
    pub fn await_zero(&self) {
        let mut c = self.count.lock().unwrap();
        while *c > 0 {
            c = self.cond.wait(c).unwrap();
        }
    }

    /// Waits up to the timeout until the count reaches zero.
    ///
    /// # Returns
    ///
    /// `true` if the count reached zero, `false` on timeout.
    pub fn await_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut c = self.count.lock().unwrap();
        while *c > 0 {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            c = self.cond.wait_timeout(c, deadline - now).unwrap().0;
        }
        true
    }
}

// ---------------------------------------------------------------------------------------
// context values

static CONTEXT_VALUE_IDS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static CONTEXT_VALUES: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::new(HashMap::new());
}

/// A thread-scoped value (Java `ContextValue`, backed by a thread local).
///
/// Use [`ContextValue::where_`] to bind a value for the duration of a closure (the previous
/// value is restored afterwards, also on panic), or [`ContextValue::set`] for mutable state.
/// The value is not visible in other threads.
///
/// # Examples
///
/// ```
/// use camel_util::concurrent::ContextValue;
/// let route_id: ContextValue<String> = ContextValue::new_instance("routeId");
/// let r = route_id.where_("myRoute".to_string(), || format!("Result: {}", route_id.get().unwrap()));
/// assert_eq!("Result: myRoute", r);
/// assert!(!route_id.is_bound());
/// ```
pub struct ContextValue<T> {
    id: u64,
    name: String,
    supplier: Option<Box<dyn Fn() -> T + Send + Sync>>,
}

impl<T: Clone + 'static> ContextValue<T> {
    /// Creates a new context value with the given name.
    pub fn new_instance(name: &str) -> Self {
        ContextValue {
            id: CONTEXT_VALUE_IDS.fetch_add(1, Ordering::SeqCst),
            name: name.to_string(),
            supplier: None,
        }
    }

    /// Creates a new thread-local context value with the given name.
    pub fn new_thread_local(name: &str) -> Self {
        Self::new_instance(name)
    }

    /// Creates a new thread-local context value with an initial value supplier.
    pub fn new_thread_local_with(
        name: &str,
        supplier: impl Fn() -> T + Send + Sync + 'static,
    ) -> Self {
        ContextValue {
            id: CONTEXT_VALUE_IDS.fetch_add(1, Ordering::SeqCst),
            name: name.to_string(),
            supplier: Some(Box::new(supplier)),
        }
    }

    /// Returns the value for the current thread (initialized from the supplier if any), or
    /// `None` if not bound.
    pub fn get(&self) -> Option<T> {
        let existing = CONTEXT_VALUES.with(|m| {
            m.borrow()
                .get(&self.id)
                .and_then(|v| v.downcast_ref::<T>().cloned())
        });
        if existing.is_some() {
            return existing;
        }
        let initial = self.supplier.as_ref().map(|s| s())?;
        self.set(initial.clone());
        Some(initial)
    }

    /// Returns the value for the current thread, or the default value if not bound.
    pub fn or_else(&self, default_value: T) -> T {
        self.get().unwrap_or(default_value)
    }

    /// Whether a value is bound for the current thread.
    pub fn is_bound(&self) -> bool {
        self.get().is_some()
    }

    /// Sets the value for the current thread.
    pub fn set(&self, value: T) {
        CONTEXT_VALUES.with(|m| m.borrow_mut().insert(self.id, Box::new(value)));
    }

    /// Removes the value for the current thread.
    pub fn remove(&self) {
        CONTEXT_VALUES.with(|m| m.borrow_mut().remove(&self.id));
    }

    /// The name of this context value.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Executes the operation with this context value bound to the value, restoring the
    /// previous value afterwards (Java `ContextValue.where`).
    pub fn where_<R>(&self, value: T, operation: impl FnOnce() -> R) -> R {
        struct Restore<'a, T: Clone + 'static>(&'a ContextValue<T>, Option<T>);
        impl<T: Clone + 'static> Drop for Restore<'_, T> {
            fn drop(&mut self) {
                match self.1.take() {
                    Some(old) => self.0.set(old),
                    None => self.0.remove(),
                }
            }
        }
        let old = CONTEXT_VALUES.with(|m| {
            m.borrow()
                .get(&self.id)
                .and_then(|v| v.downcast_ref::<T>().cloned())
        });
        self.set(value);
        let _restore = Restore(self, old);
        operation()
    }
}

impl<T> fmt::Display for ContextValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContextValue[{}]", self.name)
    }
}

#[cfg(test)]
mod tests;
