//! The redelivery policy (Java `RedeliveryPolicy`): how many times and how often a failed
//! exchange is redelivered, and what is logged.

use camel_api::enums::LoggingLevel;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::SharedPredicate;

/// The redelivery policy. The defaults are those of Java: no redelivery, 1 second delay,
/// exhausted failures logged at `ERROR` with the stack trace.
#[derive(Debug, Clone, PartialEq)]
pub struct RedeliveryPolicy {
    /// The delay (millis) before the first redelivery.
    pub redelivery_delay: u64,
    /// The maximum number of redeliveries (-1 forever, 0 no redelivery).
    pub maximum_redeliveries: i32,
    /// The maximum delay (millis) between redeliveries.
    pub maximum_redelivery_delay: u64,
    /// The multiplier of the exponential back off.
    pub back_off_multiplier: f64,
    /// Whether the delay grows exponentially.
    pub use_exponential_back_off: bool,
    /// The variance of the collision avoidance (0.15 is 15%).
    pub collision_avoidance_factor: f64,
    /// Whether a random variance is added to the delays.
    pub use_collision_avoidance: bool,
    /// The level to log exhausted failures at.
    pub retries_exhausted_log_level: LoggingLevel,
    /// The level to log redelivery attempts at.
    pub retry_attempted_log_level: LoggingLevel,
    /// Log every n-th redelivery attempt.
    pub retry_attempted_log_interval: i32,
    /// Whether to log the stack trace of exhausted failures.
    pub log_stack_trace: bool,
    /// Whether to log the stack trace of redelivery attempts.
    pub log_retry_stack_trace: bool,
    /// Whether to log handled failures.
    pub log_handled: bool,
    /// Whether to log continued failures.
    pub log_continued: bool,
    /// Whether to log exhausted failures.
    pub log_exhausted: bool,
    /// Whether to log new exceptions of the failure processor.
    pub log_new_exception: bool,
    /// Whether to log redelivery attempts.
    pub log_retry_attempted: bool,
    /// A delay pattern such as `0:1000;5:5000` (from attempt 5 the delay is 5 seconds).
    pub delay_pattern: Option<String>,
    /// Whether redeliveries are allowed while the context is stopping.
    pub allow_redelivery_while_stopping: bool,
}

impl Default for RedeliveryPolicy {
    fn default() -> Self {
        RedeliveryPolicy {
            redelivery_delay: 1000,
            maximum_redeliveries: 0,
            maximum_redelivery_delay: 60_000,
            back_off_multiplier: 2.0,
            use_exponential_back_off: false,
            collision_avoidance_factor: 0.15,
            use_collision_avoidance: false,
            retries_exhausted_log_level: LoggingLevel::ERROR,
            retry_attempted_log_level: LoggingLevel::DEBUG,
            retry_attempted_log_interval: 1,
            log_stack_trace: true,
            log_retry_stack_trace: false,
            log_handled: false,
            log_continued: false,
            log_exhausted: true,
            log_new_exception: true,
            log_retry_attempted: true,
            delay_pattern: None,
            allow_redelivery_while_stopping: true,
        }
    }
}

impl RedeliveryPolicy {
    /// Whether to redeliver the exchange for the attempt (Java `shouldRedeliver`): the
    /// `retryWhile` predicate if any, else the maximum redeliveries.
    ///
    /// # Errors
    ///
    /// Returns the error of the predicate.
    pub fn should_redeliver(
        &self,
        exchange: &Exchange,
        redelivery_counter: i32,
        retry_while: Option<&SharedPredicate>,
    ) -> CamelResult<bool> {
        if let Some(p) = retry_while {
            return p.matches(exchange);
        }
        if self.maximum_redeliveries < 0 {
            return Ok(true);
        }
        Ok(redelivery_counter <= self.maximum_redeliveries)
    }

