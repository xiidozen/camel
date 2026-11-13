//! The scheduled polling of a consumer (Java `ScheduledPollConsumer` and the scheduling
//! options of `ScheduledPollEndpoint`): a tokio task polls after the initial delay, then every
//! delay, with the poll strategy, greedy polling, backoff on idle polls or errors, a repeat
//! count and empty messages when idle.

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::processor::BoxFuture;
use camel_api::spi::poll_strategy::{
    poll_strategy_of, DefaultPollingConsumerPollStrategy, PolledConsumer, SharedPollStrategy,
};
use camel_util::uri_support::Parameters;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

use crate::component_support::get_and_remove_parameter;

/// The scheduling options of a polling consumer (Java `ScheduledPollEndpoint`).
#[derive(Clone)]
pub struct ScheduledPollConfig {
    /// Milliseconds before the first poll starts (default 1000).
    pub initial_delay: u64,
    /// Milliseconds before the next poll (default 500).
    pub delay: u64,
    /// Whether the delay is between the end of a poll and the next one (default `true`), else
    /// between the starts (fixed rate).
    pub use_fixed_delay: bool,
    /// The maximum number of polls (0: forever).
    pub repeat_count: u64,
    /// Whether to poll again at once when the previous poll polled messages.
    pub greedy: bool,
    /// Whether an empty exchange is sent when a poll polled nothing.
    pub send_empty_message_when_idle: bool,
    /// The number of polls skipped after the idle or error thresholds (0: no backoff).
    pub backoff_multiplier: u32,
    /// The number of subsequent idle polls that trigger the backoff.
    pub backoff_idle_threshold: u32,
    /// The number of subsequent failed polls that trigger the backoff.
    pub backoff_error_threshold: u32,
    /// Whether the scheduler starts with the consumer (default `true`).
    pub start_scheduler: bool,
    /// Whether the poll errors are routed to the error handler of the route.
    pub bridge_error_handler: bool,
    /// The poll strategy (the default one if `None`).
    pub poll_strategy: Option<SharedPollStrategy>,
}

impl Default for ScheduledPollConfig {
    fn default() -> Self {
        ScheduledPollConfig {
            initial_delay: 1000,
            delay: 500,
            use_fixed_delay: true,
            repeat_count: 0,
            greedy: false,
            send_empty_message_when_idle: false,
            backoff_multiplier: 0,
            backoff_idle_threshold: 0,
            backoff_error_threshold: 0,
            start_scheduler: true,
            bridge_error_handler: false,
            poll_strategy: None,
        }
    }
}

/// A time option in the time unit (a number), or a duration text such as `5s`.
fn time_option(
    parameters: &mut Parameters,
    name: &str,
    unit_millis: u64,
    default: u64,
) -> CamelResult<u64> {
    match get_and_remove_parameter::<String>(parameters, name)? {
        Some(t) => match t.trim().parse::<i64>() {
            Ok(n) => Ok((n.max(0) as u64).saturating_mul(unit_millis)),
            Err(_) => camel_util::time_utils::to_milli_seconds(&t)
                .map(|m| m.max(0) as u64)
                .map_err(|e| CamelError::illegal_argument(format!("Invalid {name}: {e}"))),
        },
        None => Ok(default),
    }
}

