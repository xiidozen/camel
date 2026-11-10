//! Back-off policies and a timer that runs tasks according to them. Port of the
//! `org.apache.camel.util.backoff` package (`BackOff`, `BackOffTimer`, `BackOffTimerTask` and
//! `SimpleBackOffTimer`).

use crate::concurrent::{ScheduledFuture, ScheduledThreadPoolExecutor};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The value of a delay or time meaning "never" (no further attempt should be made).
pub const NEVER: i64 = -1;
/// The default delay between attempts (2 seconds).
pub const DEFAULT_DELAY: Duration = Duration::from_secs(2);
/// The default multiplier (`1.0`, a constant delay).
pub const DEFAULT_MULTIPLIER: f64 = 1.0;
/// The maximum duration, meaning no limit.
pub const MAX_DURATION: Duration = Duration::from_millis(i64::MAX as u64);

/// A back-off policy.
///
/// # Examples
///
/// ```
/// use camel_util::backoff::BackOff;
/// use std::time::Duration;
/// let backoff = BackOff::builder().delay_millis(100).multiplier(1.5).max_attempts(5).build();
/// assert_eq!(Duration::from_millis(100), backoff.delay());
/// assert_eq!("BackOff[delay=100, maxAttempts=5, multiplier=1.5, remove=true]", backoff.to_string());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BackOff {
    delay: Duration,
    max_delay: Duration,
    max_elapsed_time: Duration,
    max_attempts: i64,
    multiplier: f64,
    remove_on_complete: bool,
}

impl Default for BackOff {
    fn default() -> Self {
        BackOff {
            delay: DEFAULT_DELAY,
            max_delay: MAX_DURATION,
            max_elapsed_time: MAX_DURATION,
            max_attempts: i64::MAX,
            multiplier: DEFAULT_MULTIPLIER,
            remove_on_complete: true,
        }
    }
}

impl BackOff {
    /// Creates a back-off with the default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a builder.
    pub fn builder() -> BackOffBuilder {
        BackOffBuilder::default()
    }

    /// Creates a builder initialized from the template.
    pub fn builder_from(template: &BackOff) -> BackOffBuilder {
        BackOffBuilder {
            inner: template.clone(),
        }
    }

    /// The delay to wait before retrying the operation.
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// Sets the delay to wait before retrying the operation.
    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = delay;
    }

    /// The maximum back-off time after which the delay is not increased any more.
    pub fn max_delay(&self) -> Duration {
        self.max_delay
    }

    /// Sets the maximum delay.
    pub fn set_max_delay(&mut self, max_delay: Duration) {
        self.max_delay = max_delay;
    }

    /// The maximum elapsed time after which the back-off is exhausted and no more attempts
    /// should be made.
    pub fn max_elapsed_time(&self) -> Duration {
        self.max_elapsed_time
    }

    /// Sets the maximum elapsed time.
    pub fn set_max_elapsed_time(&mut self, max_elapsed_time: Duration) {
        self.max_elapsed_time = max_elapsed_time;
    }

    /// The maximum number of attempts after which the back-off is exhausted.
    pub fn max_attempts(&self) -> i64 {
        self.max_attempts
    }

    /// Sets the maximum number of attempts.
    pub fn set_max_attempts(&mut self, max_attempts: i64) {
        self.max_attempts = max_attempts;
    }

    /// The value to multiply the current delay by for each retry attempt.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// Sets the multiplier.
    pub fn set_multiplier(&mut self, multiplier: f64) {
        self.multiplier = multiplier;
    }

    /// Whether the task should be removed from the timer when it completes successfully.
    pub fn is_remove_on_complete(&self) -> bool {
        self.remove_on_complete
    }

    /// Sets whether the task is removed from the timer when complete.
    pub fn set_remove_on_complete(&mut self, remove_on_complete: bool) {
        self.remove_on_complete = remove_on_complete;
    }
}

fn millis(d: Duration) -> i64 {
    d.as_millis().min(i64::MAX as u128) as i64
}

