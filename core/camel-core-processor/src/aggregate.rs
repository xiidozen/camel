//! The aggregate EIP and the aggregation strategies (Java
//! `org.apache.camel.processor.aggregate`).

use camel_api::convert::convert_to;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    AGGREGATED_COMPLETED_BY, AGGREGATED_CORRELATION_KEY, AGGREGATED_SIZE,
    AGGREGATION_COMPLETE_ALL_GROUPS, AGGREGATION_COMPLETE_ALL_GROUPS_INCLUSIVE,
    AGGREGATION_COMPLETE_CURRENT_GROUP, BATCH_SIZE, FAILURE_ENDPOINT, GROUPED_EXCHANGE,
};
use camel_api::processor::{
    AggregationStrategy, BoxFuture, Processor, SharedAggregationStrategy, SharedExpression,
    SharedPredicate, SharedProcessor,
};
use camel_api::spi::aggregation_repository::AggregationRepository;
use camel_support::exchange_helper::{create_correlated_copy, prepare_out_to_in};
use camel_util::value::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Uses the latest exchange, propagating the errors and failures of the previous ones (Java
/// `UseLatestAggregationStrategy`).
#[derive(Debug, Default, Clone, Copy)]
pub struct UseLatestAggregationStrategy;

impl AggregationStrategy for UseLatestAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        let Some(mut new) = new_exchange else {
            return Ok(old_exchange);
        };
        let Some(old) = old_exchange else {
            return Ok(Some(new));
        };
        // propagate the exception first
        if new.exception().is_none() {
            new.set_exception(old.exception().cloned());
            match old.property(FAILURE_ENDPOINT).cloned() {
                Some(v) => new.set_property(FAILURE_ENDPOINT, v),
                None => {
                    new.remove_property(FAILURE_ENDPOINT);
                }
            }
        }
        if new.exception().is_some() {
            return Ok(Some(new));
        }
        // then the failures
        let failed = old.is_failed()
            || old.is_rollback_only()
            || old.is_rollback_only_last()
            || old.error_handler_handled() == Some(true);
        Ok(Some(if failed { old } else { new }))
    }
}

/// Keeps the input exchange of the EIP (Java `UseOriginalAggregationStrategy`, the
/// `AggregationStrategies.useOriginal()` of multicast and recipient list): the result is the
/// original message, with the failure of a copy propagated if asked.
#[derive(Debug, Clone, Copy)]
pub struct UseOriginalAggregationStrategy {
    propagate_exception: bool,
}

impl UseOriginalAggregationStrategy {
    /// The strategy, propagating the failures of the copies or not.
    pub fn new(propagate_exception: bool) -> Self {
        UseOriginalAggregationStrategy {
            propagate_exception,
        }
    }
}

impl Default for UseOriginalAggregationStrategy {
    fn default() -> Self {
        Self::new(true)
    }
}

impl AggregationStrategy for UseOriginalAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        Ok(old_exchange.or(new_exchange))
    }

    fn aggregate_with_input(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
        input_exchange: &Exchange,
    ) -> CamelResult<Option<Exchange>> {
        let mut result = old_exchange.unwrap_or_else(|| input_exchange.clone());
        if self.propagate_exception {
            if let Some(new) = &new_exchange {
                if let Some(e) = new.exception() {
                    result.set_exception(Some(e.clone()));
                }
                if let Some(c) = new.property(camel_api::exchange_constants::EXCEPTION_CAUGHT) {
                    result.set_property(camel_api::exchange_constants::EXCEPTION_CAUGHT, c.clone());
                }
            }
        }
        Ok(Some(result))
    }
}

/// Keeps the first exchange (Java `UseOriginalAggregationStrategy` without original).
#[derive(Debug, Default, Clone, Copy)]
pub struct UseFirstAggregationStrategy;

impl AggregationStrategy for UseFirstAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        Ok(old_exchange.or(new_exchange))
    }
}

/// Keeps the exchanges of a group in a list, set as body on completion (Java
/// `GroupedExchangeAggregationStrategy`): the body of the completed exchange is a list of
/// [`Value::object`]s holding the [`Exchange`]s.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroupedExchangeAggregationStrategy;

impl AggregationStrategy for GroupedExchangeAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        let Some(new) = new_exchange else {
            return Ok(old_exchange);
        };
        // the holder must not be one of the grouped exchanges
        let mut old = old_exchange
            .unwrap_or_else(|| Exchange::with_pattern(new.context().clone(), new.pattern()));
        add_to_group(&mut old, Value::object(new));
        Ok(Some(old))
    }

    fn on_completion(&self, exchange: &mut Exchange, _input_exchange: Option<&Exchange>) {
        store_group_as_body(exchange);
    }
}

/// Keeps the bodies of a group in a list, set as body on completion (Java
/// `GroupedBodyAggregationStrategy`).
#[derive(Debug, Default, Clone, Copy)]
pub struct GroupedBodyAggregationStrategy;

impl AggregationStrategy for GroupedBodyAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        let Some(new) = new_exchange else {
            return Ok(old_exchange);
        };
        let body = new.message().body().clone();
        let mut old = old_exchange.unwrap_or(new);
        if !body.is_null() {
            add_to_group(&mut old, body);
        }
        Ok(Some(old))
    }

    fn on_completion(&self, exchange: &mut Exchange, _input_exchange: Option<&Exchange>) {
        store_group_as_body(exchange);
    }
}

fn add_to_group(exchange: &mut Exchange, value: Value) {
    let mut list = match exchange.remove_property(GROUPED_EXCHANGE) {
        Some(Value::List(l)) => l,
        _ => Vec::new(),
    };
    list.push(value);
    exchange.set_property(GROUPED_EXCHANGE, Value::List(list));
}

fn store_group_as_body(exchange: &mut Exchange) {
    if let Some(list) = exchange.remove_property(GROUPED_EXCHANGE) {
        exchange.message_mut().set_body(list);
    }
}

/// The in-memory aggregation repository (Java `MemoryAggregationRepository`).
#[derive(Default)]
pub struct MemoryAggregationRepository {
    cache: Mutex<HashMap<String, Exchange>>,
}

impl AggregationRepository for MemoryAggregationRepository {
    fn add(&self, key: &str, exchange: Exchange) -> Option<Exchange> {
        self.cache.lock().unwrap().insert(key.to_string(), exchange)
    }

    fn get(&self, key: &str) -> Option<Exchange> {
        self.cache.lock().unwrap().get(key).cloned()
    }

    fn remove(&self, key: &str, _exchange: &Exchange) {
        self.cache.lock().unwrap().remove(key);
    }

    fn confirm(&self, _exchange_id: &str) {
        // noop
    }

    fn keys(&self) -> Vec<String> {
        self.cache.lock().unwrap().keys().cloned().collect()
    }
}

/// The values of the `CamelAggregatedCompletedBy` property (Java `AggregateProcessor.COMPLETED_BY_*`).
pub mod completed_by {
    /// The completion size was reached.
    pub const SIZE: &str = "size";
    /// The completion predicate matched.
    pub const PREDICATE: &str = "predicate";
    /// The batch consumer completed.
    pub const CONSUMER: &str = "consumer";
    /// The aggregation strategy completed the group.
    pub const STRATEGY: &str = "strategy";
    /// The completion interval elapsed.
    pub const INTERVAL: &str = "interval";
    /// The group was inactive for the completion timeout.
    pub const TIMEOUT: &str = "timeout";
    /// The completion was forced.
    pub const FORCE: &str = "force";
}

/// The options of the aggregator (Java `AggregateDefinition`).
#[derive(Clone)]
pub struct AggregateOptions {
    /// The number of exchanges of a complete group (0 for none).
    pub completion_size: i32,
    /// The completion size per group, overriding `completion_size` when positive.
    pub completion_size_expression: Option<SharedExpression>,
    /// The inactivity timeout in millis after which a group completes (0 for none).
    pub completion_timeout: i64,
    /// The timeout per group, overriding `completion_timeout` when positive.
    pub completion_timeout_expression: Option<SharedExpression>,
    /// How often the timeouts are checked, in millis (default 1000).
    pub completion_timeout_checker_interval: u64,
    /// Completes all groups every interval millis (0 for none).
    pub completion_interval: i64,
    /// Completes the group when it matches.
    pub completion_predicate: Option<SharedPredicate>,
    /// Checks the completion on the incoming exchange, before the aggregation.
    pub eager_check_completion: bool,
    /// Ignores exchanges without correlation key instead of failing them.
    pub ignore_invalid_correlation_keys: bool,
    /// Closes the correlation keys of completed groups, remembering at most that many keys
    /// (0 for no limit); later exchanges of a closed key fail.
    pub close_correlation_key_on_completion: Option<usize>,
    /// Discards (does not send on) groups completed by timeout.
    pub discard_on_completion_timeout: bool,
    /// Discards the group when the aggregation strategy fails.
    pub discard_on_aggregation_failure: bool,
    /// Completes all groups when a new correlation group starts.
    pub completion_on_new_correlation_group: bool,
    /// Completes the groups of a batch of a batch consumer (such as the file consumer) when
    /// its `CamelBatchSize` exchanges arrived.
    pub completion_from_batch_consumer: bool,
    /// Completes all groups when the route stops.
    pub force_completion_on_stop: bool,
    /// Completes all groups when the route stops (Java: also waits for them to complete).
    pub complete_all_on_stop: bool,
    /// The repository (default in memory).
    pub repository: Option<Arc<dyn AggregationRepository>>,
}