impl ScheduledPollConfig {
    /// Reads (and removes) the scheduling options of the uri parameters, with the defaults
    /// given: `initialDelay`, `delay`, `timeUnit`, `useFixedDelay`, `repeatCount`, `greedy`,
    /// `sendEmptyMessageWhenIdle`, `backoffMultiplier`, `backoffIdleThreshold`,
    /// `backoffErrorThreshold`, `startScheduler`, `bridgeErrorHandler`, `pollStrategy` (a
    /// `#bean`), and the accepted `runLoggingLevel`, `scheduler` (only the default) and
    /// `schedulerProperties`.
    ///
    /// # Errors
    ///
    /// An invalid option value, an unknown poll strategy bean or a custom scheduler.
    pub fn from_parameters(
        context: &Arc<dyn CamelContext>,
        parameters: &mut Parameters,
        defaults: ScheduledPollConfig,
    ) -> CamelResult<Self> {
        let unit = match get_and_remove_parameter::<String>(parameters, "timeUnit")? {
            None => 1,
            Some(u) => match u.to_ascii_uppercase().as_str() {
                "NANOSECONDS" | "MICROSECONDS" | "MILLISECONDS" => 1,
                "SECONDS" => 1000,
                "MINUTES" => 60_000,
                "HOURS" => 3_600_000,
                "DAYS" => 86_400_000,
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Invalid timeUnit: {u}"
                    )))
                }
            },
        };
        let mut c = defaults;
        c.initial_delay = time_option(parameters, "initialDelay", unit, c.initial_delay)?;
        c.delay = time_option(parameters, "delay", unit, c.delay)?;
        let flag = |p: &mut Parameters, name: &str, d: bool| {
            get_and_remove_parameter::<bool>(p, name).map(|v| v.unwrap_or(d))
        };
        let number = |p: &mut Parameters, name: &str, d: u64| {
            get_and_remove_parameter::<i64>(p, name).map(|v| v.map_or(d, |n| n.max(0) as u64))
        };
        c.use_fixed_delay = flag(parameters, "useFixedDelay", c.use_fixed_delay)?;
        c.repeat_count = number(parameters, "repeatCount", c.repeat_count)?;
        c.greedy = flag(parameters, "greedy", c.greedy)?;
        c.send_empty_message_when_idle = flag(
            parameters,
            "sendEmptyMessageWhenIdle",
            c.send_empty_message_when_idle,
        )?;
        c.backoff_multiplier =
            number(parameters, "backoffMultiplier", c.backoff_multiplier.into())? as u32;
        c.backoff_idle_threshold = number(
            parameters,
            "backoffIdleThreshold",
            c.backoff_idle_threshold.into(),
        )? as u32;
        c.backoff_error_threshold = number(
            parameters,
            "backoffErrorThreshold",
            c.backoff_error_threshold.into(),
        )? as u32;
        if c.backoff_multiplier > 0
            && c.backoff_idle_threshold == 0
            && c.backoff_error_threshold == 0
        {
            return Err(CamelError::illegal_argument(
                "backoffIdleThreshold and/or backoffErrorThreshold must be configured to a positive value when using backoffMultiplier",
            ));
        }
        c.start_scheduler = flag(parameters, "startScheduler", c.start_scheduler)?;
        c.bridge_error_handler = flag(parameters, "bridgeErrorHandler", c.bridge_error_handler)?;
        if let Some(s) = get_and_remove_parameter::<String>(parameters, "pollStrategy")? {
            let name = s.strip_prefix('#').unwrap_or(&s);
            let name = name.strip_prefix("bean:").unwrap_or(name);
            let strategy = context
                .registry()
                .and_then(|r| r.lookup_by_name(name))
                .as_ref()
                .and_then(poll_strategy_of)
                .ok_or_else(|| CamelError::no_such_bean(name))?;
            c.poll_strategy = Some(strategy);
        }
        let _: Option<String> = get_and_remove_parameter(parameters, "runLoggingLevel")?;
        if let Some(s) = get_and_remove_parameter::<String>(parameters, "scheduler")? {
            if s != "none" && s != "default" {
                return Err(CamelError::illegal_argument(format!(
                    "The scheduler {s} is not supported by the Rust port"
                )));
            }
        }
        parameters.retain(|k, _| !k.starts_with("scheduler."));
        let _: Option<String> = get_and_remove_parameter(parameters, "schedulerProperties")?;
        Ok(c)
    }
}

/// The polling of a consumer, run by a [`ScheduledPollConsumer`].
pub trait PollTask: Send + Sync {
    /// The uri of the endpoint polled, for logging.
    fn endpoint_uri(&self) -> &str;

    /// Polls once (Java `ScheduledPollConsumer.poll`), routing what was polled.
    ///
    /// # Returns
    ///
    /// The number of polled messages.
    ///
    /// # Errors
    ///
    /// A failed poll, given to the poll strategy.
    fn poll(&self) -> BoxFuture<'_, CamelResult<usize>>;