impl fmt::Display for BackOff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BackOff[delay={}", millis(self.delay))?;
        if self.max_delay != MAX_DURATION {
            write!(f, ", maxDelay={}", millis(self.max_delay))?;
        }
        if self.max_elapsed_time != MAX_DURATION {
            write!(f, ", maxElapsedTime={}", millis(self.max_elapsed_time))?;
        }
        if self.max_attempts != i64::MAX {
            write!(f, ", maxAttempts={}", self.max_attempts)?;
        }
        if self.multiplier != DEFAULT_MULTIPLIER {
            write!(
                f,
                ", multiplier={}",
                crate::value::java_double_to_string(self.multiplier)
            )?;
        }
        write!(f, ", remove={}]", self.remove_on_complete)
    }
}

/// A builder for [`BackOff`].
#[derive(Debug, Clone, Default)]
pub struct BackOffBuilder {
    inner: BackOff,
}

impl BackOffBuilder {
    /// Reads the values from the given back-off.
    pub fn read(mut self, template: &BackOff) -> Self {
        self.inner = template.clone();
        self
    }

    /// The delay.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.inner.delay = delay;
        self
    }

    /// The delay in milliseconds.
    pub fn delay_millis(self, delay: u64) -> Self {
        self.delay(Duration::from_millis(delay))
    }

    /// The maximum delay.
    pub fn max_delay(mut self, max_delay: Duration) -> Self {
        self.inner.max_delay = max_delay;
        self
    }

    /// The maximum delay in milliseconds.
    pub fn max_delay_millis(self, max_delay: u64) -> Self {
        self.max_delay(Duration::from_millis(max_delay))
    }

    /// The maximum elapsed time.
    pub fn max_elapsed_time(mut self, max_elapsed_time: Duration) -> Self {
        self.inner.max_elapsed_time = max_elapsed_time;
        self
    }

    /// The maximum elapsed time in milliseconds.
    pub fn max_elapsed_time_millis(self, max_elapsed_time: u64) -> Self {
        self.max_elapsed_time(Duration::from_millis(max_elapsed_time))
    }

    /// The maximum number of attempts.
    pub fn max_attempts(mut self, attempts: i64) -> Self {
        self.inner.max_attempts = attempts;
        self
    }

    /// The multiplier.
    pub fn multiplier(mut self, multiplier: f64) -> Self {
        self.inner.multiplier = multiplier;
        self
    }

    /// Whether the task is removed from the timer when complete.
    pub fn remove_on_complete(mut self, remove_on_complete: bool) -> Self {
        self.inner.remove_on_complete = remove_on_complete;
        self
    }

    /// Builds a new [`BackOff`].
    pub fn build(self) -> BackOff {
        self.inner
    }
}

/// The status of a back-off task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// The task is scheduled to run again.
    Active,
    /// The task was stopped (or cancelled).
    Inactive,
    /// The task ran out of attempts or time.
    Exhausted,
    /// The task function returned `false`: no more attempts are needed.
    Completed,
    /// The task function failed with an error.
    Failed,
}

/// The error type of a task function.
pub type TaskError = Arc<dyn Error + Send + Sync>;

type TaskFn = dyn Fn(&BackOffTimerTask) -> Result<bool, TaskError> + Send + Sync;
type CompleteFn = dyn Fn(&BackOffTimerTask, Option<&TaskError>) + Send + Sync;

struct TaskState {
    status: Status,
    first_attempt_time: i64,
    current_attempts: i64,
    current_delay: i64,
    current_elapsed_time: i64,
    last_attempt_time: i64,
    next_attempt_time: i64,
    cause: Option<TaskError>,
    future: Option<ScheduledFuture>,
}

struct TaskInner {
    name: String,
    backoff: BackOff,
    timer: Option<Weak<TimerInner>>,
    scheduler: Option<ScheduledThreadPoolExecutor>,
    function: Box<TaskFn>,
    consumers: Mutex<Vec<Arc<CompleteFn>>>,
    state: Mutex<TaskState>,
}

/// A task run by a back-off timer (Java `BackOffTimer.Task` / `BackOffTimerTask`). Cloning is
/// cheap and refers to the same task.
#[derive(Clone)]
pub struct BackOffTimerTask {
    inner: Arc<TaskInner>,
}