    /// The delay before the attempt (Java `calculateRedeliveryDelay`).
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_core_processor::errorhandler::redelivery_policy::RedeliveryPolicy;
    /// let mut p = RedeliveryPolicy { redelivery_delay: 100, use_exponential_back_off: true, ..Default::default() };
    /// assert_eq!(100, p.calculate_redelivery_delay(0, 1));
    /// assert_eq!(200, p.calculate_redelivery_delay(100, 2));
    /// p.delay_pattern = Some("0:10;3:50".into());
    /// assert_eq!(10, p.calculate_redelivery_delay(0, 2));
    /// assert_eq!(50, p.calculate_redelivery_delay(0, 3));
    /// ```
    pub fn calculate_redelivery_delay(&self, previous_delay: u64, redelivery_counter: i32) -> u64 {
        if let Some(pattern) = self.delay_pattern.as_deref().filter(|p| !p.is_empty()) {
            return delay_using_pattern(pattern, redelivery_counter);
        }
        let mut result = if previous_delay == 0 {
            self.redelivery_delay
        } else if self.use_exponential_back_off && self.back_off_multiplier > 1.0 {
            (self.back_off_multiplier * previous_delay as f64).round() as u64
        } else {
            previous_delay
        };
        if self.use_collision_avoidance {
            let up = camel_util::secure_random_helper::next_below(2) == 1;
            let fraction =
                camel_util::secure_random_helper::next_below(1_000_000) as f64 / 1_000_000.0;
            let variance = (if up {
                self.collision_avoidance_factor
            } else {
                -self.collision_avoidance_factor
            }) * fraction;
            result = (result as f64 + result as f64 * variance) as u64;
        }
        if self.maximum_redelivery_delay > 0 && result > self.maximum_redelivery_delay {
            result = self.maximum_redelivery_delay;
        }
        result
    }
}

/// The delay of the pattern for the attempt (Java `calculateRedeliverDelayUsingPattern`).
fn delay_using_pattern(pattern: &str, counter: i32) -> u64 {
    let mut answer = 0;
    for group in pattern.split(';') {
        let Some((count, delay)) = group.split_once(':') else {
            continue;
        };
        let (Ok(count), Ok(delay)) = (count.trim().parse::<i32>(), delay.trim().parse::<u64>())
        else {
            continue;
        };
        if count > counter {
            break;
        }
        answer = delay;
    }
    answer
}

/// Overrides of the redelivery policy of an exception policy (`onException`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RedeliveryPolicyOverrides {
    /// See [`RedeliveryPolicy::maximum_redeliveries`].
    pub maximum_redeliveries: Option<i32>,
    /// See [`RedeliveryPolicy::redelivery_delay`].
    pub redelivery_delay: Option<u64>,
    /// See [`RedeliveryPolicy::use_exponential_back_off`].
    pub use_exponential_back_off: Option<bool>,
    /// See [`RedeliveryPolicy::back_off_multiplier`].
    pub back_off_multiplier: Option<f64>,
    /// See [`RedeliveryPolicy::maximum_redelivery_delay`].
    pub maximum_redelivery_delay: Option<u64>,
    /// See [`RedeliveryPolicy::retries_exhausted_log_level`].
    pub retries_exhausted_log_level: Option<LoggingLevel>,
    /// See [`RedeliveryPolicy::retry_attempted_log_level`].
    pub retry_attempted_log_level: Option<LoggingLevel>,
    /// See [`RedeliveryPolicy::log_stack_trace`].
    pub log_stack_trace: Option<bool>,
    /// See [`RedeliveryPolicy::log_handled`].
    pub log_handled: Option<bool>,
    /// See [`RedeliveryPolicy::log_exhausted`].
    pub log_exhausted: Option<bool>,
    /// See [`RedeliveryPolicy::delay_pattern`].
    pub delay_pattern: Option<String>,
}

impl RedeliveryPolicyOverrides {
    /// The policy with these overrides applied on the parent (Java
    /// `ExceptionPolicy.createRedeliveryPolicy`).
    pub fn apply(&self, parent: &RedeliveryPolicy) -> RedeliveryPolicy {
        let mut p = parent.clone();
        if let Some(v) = self.maximum_redeliveries {
            p.maximum_redeliveries = v;
        }
        if let Some(v) = self.redelivery_delay {
            p.redelivery_delay = v;
        }
        if let Some(v) = self.use_exponential_back_off {
            p.use_exponential_back_off = v;
        }
        if let Some(v) = self.back_off_multiplier {
            p.back_off_multiplier = v;
        }
        if let Some(v) = self.maximum_redelivery_delay {
            p.maximum_redelivery_delay = v;
        }
        if let Some(v) = self.retries_exhausted_log_level {
            p.retries_exhausted_log_level = v;
        }
        if let Some(v) = self.retry_attempted_log_level {
            p.retry_attempted_log_level = v;
        }
        if let Some(v) = self.log_stack_trace {
            p.log_stack_trace = v;
        }
        if let Some(v) = self.log_handled {
            p.log_handled = v;
        }
        if let Some(v) = self.log_exhausted {
            p.log_exhausted = v;
        }
        if let Some(v) = &self.delay_pattern {
            p.delay_pattern = Some(v.clone());
        }
        p
    }
}