    /// Routes an empty exchange, with `sendEmptyMessageWhenIdle` (Java
    /// `processEmptyMessage`).
    ///
    /// # Errors
    ///
    /// The failure of the routing.
    fn process_empty_message(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// Handles a failed poll the strategy does not retry: routed to the error handler of the
    /// route with `bridgeErrorHandler`, else logged (Java `getExceptionHandler()`).
    fn handle_poll_error(&self, error: CamelError, bridge: bool) -> BoxFuture<'_, ()> {
        let _ = bridge;
        log::warn!(
            "Failed polling endpoint: {}. Will try again at next poll. Caused by: [{error}]",
            self.endpoint_uri()
        );
        Box::pin(std::future::ready(()))
    }
}

#[derive(Default)]
struct State {
    stopped: AtomicBool,
    wake: Notify,
}

impl State {
    /// Sleeps the millis unless stopped; whether still running.
    async fn sleep(&self, millis: u64) -> bool {
        if self.stopped.load(Ordering::SeqCst) {
            return false;
        }
        if millis > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(millis)) => {}
                _ = self.wake.notified() => {}
            }
        }
        !self.stopped.load(Ordering::SeqCst)
    }
}

struct Handle {
    uri: String,
    state: Arc<State>,
}

impl PolledConsumer for Handle {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn stop(&self) {
        self.state.stopped.store(true, Ordering::SeqCst);
        self.state.wake.notify_waiters();
    }
}

/// The counters of the backoff (Java `ScheduledPollConsumer`).
#[derive(Default)]
struct Counters {
    polls: u64,
    idle: u64,
    errors: u64,
    backoff: u32,
}

/// Runs a [`PollTask`] on a tokio task with the scheduling options (Java
/// `ScheduledPollConsumer`).
pub struct ScheduledPollConsumer {
    config: ScheduledPollConfig,
    task: Arc<dyn PollTask>,
    state: Mutex<Arc<State>>,
    join: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ScheduledPollConsumer {
    /// The consumer running the task.
    pub fn new(config: ScheduledPollConfig, task: Arc<dyn PollTask>) -> Self {
        ScheduledPollConsumer {
            config,
            task,
            state: Mutex::new(Arc::new(State::default())),
            join: Mutex::new(None),
        }
    }

    /// The scheduling options.
    pub fn config(&self) -> &ScheduledPollConfig {
        &self.config
    }

    /// Starts the polling task, unless `startScheduler` is off.
    ///
    /// # Errors
    ///
    /// Not within a tokio runtime.
    pub fn start(&self) -> CamelResult<()> {
        if !self.config.start_scheduler {
            return Ok(());
        }
        self.start_scheduler()
    }

    /// Starts the polling task (also when `startScheduler` is off, Java `startScheduler()`).
    ///
    /// # Errors
    ///
    /// Not within a tokio runtime.
    pub fn start_scheduler(&self) -> CamelResult<()> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(CamelError::illegal_state(
                "The scheduled poll consumer must be started within a tokio runtime",
            ));
        }
        let mut join = self.join.lock().unwrap();
        if join.as_ref().is_some_and(|j| !j.is_finished()) {
            return Ok(());
        }
        let state = Arc::new(State::default());
        *self.state.lock().unwrap() = state.clone();
        *join = Some(tokio::spawn(run(
            self.config.clone(),
            self.task.clone(),
            state,
        )));
        Ok(())
    }

    /// Stops the polling task (a running poll completes).
    pub fn stop(&self) {
        let state = self.state.lock().unwrap().clone();
        state.stopped.store(true, Ordering::SeqCst);
        state.wake.notify_waiters();
        self.join.lock().unwrap().take();
    }
}

/// Java `ScheduledPollConsumer.run`/`doRun`, scheduled.
async fn run(config: ScheduledPollConfig, task: Arc<dyn PollTask>, state: Arc<State>) {
    if !state.sleep(config.initial_delay).await {
        return;
    }
    let handle = Handle {
        uri: task.endpoint_uri().to_string(),
        state: state.clone(),
    };
    let default_strategy: SharedPollStrategy = Arc::new(DefaultPollingConsumerPollStrategy);
    let strategy = config.poll_strategy.clone().unwrap_or(default_strategy);
    let mut counters = Counters::default();
    loop {
        let started = tokio::time::Instant::now();
        if !do_run(&config, &task, &state, &handle, &strategy, &mut counters).await {
            break;
        }
        let wait = if config.use_fixed_delay {
            config.delay
        } else {
            config
                .delay
                .saturating_sub(started.elapsed().as_millis() as u64)
        };
        if !state.sleep(wait.max(1)).await {
            break;
        }
    }
}