impl Default for AggregateOptions {
    fn default() -> Self {
        AggregateOptions {
            completion_size: 0,
            completion_size_expression: None,
            completion_timeout: 0,
            completion_timeout_expression: None,
            completion_timeout_checker_interval: 1000,
            completion_interval: 0,
            completion_predicate: None,
            eager_check_completion: false,
            ignore_invalid_correlation_keys: false,
            close_correlation_key_on_completion: None,
            discard_on_completion_timeout: false,
            discard_on_aggregation_failure: false,
            completion_on_new_correlation_group: false,
            completion_from_batch_consumer: false,
            force_completion_on_stop: false,
            complete_all_on_stop: false,
            repository: None,
        }
    }
}

/// The closed correlation keys, oldest first evicted beyond the capacity.
struct ClosedKeys {
    capacity: usize,
    keys: VecDeque<String>,
    set: HashSet<String>,
}

impl ClosedKeys {
    fn insert(&mut self, key: &str) {
        if self.set.insert(key.to_string()) {
            self.keys.push_back(key.to_string());
            if self.capacity > 0 && self.keys.len() > self.capacity {
                if let Some(old) = self.keys.pop_front() {
                    self.set.remove(&old);
                }
            }
        }
    }
}

#[derive(Default)]
struct State {
    /// The deadline of each group with a timeout.
    timeouts: HashMap<String, Instant>,
    closed: Option<ClosedKeys>,
    /// The groups of the current batch of a batch consumer, in order.
    batch_keys: Vec<String>,
    /// The number of exchanges of the current batch.
    batch_counter: i64,
}

struct Inner {
    correlation: SharedExpression,
    strategy: SharedAggregationStrategy,
    processor: SharedProcessor,
    options: AggregateOptions,
    repository: Arc<dyn AggregationRepository>,
    state: Mutex<State>,
    /// The timeout checker and interval tasks, `None` until started.
    background: Mutex<Option<Vec<JoinHandle<()>>>>,
}

/// Aggregates the exchanges with the same correlation key into one exchange, sent to the
/// outputs when the group completes (the aggregate EIP; Java `AggregateProcessor`).
///
/// A group completes by size, predicate, timeout (inactivity), interval, the strategy
/// (pre-completion or the `CamelAggregationCompleteCurrentGroup` property) or force (the
/// `CamelAggregationCompleteAllGroups` header or property). Completed groups are processed
/// before the incoming exchange returns (Java: the synchronous executor); groups completed by
/// timeout or interval are processed by a background task.
///
/// Not ported yet: parallel processing, optimistic locking, recoverable repositories, the
/// batch consumer completion, forceCompletionOnStop and completeAllOnStop.
pub struct AggregateProcessor {
    inner: Arc<Inner>,
}

impl AggregateProcessor {
    /// Creates the aggregator.
    ///
    /// # Errors
    ///
    /// Returns an `IllegalStateException` if no completion option is set (unless the strategy
    /// pre-completes).
    pub fn new(
        correlation: SharedExpression,
        strategy: SharedAggregationStrategy,
        processor: SharedProcessor,
        options: AggregateOptions,
    ) -> CamelResult<Self> {
        if !strategy.can_pre_complete()
            && options.completion_timeout <= 0
            && options.completion_interval <= 0
            && options.completion_size <= 0
            && options.completion_predicate.is_none()
            && options.completion_timeout_expression.is_none()
            && options.completion_size_expression.is_none()
            && !options.completion_from_batch_consumer
        {
            return Err(CamelError::illegal_state(
                "At least one of the completions options [completionTimeout, completionInterval, completionSize, completionPredicate, completionFromBatchConsumer] must be set",
            ));
        }
        let repository = options
            .repository
            .clone()
            .unwrap_or_else(|| Arc::new(MemoryAggregationRepository::default()));
        let closed = options
            .close_correlation_key_on_completion
            .map(|capacity| ClosedKeys {
                capacity,
                keys: VecDeque::new(),
                set: HashSet::new(),
            });
        Ok(AggregateProcessor {
            inner: Arc::new(Inner {
                correlation,
                strategy,
                processor,
                options,
                repository,
                state: Mutex::new(State {
                    timeouts: HashMap::new(),
                    closed,
                    batch_keys: Vec::new(),
                    batch_counter: 0,
                }),
                background: Mutex::new(None),
            }),
        })
    }