impl PartialEq for BackOffTimerTask {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl BackOffTimerTask {
    /// Creates a task (normally done by [`SimpleBackOffTimer::schedule`]); without a timer and
    /// scheduler the task can be driven manually with [`BackOffTimerTask::next`].
    pub fn new(
        timer: Option<&SimpleBackOffTimer>,
        backoff: BackOff,
        scheduler: Option<ScheduledThreadPoolExecutor>,
        function: impl Fn(&BackOffTimerTask) -> Result<bool, TaskError> + Send + Sync + 'static,
    ) -> Self {
        let delay = millis(backoff.delay());
        BackOffTimerTask {
            inner: Arc::new(TaskInner {
                name: timer.map(|t| t.inner.name.clone()).unwrap_or_default(),
                backoff,
                timer: timer.map(|t| Arc::downgrade(&t.inner)),
                scheduler,
                function: Box::new(function),
                consumers: Mutex::new(Vec::new()),
                state: Mutex::new(TaskState {
                    status: Status::Active,
                    first_attempt_time: NEVER,
                    current_attempts: 0,
                    current_delay: delay,
                    current_elapsed_time: 0,
                    last_attempt_time: NEVER,
                    next_attempt_time: NEVER,
                    cause: None,
                    future: None,
                }),
            }),
        }
    }

    /// The name of the task (the name of its timer).
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// The back-off associated with this task.
    pub fn backoff(&self) -> &BackOff {
        &self.inner.backoff
    }

    /// The task status.
    pub fn status(&self) -> Status {
        self.inner.state.lock().unwrap().status
    }

    /// The number of attempts so far.
    pub fn current_attempts(&self) -> i64 {
        self.inner.state.lock().unwrap().current_attempts
    }

    /// The current computed delay in milliseconds ([`NEVER`] when stopped or exhausted).
    pub fn current_delay(&self) -> i64 {
        self.inner.state.lock().unwrap().current_delay
    }

    /// The current elapsed time in milliseconds.
    pub fn current_elapsed_time(&self) -> i64 {
        self.inner.state.lock().unwrap().current_elapsed_time
    }

    /// The time (millis since epoch) of the first attempt, or [`NEVER`].
    pub fn first_attempt_time(&self) -> i64 {
        self.inner.state.lock().unwrap().first_attempt_time
    }

    /// The time (millis since epoch) of the last attempt, or [`NEVER`].
    pub fn last_attempt_time(&self) -> i64 {
        self.inner.state.lock().unwrap().last_attempt_time
    }

    /// An indication of the time (millis since epoch) of the next attempt, or [`NEVER`].
    pub fn next_attempt_time(&self) -> i64 {
        self.inner.state.lock().unwrap().next_attempt_time
    }

    /// The error if the task failed.
    pub fn exception(&self) -> Option<TaskError> {
        self.inner.state.lock().unwrap().cause.clone()
    }

    /// Resets the task so it starts over from the initial delay.
    pub fn reset(&self) {
        let mut s = self.inner.state.lock().unwrap();
        s.current_attempts = 0;
        s.current_delay = millis(self.inner.backoff.delay());
        s.current_elapsed_time = 0;
        s.first_attempt_time = NEVER;
        s.last_attempt_time = NEVER;
        s.next_attempt_time = NEVER;
        s.status = Status::Active;
        s.cause = None;
    }

    /// Cancels the task: it is stopped, the completion callbacks are called and it is removed
    /// from its timer.
    pub fn cancel(&self) {
        let future = {
            let mut s = self.inner.state.lock().unwrap();
            stop(&mut s);
            s.future.take()
        };
        if let Some(f) = future {
            f.cancel();
        }
        self.complete(None);
        self.remove_from_timer();
    }

    /// Adds an action to execute when the task is completed (cancelled, exhausted, completed or
    /// failed). If the back-off removes completed tasks, the task is removed from its timer.
    pub fn when_complete(
        &self,
        when_completed: impl Fn(&BackOffTimerTask, Option<&TaskError>) + Send + Sync + 'static,
    ) {
        if self.inner.backoff.is_remove_on_complete() {
            self.remove_from_timer();
        }
        self.inner
            .consumers
            .lock()
            .unwrap()
            .push(Arc::new(when_completed));
    }