/// One scheduled run; whether to keep on scheduling.
async fn do_run(
    config: &ScheduledPollConfig,
    task: &Arc<dyn PollTask>,
    state: &State,
    handle: &Handle,
    strategy: &SharedPollStrategy,
    c: &mut Counters,
) -> bool {
    if config.backoff_multiplier > 0 {
        let idle_threshold = if config.backoff_idle_threshold > 0 {
            u64::from(config.backoff_idle_threshold)
        } else {
            u64::MAX
        };
        let error_threshold = if config.backoff_error_threshold > 0 {
            u64::from(config.backoff_error_threshold)
        } else {
            u64::MAX
        };
        if c.idle >= idle_threshold || c.errors >= error_threshold {
            c.backoff += 1;
            if c.backoff < config.backoff_multiplier {
                log::debug!(
                    "doRun() backoff due subsequent {} idles / {} errors (backoff at {}/{})",
                    c.idle,
                    c.errors,
                    c.backoff,
                    config.backoff_multiplier
                );
                return true;
            }
            c.idle = 0;
            c.errors = 0;
            c.backoff = 0;
        }
    }
    c.polls += 1;
    if config.repeat_count > 0 && c.polls > config.repeat_count {
        log::debug!(
            "Cancelling {} scheduler as repeat count limit reached after {} counts.",
            task.endpoint_uri(),
            config.repeat_count
        );
        return false;
    }
    let mut retry_counter: i32 = -1;
    let mut polled = 0;
    let mut cause = None;
    loop {
        if state.stopped.load(Ordering::SeqCst) {
            return false;
        }
        let attempt: CamelResult<bool> = async {
            if !strategy.begin(handle)? {
                log::debug!("Cannot begin polling as pollStrategy returned false");
                return Ok(false);
            }
            retry_counter += 1;
            polled = task.poll().await?;
            if polled == 0 && config.send_empty_message_when_idle {
                task.process_empty_message().await?;
            }
            strategy.commit(handle, polled);
            // greedy: poll again at once
            Ok(polled > 0 && config.greedy)
        }
        .await;
        match attempt {
            Ok(true) => {
                retry_counter = -1;
                c.errors = 0;
            }
            Ok(false) => break,
            Err(e) => match strategy.rollback(handle, retry_counter, &e) {
                Ok(true) => {
                    log::debug!(
                        "Retrying attempt {retry_counter} to poll: {}",
                        task.endpoint_uri()
                    );
                }
                Ok(false) => {
                    cause = Some(e);
                    break;
                }
                Err(e) => {
                    cause = Some(e);
                    break;
                }
            },
        }
    }
    if state.stopped.load(Ordering::SeqCst) {
        return false;
    }
    match cause {
        Some(e) => {
            c.idle = 0;
            c.errors += 1;
            task.handle_poll_error(e, config.bridge_error_handler).await;
        }
        None => {
            if polled == 0 {
                c.idle += 1;
            } else {
                c.idle = 0;
            }
            c.errors = 0;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_parameters() {
        let context: Arc<dyn CamelContext> = camel_api::context::StandaloneContext::shared();
        let mut p = Parameters::new();
        for (k, v) in [
            ("delay", "2"),
            ("timeUnit", "SECONDS"),
            ("initialDelay", "250"),
            ("greedy", "true"),
            ("repeatCount", "3"),
        ] {
            p.insert(k.to_string(), v.into());
        }
        let c =
            ScheduledPollConfig::from_parameters(&context, &mut p, ScheduledPollConfig::default())
                .unwrap();
        assert_eq!(2000, c.delay);
        assert_eq!(250_000, c.initial_delay);
        assert!(c.greedy);
        assert_eq!(3, c.repeat_count);
        assert!(p.is_empty());
        let mut p = Parameters::new();
        p.insert("backoffMultiplier".to_string(), "2".into());
        assert!(ScheduledPollConfig::from_parameters(
            &context,
            &mut p,
            ScheduledPollConfig::default()
        )
        .is_err());
    }
}