    /// The repository of the groups.
    pub fn repository(&self) -> &Arc<dyn AggregationRepository> {
        &self.inner.repository
    }

    /// Completes all the groups now (Java `forceCompletionOfAllGroups`).
    pub async fn force_completion_of_all_groups(&self) {
        let list = self.inner.complete_all(completed_by::FORCE);
        self.inner.submit_all(list).await;
    }
}

fn flag(exchange: &Exchange, header: bool, name: &str) -> bool {
    let v = if header {
        exchange.message().header(name)
    } else {
        exchange.property(name)
    };
    v.map(|v| convert_to::<bool>(v).ok().flatten().unwrap_or(false))
        .unwrap_or(false)
}

fn int_of(exchange: &Exchange, e: &Option<SharedExpression>) -> Option<i64> {
    e.as_ref()
        .and_then(|e| e.evaluate(exchange).ok())
        .and_then(|v| convert_to::<i64>(&v).ok().flatten())
}

fn size_of(exchange: &Exchange) -> i64 {
    exchange
        .property(AGGREGATED_SIZE)
        .and_then(|v| convert_to::<i64>(v).ok().flatten())
        .unwrap_or(1)
}

impl Inner {
    /// Starts the timeout checker and the interval task, unless started (when the route
    /// starts, or on the first exchange outside a route).
    fn start_background(self: &Arc<Self>) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let mut background = self.background.lock().unwrap();
        if background.is_some() {
            return;
        }
        let mut handles = Vec::new();
        {
            let has_timeout = self.options.completion_timeout > 0
                || self.options.completion_timeout_expression.is_some();
            if has_timeout {
                let weak = Arc::downgrade(self);
                let every =
                    Duration::from_millis(self.options.completion_timeout_checker_interval.max(1));
                handles.push(tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(every).await;
                        let Some(inner) = weak.upgrade() else { break };
                        let list = inner.complete_timed_out();
                        inner.submit_all(list).await;
                    }
                }));
            }
            if self.options.completion_interval > 0 {
                let weak = Arc::downgrade(self);
                let every = Duration::from_millis(self.options.completion_interval as u64);
                handles.push(tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(every).await;
                        let Some(inner) = weak.upgrade() else { break };
                        let list = inner.complete_all(completed_by::INTERVAL);
                        inner.submit_all(list).await;
                    }
                }));
            }
        }
        *background = Some(handles);
    }

    /// Stops the background tasks.
    fn stop_background(&self) {
        if let Some(handles) = self.background.lock().unwrap().take() {
            for h in handles {
                h.abort();
            }
        }
    }

    /// Completes the groups whose timeout has passed (Java `AggregationTimeoutMap.onEviction`).
    fn complete_timed_out(&self) -> Vec<(String, Exchange)> {
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let expired: Vec<String> = state
            .timeouts
            .iter()
            .filter(|(_, d)| **d <= now)
            .map(|(k, _)| k.clone())
            .collect();
        let mut list = Vec::new();
        for key in expired {
            state.timeouts.remove(&key);
            if let Some(mut answer) = self.repository.get(&key) {
                answer.set_property(AGGREGATED_COMPLETED_BY, completed_by::TIMEOUT);
                let original = answer.clone();
                if let Some(a) =
                    self.on_completion(&mut state, &key, Some(&original), answer, true, false)
                {
                    list.push((key, a));
                }
            }
        }
        list
    }

    /// Completes all the groups (by interval or force).
    fn complete_all(&self, by: &str) -> Vec<(String, Exchange)> {
        let mut state = self.state.lock().unwrap();
        self.complete_all_locked(&mut state, by)
    }

    fn complete_all_locked(&self, state: &mut State, by: &str) -> Vec<(String, Exchange)> {
        let mut list = Vec::new();
        for key in self.repository.keys() {
            if let Some(mut answer) = self.repository.get(&key) {
                answer.set_property(AGGREGATED_COMPLETED_BY, by);
                let original = answer.clone();
                if let Some(a) =
                    self.on_completion(state, &key, Some(&original), answer, false, false)
                {
                    list.push((key, a));
                }
            }
        }
        list
    }

    /// The completion check (Java `isCompleted`).
    fn is_completed(
        &self,
        state: &mut State,
        key: &str,
        exchange: &mut Exchange,
    ) -> Option<&'static str> {
        // the batch consumer completion first
        if self.options.completion_from_batch_consumer {
            if !state.batch_keys.iter().any(|k| k == key) {
                state.batch_keys.push(key.to_string());
            }
            state.batch_counter += 1;
            let size = exchange
                .property(BATCH_SIZE)
                .and_then(|v| convert_to::<i64>(v).ok().flatten())
                .unwrap_or(0);
            if size > 0 && state.batch_counter >= size {
                state.batch_counter = 0;
                return Some(completed_by::CONSUMER);
            }
        }
        if flag(exchange, false, AGGREGATION_COMPLETE_CURRENT_GROUP) {
            exchange.remove_property(AGGREGATION_COMPLETE_CURRENT_GROUP);
            return Some(completed_by::STRATEGY);
        }
        if let Some(p) = &self.options.completion_predicate {
            if p.matches(exchange).unwrap_or(false) {
                return Some(completed_by::PREDICATE);
            }
        }
        let mut size_checked = false;
        if let Some(value) = int_of(exchange, &self.options.completion_size_expression) {
            if value > 0 {
                size_checked = true;
                if size_of(exchange) >= value {
                    return Some(completed_by::SIZE);
                }
            }
        }
        if !size_checked
            && self.options.completion_size > 0
            && size_of(exchange) >= self.options.completion_size as i64
        {
            return Some(completed_by::SIZE);
        }
        None
    }

    /// Tracks the inactivity timeout of the group (Java `trackTimeout`).
    fn track_timeout(&self, state: &mut State, key: &str, exchange: &Exchange) {
        let mut timeout =
            int_of(exchange, &self.options.completion_timeout_expression).filter(|v| *v > 0);
        if timeout.is_none() && self.options.completion_timeout > 0 {
            timeout = Some(self.options.completion_timeout);
        }
        if let Some(t) = timeout {
            state.timeouts.insert(
                key.to_string(),
                Instant::now() + Duration::from_millis(t as u64),
            );
        }
    }

    /// Aggregates the exchange into its group (Java `doAggregation`), adding the completed
    /// exchanges to the list.
    fn do_aggregation(
        &self,
        state: &mut State,
        key: &str,
        mut new: Exchange,
        list: &mut Vec<(String, Exchange)>,
    ) -> CamelResult<()> {
        let mut complete: Option<&'static str> = None;
        let mut original = self.repository.get(key);
        let mut old = original.clone();
        let mut size = 1;
        if let Some(o) = &old {
            size = o
                .property(AGGREGATED_SIZE)
                .and_then(|v| convert_to::<i64>(v).ok().flatten())
                .unwrap_or(0)
                + 1;
        }
        if let Some(o) = old.as_mut() {
            prepare_out_to_in(o);
        }
        prepare_out_to_in(&mut new);
        let pre_completion = self.strategy.can_pre_complete();
        if pre_completion {
            new.set_property(AGGREGATED_SIZE, size);
            new.set_property(AGGREGATED_CORRELATION_KEY, key);
            if self.strategy.pre_complete(old.as_ref(), Some(&new)) {
                complete = Some(completed_by::STRATEGY);
            } else {
                self.track_timeout(state, key, &new);
            }
            new.remove_property(AGGREGATED_SIZE);
            new.remove_property(AGGREGATED_CORRELATION_KEY);
        } else if self.options.eager_check_completion {
            new.set_property(AGGREGATED_SIZE, size);
            new.set_property(AGGREGATED_CORRELATION_KEY, key);
            complete = self.is_completed(state, key, &mut new);
            if complete.is_none() {
                self.track_timeout(state, key, &new);
            }
            new.remove_property(AGGREGATED_SIZE);
            new.remove_property(AGGREGATED_CORRELATION_KEY);
        }
        if pre_completion {
            if let Some(by) = complete.take() {
                // the group completes without the new exchange, which starts a new group
                if let Some(o) = old.take() {
                    self.do_aggregation_complete(state, by, list, key, original.take(), o, false);
                }
                size = 1;
                self.track_timeout(state, key, &new);
            }
        }
        let mut aggregate_failed = false;
        let answer = match self.strategy.aggregate(old.clone(), Some(new.clone())) {
            Ok(a) => a,
            Err(e) => {
                aggregate_failed = true;
                if self.options.discard_on_aggregation_failure {
                    log::debug!("Aggregation for correlation key {key} discarding aggregated exchange due to failure in AggregationStrategy caused by: {e}");
                    complete = Some(completed_by::STRATEGY);
                    match old.clone() {
                        Some(o) => Some(o),
                        None => return Ok(()),
                    }
                } else {
                    return Err(CamelError::camel_exchange_cause(
                        "Error occurred during aggregation",
                        &new,
                        e,
                    ));
                }
            }
        };
        let Some(mut answer) = answer else {
            return Err(CamelError::camel_exchange(
                "AggregationStrategy returned null which is not allowed",
                &new,
            ));
        };
        if flag(&answer, true, AGGREGATION_COMPLETE_ALL_GROUPS)
            || flag(&answer, false, AGGREGATION_COMPLETE_ALL_GROUPS)
        {
            answer
                .message_mut()
                .remove_header(AGGREGATION_COMPLETE_ALL_GROUPS);
            answer.remove_property(AGGREGATION_COMPLETE_ALL_GROUPS);
            list.extend(self.complete_all_locked(state, completed_by::FORCE));
            original = None;
        } else if self.options.completion_on_new_correlation_group && original.is_none() {
            list.extend(self.complete_all_locked(state, completed_by::FORCE));
        }
        answer.set_property(AGGREGATED_SIZE, size);
        if !pre_completion && !self.options.eager_check_completion {
            complete = self.is_completed(state, key, &mut answer);
            if complete.is_none() {
                self.track_timeout(state, key, &new);
            }
        }
        match complete {
            None if !aggregate_failed => {
                self.repository.add(key, answer);
            }
            by => self.do_aggregation_complete(
                state,
                by.unwrap_or(completed_by::STRATEGY),
                list,
                key,
                original,
                answer,
                aggregate_failed,
            ),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn do_aggregation_complete(
        &self,
        state: &mut State,
        by: &str,
        list: &mut Vec<(String, Exchange)>,
        key: &str,
        original: Option<Exchange>,
        mut answer: Exchange,
        aggregate_failed: bool,
    ) {
        if by == completed_by::CONSUMER {
            // all the groups of the batch (Java doAggregationComplete)
            let keys = std::mem::take(&mut state.batch_keys);
            let mut current = Some((original, answer));
            for batch_key in keys {
                let (batch_original, mut batch_answer) = if batch_key == key {
                    match current.take() {
                        Some(c) => c,
                        None => continue,
                    }
                } else {
                    match self.repository.get(&batch_key) {
                        Some(a) => (Some(a.clone()), a),
                        None => continue,
                    }
                };
                batch_answer.set_property(AGGREGATED_COMPLETED_BY, by);
                if let Some(a) = self.on_completion(
                    state,
                    &batch_key,
                    batch_original.as_ref(),
                    batch_answer,
                    false,
                    aggregate_failed,
                ) {
                    list.push((batch_key, a));
                }
            }
            return;
        }
        answer.set_property(AGGREGATED_COMPLETED_BY, by);
        if let Some(a) = self.on_completion(
            state,
            key,
            original.as_ref(),
            answer,
            false,
            aggregate_failed,
        ) {
            list.push((key.to_string(), a));
        }
    }

    /// The completion of a group (Java `onCompletion`): `None` if discarded.
    fn on_completion(
        &self,
        state: &mut State,
        key: &str,
        original: Option<&Exchange>,
        mut aggregated: Exchange,
        from_timeout: bool,
        aggregate_failed: bool,
    ) -> Option<Exchange> {
        aggregated.set_property(AGGREGATED_CORRELATION_KEY, key);
        if let Some(o) = original {
            self.repository.remove(key, o);
        }
        if !from_timeout {
            state.timeouts.remove(key);
        }
        if let Some(c) = state.closed.as_mut() {
            c.insert(key);
        }
        if from_timeout {
            let timeout = if self.options.completion_timeout > 0 {
                self.options.completion_timeout
            } else {
                -1
            };
            self.strategy.timeout(&mut aggregated, -1, -1, timeout);
        }
        if (from_timeout && self.options.discard_on_completion_timeout)
            || (aggregate_failed && self.options.discard_on_aggregation_failure)
        {
            self.repository.confirm(aggregated.exchange_id());
            return None;
        }
        Some(aggregated)
    }

    /// Sends the completed exchanges to the outputs (Java `onSubmitCompletion`).
    async fn submit_all(&self, list: Vec<(String, Exchange)>) {
        for (_, mut exchange) in list {
            self.strategy.on_completion(&mut exchange, None);
            prepare_out_to_in(&mut exchange);
            if let Err(e) = self.processor.process(&mut exchange).await {
                exchange.set_exception(Some(e));
            }
            if let Some(e) = exchange.exception() {
                log::warn!("Error processing aggregated exchange: {exchange}. Caused by: {e}");
            }
            // Java AggregateOnCompletion
            self.repository.confirm(exchange.exchange_id());
        }
    }
}

impl Processor for AggregateProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            self.inner.processor.start_processor().await?;
            self.inner.start_background();
            Ok(())
        })
    }

    /// Completes all groups if `force_completion_on_stop` or `complete_all_on_stop` (Java
    /// `prepareShutdown`), then stops the background tasks and the outputs.
    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let inner = &self.inner;
            if inner.options.force_completion_on_stop || inner.options.complete_all_on_stop {
                let list = inner.complete_all(completed_by::FORCE);
                inner.submit_all(list).await;
            }
            inner.stop_background();
            {
                let mut state = inner.state.lock().unwrap();
                state.timeouts.clear();
                if let Some(c) = state.closed.as_mut() {
                    c.keys.clear();
                    c.set.clear();
                }
            }
            inner.processor.stop_processor().await
        })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            let inner = &self.inner;
            inner.start_background();
            if flag(exchange, true, AGGREGATION_COMPLETE_ALL_GROUPS)
                || flag(exchange, false, AGGREGATION_COMPLETE_ALL_GROUPS)
            {
                exchange
                    .message_mut()
                    .remove_header(AGGREGATION_COMPLETE_ALL_GROUPS);
                exchange.remove_property(AGGREGATION_COMPLETE_ALL_GROUPS);
                let list = inner.complete_all(completed_by::FORCE);
                inner.submit_all(list).await;
                return Ok(());
            }
            let key = match inner.correlation.evaluate(exchange) {
                Ok(v) if !v.is_null() => v.to_string(),
                Ok(_) => String::new(),
                Err(e) => {
                    exchange.set_exception(Some(e));
                    return Ok(());
                }
            };
            if key.is_empty() {
                if !inner.options.ignore_invalid_correlation_keys {
                    exchange.set_exception(Some(CamelError::camel_exchange(
                        "Invalid correlation key",
                        exchange,
                    )));
                }
                return Ok(());
            }
            let mut copy = create_correlated_copy(exchange);
            copy.remove_property(AGGREGATION_COMPLETE_CURRENT_GROUP);
            copy.message_mut()
                .remove_header(AGGREGATION_COMPLETE_ALL_GROUPS);
            copy.remove_property(AGGREGATION_COMPLETE_ALL_GROUPS);
            copy.message_mut()
                .remove_header(AGGREGATION_COMPLETE_ALL_GROUPS_INCLUSIVE);
            let mut list = Vec::new();
            let result = {
                let mut state = inner.state.lock().unwrap();
                if state.closed.as_ref().is_some_and(|c| c.set.contains(&key)) {
                    Err(CamelError::closed_correlation_key(&key, exchange))
                } else {
                    inner.do_aggregation(&mut state, &key, copy, &mut list)
                }
            };
            if let Err(e) = result {
                exchange.set_exception(Some(e));
            }
            inner.submit_all(list).await;
            if flag(exchange, true, AGGREGATION_COMPLETE_ALL_GROUPS_INCLUSIVE) {
                exchange
                    .message_mut()
                    .remove_header(AGGREGATION_COMPLETE_ALL_GROUPS_INCLUSIVE);
                let list = inner.complete_all(completed_by::FORCE);
                inner.submit_all(list).await;
            }
            Ok(())
        })
    }
}