    fn remove_from_timer(&self) {
        if let Some(timer) = self.inner.timer.as_ref().and_then(Weak::upgrade) {
            timer.tasks.lock().unwrap().retain(|t| t != self);
        }
    }

    fn complete(&self, error: Option<TaskError>) {
        self.inner.state.lock().unwrap().cause = error.clone();
        // call the consumers without holding any lock, as they may take locks of their own,
        // which could deadlock with a thread holding such a lock and cancelling this task
        let copy: Vec<Arc<CompleteFn>> = self.inner.consumers.lock().unwrap().clone();
        for c in copy {
            c(self, error.as_ref());
        }
    }

    /// Runs one attempt of the task (called by the scheduler).
    pub fn run(&self) {
        {
            let mut s = self.inner.state.lock().unwrap();
            if s.status != Status::Active {
                return;
            }
            s.last_attempt_time = now_millis();
            if s.first_attempt_time < 0 {
                s.first_attempt_time = s.last_attempt_time;
            }
        }
        match (self.inner.function)(self) {
            Ok(true) => {
                let delay = self.next();
                let mut s = self.inner.state.lock().unwrap();
                if s.status != Status::Active {
                    drop(s);
                    self.complete(None);
                } else {
                    s.next_attempt_time = s.last_attempt_time + delay;
                    if let Some(scheduler) = &self.inner.scheduler {
                        let task = self.clone();
                        s.future = scheduler
                            .schedule(
                                Duration::from_millis(delay.max(0) as u64),
                                Box::new(move || task.run()),
                            )
                            .ok();
                    }
                }
            }
            Ok(false) => {
                {
                    let mut s = self.inner.state.lock().unwrap();
                    stop(&mut s);
                    s.status = Status::Completed;
                }
                self.complete(None);
            }
            Err(e) => {
                {
                    let mut s = self.inner.state.lock().unwrap();
                    stop(&mut s);
                    s.status = Status::Failed;
                }
                self.complete(Some(e));
            }
        }
    }

    /// Computes the next attempt: the number of milliseconds to wait before retrying the
    /// operation, or [`NEVER`] to indicate that no further attempt should be made.
    pub fn next(&self) -> i64 {
        let mut s = self.inner.state.lock().unwrap();
        // a call to next when the delay is NEVER has no effect, as the task is either exhausted
        // or explicitly stopped
        if s.status == Status::Active {
            s.current_attempts += 1;
            let b = &self.inner.backoff;
            if s.current_attempts > b.max_attempts()
                || s.current_elapsed_time > millis(b.max_elapsed_time())
            {
                s.current_delay = NEVER;
                s.status = Status::Exhausted;
            } else {
                // the delay grows by the multiplier but must not go above the max delay
                let max_delay = millis(b.max_delay());
                if s.current_delay < max_delay {
                    s.current_delay =
                        ((s.current_delay as f64 * b.multiplier()) as i64).min(max_delay);
                }
                s.current_elapsed_time += s.current_delay;
            }
        }
        s.current_delay
    }
}

fn stop(s: &mut TaskState) {
    s.current_attempts = 0;
    s.current_delay = NEVER;
    s.current_elapsed_time = 0;
    s.first_attempt_time = NEVER;
    s.last_attempt_time = NEVER;
    s.next_attempt_time = NEVER;
    s.status = Status::Inactive;
}

impl fmt::Display for BackOffTimerTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.inner.state.lock().unwrap();
        write!(
            f,
            "BackOffTimerTask[name={}, status={:?}, currentAttempts={}, currentDelay={}, currentElapsedTime={}, firstAttemptTime={}, lastAttemptTime={}, nextAttemptTime={}]",
            self.inner.name,
            s.status,
            s.current_attempts,
            s.current_delay,
            s.current_elapsed_time,
            s.first_attempt_time,
            s.last_attempt_time,
            s.next_attempt_time
        )
    }
}

impl fmt::Debug for BackOffTimerTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

struct TimerInner {
    name: String,
    scheduler: ScheduledThreadPoolExecutor,
    tasks: Mutex<Vec<BackOffTimerTask>>,
}

/// A simple timer that uses a [`BackOff`] to determine when a task should be executed (Java
/// `BackOffTimer` / `SimpleBackOffTimer`).
///
/// # Examples
///
/// ```
/// use camel_util::backoff::{BackOff, SimpleBackOffTimer, Status};
/// use camel_util::concurrent::{CountDownLatch, ScheduledThreadPoolExecutor};
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let timer = SimpleBackOffTimer::new(ScheduledThreadPoolExecutor::new(1, "backoff"));
/// let done = Arc::new(CountDownLatch::new(1));
/// let task = timer.schedule(BackOff::builder().delay_millis(10).build(), |t| Ok(t.current_attempts() < 3));
/// let d = done.clone();
/// task.when_complete(move |_, _| d.count_down());
/// assert!(done.await_timeout(Duration::from_secs(5)));
/// assert_eq!(Status::Completed, task.status());
/// ```
#[derive(Clone)]
pub struct SimpleBackOffTimer {
    inner: Arc<TimerInner>,
}

impl SimpleBackOffTimer {
    /// Creates the timer named `SimpleBackOffTimer`.
    pub fn new(scheduler: ScheduledThreadPoolExecutor) -> Self {
        Self::with_name("SimpleBackOffTimer", scheduler)
    }

    /// Creates the timer with the given name.
    pub fn with_name(name: &str, scheduler: ScheduledThreadPoolExecutor) -> Self {
        SimpleBackOffTimer {
            inner: Arc::new(TimerInner {
                name: name.to_string(),
                scheduler,
                tasks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Schedules the function to be executed some time in the future according to the
    /// back-off; the function returns `Ok(true)` to be run again, `Ok(false)` when done.
    pub fn schedule(
        &self,
        backoff: BackOff,
        function: impl Fn(&BackOffTimerTask) -> Result<bool, TaskError> + Send + Sync + 'static,
    ) -> BackOffTimerTask {
        let task = BackOffTimerTask::new(
            Some(self),
            backoff,
            Some(self.inner.scheduler.clone()),
            function,
        );
        let delay = task.next();
        if delay != NEVER {
            self.inner.tasks.lock().unwrap().push(task.clone());
            let t = task.clone();
            let future = self
                .inner
                .scheduler
                .schedule(
                    Duration::from_millis(delay as u64),
                    Box::new(move || t.run()),
                )
                .ok();
            task.inner.state.lock().unwrap().future = future;
        } else {
            task.cancel();
        }
        task
    }

    /// The name of this timer.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Removes the task.
    pub fn remove(&self, task: &BackOffTimerTask) {
        self.inner.tasks.lock().unwrap().retain(|t| t != task);
    }

    /// A snapshot of all the tasks.
    pub fn tasks(&self) -> Vec<BackOffTimerTask> {
        self.inner.tasks.lock().unwrap().clone()
    }

    /// The number of tasks.
    pub fn size(&self) -> usize {
        self.inner.tasks.lock().unwrap().len()
    }

    /// Stops and closes this timer (the tasks are forgotten).
    pub fn close(&self) {
        self.inner.tasks.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concurrent::{submit, CountDownLatch, ThreadPoolExecutor};
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    const DELAY: i64 = 2000;

    fn manual(backoff: BackOff) -> BackOffTimerTask {
        BackOffTimerTask::new(None, backoff, None, |_| Ok(true))
    }

    // --- BackOffTest ---

    #[test]
    fn test_simple_back_off() {
        let task = manual(BackOff::builder().build());
        for i in 1..=5 {
            let delay = task.next();
            assert_eq!(i, task.current_attempts());
            assert_eq!(DELAY, delay);
            assert_eq!(DELAY, task.current_delay());
            assert_eq!(DELAY * i, task.current_elapsed_time());
        }
    }

    #[test]
    fn test_back_off_with_multiplier() {
        let task = manual(BackOff::builder().multiplier(1.5).build());
        let mut delay = DELAY;
        let mut elapsed = 0;
        for i in 1..=5 {
            let old = delay;
            delay = task.next();
            elapsed += delay;
            assert_eq!(i, task.current_attempts());
            assert_eq!((old as f64 * 1.5) as i64, delay);
            assert_eq!((old as f64 * 1.5) as i64, task.current_delay());
            assert_eq!(elapsed, task.current_elapsed_time());
        }
    }

    #[test]
    fn test_back_off_with_max_attempts() {
        let task = manual(BackOff::builder().max_attempts(5).build());
        for i in 1..=5 {
            assert_eq!(DELAY, task.next());
            assert_eq!(i, task.current_attempts());
            assert_eq!(DELAY * i, task.current_elapsed_time());
        }
        assert_eq!(NEVER, task.next());
        assert_eq!(6, task.current_attempts());
    }

    #[test]
    fn test_back_off_with_max_time() {
        let task = manual(
            BackOff::builder()
                .max_elapsed_time(Duration::from_secs(9))
                .build(),
        );
        for i in 1..=5 {
            assert_eq!(DELAY, task.next());
            assert_eq!(i, task.current_attempts());
            assert_eq!(DELAY * i, task.current_elapsed_time());
        }
        assert_eq!(NEVER, task.next());
        assert_eq!(6, task.current_attempts());
    }

    #[test]
    fn test_back_off_max_delay_and_display() {
        let task = manual(
            BackOff::builder()
                .delay_millis(100)
                .multiplier(2.0)
                .max_delay_millis(300)
                .build(),
        );
        assert_eq!(200, task.next());
        assert_eq!(300, task.next());
        assert_eq!(300, task.next());
        task.reset();
        assert_eq!(0, task.current_attempts());
        assert_eq!(100, task.current_delay());
        let b = BackOff::builder()
            .max_delay_millis(5)
            .max_elapsed_time_millis(6)
            .build();
        assert_eq!(
            "BackOff[delay=2000, maxDelay=5, maxElapsedTime=6, remove=true]",
            b.to_string()
        );
        let mut copy = BackOff::builder_from(&b).remove_on_complete(false).build();
        assert!(!copy.is_remove_on_complete());
        copy.set_delay(Duration::from_millis(1));
        copy.set_max_attempts(1);
        copy.set_multiplier(3.0);
        copy.set_max_delay(MAX_DURATION);
        copy.set_max_elapsed_time(MAX_DURATION);
        copy.set_remove_on_complete(true);
        assert_eq!(
            "BackOff[delay=1, maxAttempts=1, multiplier=3.0, remove=true]",
            copy.to_string()
        );
        assert_eq!(
            BackOff::new(),
            BackOff::builder().read(&BackOff::default()).build()
        );
    }

    // --- SimpleBackOffTimerTest ---

    fn scheduler() -> ScheduledThreadPoolExecutor {
        ScheduledThreadPoolExecutor::new(3, "backoff-test")
    }

    #[test]
    fn test_back_off_timer() {
        let latch = Arc::new(CountDownLatch::new(1));
        let counter = Arc::new(AtomicI64::new(0));
        let first = Arc::new(AtomicI64::new(0));
        let executor = scheduler();
        let timer = SimpleBackOffTimer::new(executor.clone());
        let (c, f) = (counter.clone(), first.clone());
        let task = timer.schedule(
            BackOff::builder()
                .delay_millis(100)
                .remove_on_complete(false)
                .build(),
            move |t| {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                assert_eq!(n, t.current_attempts());
                assert_eq!(100, t.current_delay());
                assert_eq!(100 * n, t.current_elapsed_time());
                if f.load(Ordering::SeqCst) == 0 {
                    f.store(t.first_attempt_time(), Ordering::SeqCst);
                } else {
                    assert_eq!(f.load(Ordering::SeqCst), t.first_attempt_time());
                }
                Ok(n < 5)
            },
        );
        let (c, l) = (counter.clone(), latch.clone());
        task.when_complete(move |_, _| {
            assert_eq!(5, c.load(Ordering::SeqCst));
            l.count_down();
        });
        assert!(latch.await_timeout(Duration::from_secs(5)));
        executor.shutdown_now();
        assert_eq!(1, timer.size());
        assert_eq!(Status::Completed, timer.tasks()[0].status());
        timer.close();
    }

    #[test]
    fn test_back_off_timer_with_max_attempts() {
        let latch = Arc::new(CountDownLatch::new(1));
        let counter = Arc::new(AtomicI64::new(0));
        let executor = scheduler();
        let timer = SimpleBackOffTimer::new(executor.clone());
        let c = counter.clone();
        let task = timer.schedule(
            BackOff::builder()
                .delay_millis(100)
                .max_attempts(5)
                .remove_on_complete(false)
                .build(),
            move |t| {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                assert_eq!(n, t.current_attempts());
                assert_eq!(100, t.current_delay());
                assert_eq!(100 * n, t.current_elapsed_time());
                Ok(true)
            },
        );
        let (c, l) = (counter.clone(), latch.clone());
        task.when_complete(move |t, _| {
            assert_eq!(5, c.load(Ordering::SeqCst));
            assert_eq!(Status::Exhausted, t.status());
            l.count_down();
        });
        assert!(latch.await_timeout(Duration::from_secs(5)));
        executor.shutdown_now();
        assert_eq!(1, timer.size());
        assert_eq!(Status::Exhausted, timer.tasks()[0].status());
        timer.close();
    }

    #[test]
    fn test_back_off_timer_with_max_elapsed_time() {
        let latch = Arc::new(CountDownLatch::new(1));
        let counter = Arc::new(AtomicI64::new(0));
        let executor = scheduler();
        let timer = SimpleBackOffTimer::new(executor.clone());
        let c = counter.clone();
        let task = timer.schedule(
            BackOff::builder()
                .delay_millis(100)
                .max_elapsed_time_millis(400)
                .remove_on_complete(false)
                .build(),
            move |t| {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                assert_eq!(n, t.current_attempts());
                assert_eq!(100, t.current_delay());
                assert_eq!(100 * n, t.current_elapsed_time());
                Ok(true)
            },
        );
        let (c, l) = (counter.clone(), latch.clone());
        task.when_complete(move |t, _| {
            assert!(c.load(Ordering::SeqCst) <= 5);
            assert_eq!(Status::Exhausted, t.status());
            l.count_down();
        });
        assert!(latch.await_timeout(Duration::from_secs(5)));
        executor.shutdown_now();
        assert_eq!(1, timer.size());
        assert_eq!(Status::Exhausted, timer.tasks()[0].status());
        timer.close();
    }

    #[test]
    fn test_back_off_timer_stop() {
        let latch = Arc::new(CountDownLatch::new(5));
        let done = Arc::new(AtomicBool::new(false));
        let executor = scheduler();
        let timer = SimpleBackOffTimer::new(executor.clone());
        let l = latch.clone();
        let task = timer.schedule(
            BackOff::builder()
                .delay_millis(100)
                .remove_on_complete(false)
                .build(),
            move |t| {
                assert_eq!(Status::Active, t.status());
                l.count_down();
                Ok(false)
            },
        );
        let d = done.clone();
        task.when_complete(move |t, _| {
            assert_eq!(Status::Inactive, t.status());
            d.store(true, Ordering::SeqCst);
        });
        // the function returns false, so it runs only once (the latch times out like in Java)
        latch.await_timeout(Duration::from_secs(2));
        assert_eq!(1, timer.size());
        assert_eq!(Status::Completed, timer.tasks()[0].status());
        task.cancel();
        assert_eq!(0, timer.size());
        assert!(done.load(Ordering::SeqCst));
        executor.shutdown_now();
        timer.close();
    }

    #[test]
    fn test_back_off_timer_remove_on_complete() {
        let latch = Arc::new(CountDownLatch::new(1));
        let counter = Arc::new(AtomicI64::new(0));
        let executor = scheduler();
        let timer = SimpleBackOffTimer::new(executor.clone());
        let c = counter.clone();
        let task = timer.schedule(
            BackOff::builder()
                .delay_millis(100)
                .remove_on_complete(true)
                .build(),
            move |t| {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                assert_eq!(n, t.current_attempts());
                Ok(n < 5)
            },
        );
        let (c, l) = (counter.clone(), latch.clone());
        task.when_complete(move |_, _| {
            assert_eq!(5, c.load(Ordering::SeqCst));
            l.count_down();
        });
        assert!(latch.await_timeout(Duration::from_secs(5)));
        executor.shutdown_now();
        // task is removed
        assert_eq!(0, timer.size());
        timer.close();
    }

    /// A reentrant lock like Java's `ReentrantLock` (std `Mutex` is not reentrant, and the
    /// owner thread runs the completion callback again when it cancels the task).
    #[derive(Default)]
    struct ReentrantLock {
        state: Mutex<(Option<std::thread::ThreadId>, usize)>,
        cond: std::sync::Condvar,
    }

    impl ReentrantLock {
        fn lock(&self) {
            let me = std::thread::current().id();
            let mut s = self.state.lock().unwrap();
            while s.0.is_some() && s.0 != Some(me) {
                s = self.cond.wait(s).unwrap();
            }
            s.0 = Some(me);
            s.1 += 1;
        }

        fn unlock(&self) {
            let mut s = self.state.lock().unwrap();
            s.1 -= 1;
            if s.1 == 0 {
                s.0 = None;
                self.cond.notify_all();
            }
        }
    }

    #[test]
    fn test_cancel_while_completion_callback_waits_for_lock() {
        // the completion callback takes a lock of the owner of the task (as the supervising
        // route controller does), and the owner cancels the task while it holds that lock
        let owner_lock = Arc::new(ReentrantLock::default());
        let owner_holds_lock = Arc::new(CountDownLatch::new(1));
        let in_callback = Arc::new(AtomicBool::new(false));
        let executor = ScheduledThreadPoolExecutor::new(1, "single");
        let owner = ThreadPoolExecutor::fixed(1, "owner");
        let timer = SimpleBackOffTimer::new(executor.clone());
        let ohl = owner_holds_lock.clone();
        let task = timer.schedule(BackOff::builder().delay_millis(10).build(), move |_| {
            ohl.await_timeout(Duration::from_secs(5));
            // done, so the task completes
            Ok(false)
        });
        let (lock, ic) = (owner_lock.clone(), in_callback.clone());
        task.when_complete(move |_, _| {
            ic.store(true, Ordering::SeqCst);
            lock.lock();
            lock.unlock();
        });
        let (lock, ohl, ic, t) = (
            owner_lock.clone(),
            owner_holds_lock.clone(),
            in_callback.clone(),
            task.clone(),
        );
        let cancel = submit(&owner, move || {
            lock.lock();
            ohl.count_down();
            // wait until the completion callback of the task waits for the lock
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !ic.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                std::thread::yield_now();
            }
            t.cancel();
            lock.unlock();
        })
        .unwrap();
        assert!(
            cancel.get(Duration::from_secs(5)).is_some(),
            "Cancelling the task should not deadlock"
        );
        owner.shutdown_now();
        executor.shutdown_now();
        timer.close();
        assert!(task
            .to_string()
            .starts_with("BackOffTimerTask[name=SimpleBackOffTimer"));
        assert_eq!("SimpleBackOffTimer", timer.name());
    }

    #[test]
    fn test_failure_and_remove() {
        #[derive(Debug)]
        struct Boom;
        impl fmt::Display for Boom {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("boom")
            }
        }
        impl Error for Boom {}
        let latch = Arc::new(CountDownLatch::new(1));
        let timer = SimpleBackOffTimer::with_name("t", scheduler());
        let task = timer.schedule(
            BackOff::builder()
                .delay_millis(5)
                .remove_on_complete(false)
                .build(),
            |_| Err(Arc::new(Boom)),
        );
        let l = latch.clone();
        task.when_complete(move |t, e| {
            assert_eq!(Status::Failed, t.status());
            assert_eq!("boom", e.unwrap().to_string());
            l.count_down();
        });
        assert!(latch.await_timeout(Duration::from_secs(5)));
        assert_eq!("boom", task.exception().unwrap().to_string());
        timer.remove(&task);
        assert_eq!(0, timer.size());
        // a back-off that is exhausted right away is cancelled at schedule time
        let never = timer.schedule(BackOff::builder().max_attempts(0).build(), |_| Ok(true));
        assert_eq!(Status::Inactive, never.status());
        assert_eq!(0, timer.size());
        assert_eq!(NEVER, never.next_attempt_time());
        assert_eq!(NEVER, never.last_attempt_time());
        assert_eq!("t", never.name());
        assert_eq!(0, never.backoff().max_attempts());
    }
}
