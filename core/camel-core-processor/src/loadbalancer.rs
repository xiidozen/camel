//! The load balancer EIP (Java `org.apache.camel.processor.loadbalancer`): sends each message
//! to one of the processors, chosen round robin, at random, by a sticky correlation key, by
//! weights, or to all of them (topic), or fails over to the next processor on errors.

use crate::process_and_capture;
use camel_api::error::{CamelError, CamelResult, ErrorType};
use camel_api::exchange::Exchange;
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedExpression, SharedProcessor,
};
use camel_support::exchange_helper::copy_results;
use camel_util::secure_random_helper::next_below;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// The failover options (Java `FailoverLoadBalancerDefinition`).
#[derive(Debug, Clone, Default)]
pub struct FailoverOptions {
    /// The errors that fail over (empty: any error).
    pub exceptions: Vec<ErrorType>,
    /// Whether each message starts at the next processor (round robin) instead of the first.
    pub round_robin: bool,
    /// Whether each message starts at the last processor that succeeded.
    pub sticky: bool,
    /// The maximum number of failover attempts, `-1` (the default) for no limit.
    pub maximum_failover_attempts: i32,
}

/// How the processor is chosen.
pub enum LoadBalance {
    /// The processors in turn.
    RoundRobin,
    /// A processor at random.
    Random,
    /// The same processor for the same correlation key.
    Sticky(SharedExpression),
    /// All the processors, one after the other, each with a copy of the exchange.
    Topic,
    /// The first processor, then the next one when it fails.
    Failover(FailoverOptions),
    /// The processors by weights (distribution ratios), in turn or at random.
    Weighted {
        /// Whether the processors are chosen in turn, else at random.
        round_robin: bool,
        /// The distribution ratio of each processor.
        ratios: Vec<u32>,
    },
}

/// The mutable state of the balancer.
#[derive(Default)]
struct State {
    /// The last chosen index (round robin counters start at -1, as Java).
    counter: i64,
    /// The last good index of a sticky failover.
    last_good: i64,
    /// The processor of each sticky key.
    sticky: HashMap<u64, usize>,
    /// The remaining weights of a weighted balancer.
    runtime: Vec<u32>,
}

/// The load balancer processor.
pub struct LoadBalancerProcessor {
    balance: LoadBalance,
    processors: Vec<SharedProcessor>,
    state: Mutex<State>,
}

/// The number of hash groups of a sticky balancer (Java `numberOfHashGroups`).
const HASH_GROUPS: u64 = 64 * 1024;

impl LoadBalancerProcessor {
    /// Creates the processor.
    ///
    /// # Errors
    ///
    /// Invalid distribution ratios (negative sum, not one per processor).
    pub fn new(balance: LoadBalance, processors: Vec<SharedProcessor>) -> CamelResult<Self> {
        let mut runtime = Vec::new();
        if let LoadBalance::Weighted { ratios, .. } = &balance {
            let sum: u64 = ratios.iter().map(|r| *r as u64).sum();
            if sum == 0 {
                return Err(CamelError::illegal_argument(
                    "At least one distribution ratio must be a positive number",
                ));
            }
            if ratios.len() != processors.len() {
                return Err(CamelError::illegal_argument(format!(
                    "Loadbalacing with {} should match number of distributions {}",
                    processors.len(),
                    ratios.len()
                )));
            }
            runtime = ratios.clone();
        }
        Ok(LoadBalancerProcessor {
            balance,
            processors,
            state: Mutex::new(State {
                counter: -1,
                last_good: -1,
                sticky: HashMap::new(),
                runtime,
            }),
        })
    }

    fn next_round_robin(state: &mut State, size: usize) -> usize {
        state.counter += 1;
        if state.counter >= size as i64 {
            state.counter = 0;
        }
        state.counter as usize
    }

    /// The index of the processor of the queue balancers.
    fn choose(&self, exchange: &mut Exchange) -> CamelResult<usize> {
        let size = self.processors.len();
        let mut state = self.state.lock().unwrap();
        Ok(match &self.balance {
            LoadBalance::RoundRobin => Self::next_round_robin(&mut state, size),
            LoadBalance::Random => {
                if size <= 1 {
                    0
                } else {
                    next_below(size as u64) as usize
                }
            }
            LoadBalance::Sticky(expression) => {
                let value = expression.evaluate_mut(exchange)?;
                let mut hasher = DefaultHasher::new();
                value.to_string().hash(&mut hasher);
                let key = hasher.finish() % HASH_GROUPS;
                if let Some(i) = state.sticky.get(&key) {
                    *i
                } else {
                    let i = Self::next_round_robin(&mut state, size);
                    state.sticky.insert(key, i);
                    i
                }
            }
            LoadBalance::Weighted {
                round_robin,
                ratios,
            } => {
                let i = if *round_robin {
                    let mut counter = state.counter;
                    loop {
                        counter += 1;
                        if counter >= ratios.len() as i64 {
                            counter = 0;
                        }
                        if state.runtime[counter as usize] > 0 {
                            break counter as usize;
                        }
                    }
                } else {
                    let sum: u32 = state.runtime.iter().sum();
                    let random = next_below(sum as u64) as u32;
                    let mut weight = 0;
                    let mut index = 0;
                    loop {
                        weight += state.runtime[index];
                        if random < weight {
                            break index;
                        }
                        index += 1;
                    }
                };
                state.counter = i as i64;
                state.runtime[i] -= 1;
                if state.runtime.iter().all(|w| *w == 0) {
                    state.runtime = ratios.clone();
                    state.counter = -1;
                }
                i
            }
            LoadBalance::Topic | LoadBalance::Failover(_) => 0,
        })
    }

    async fn topic(&self, exchange: &mut Exchange) {
        for p in &self.processors {
            let mut copy = exchange.copy();
            process_and_capture(p.as_ref(), &mut copy).await;
            if let Some(e) = copy.exception().cloned() {
                exchange.set_exception(Some(e));
                return;
            }
        }
    }

    fn should_fail_over(options: &FailoverOptions, exchange: &Exchange) -> bool {
        match exchange.exception() {
            None => false,
            Some(_) if options.exceptions.is_empty() => true,
            Some(e) => ErrorType::find(&options.exceptions, e).is_some(),
        }
    }

    async fn failover(&self, options: &FailoverOptions, exchange: &mut Exchange) {
        let size = self.processors.len();
        let mut index = {
            let mut state = self.state.lock().unwrap();
            if options.sticky {
                state.last_good.max(0) as usize
            } else if options.round_robin {
                Self::next_round_robin(&mut state, size)
            } else {
                0
            }
        };
        let mut attempts = 0;
        let mut tried = 0;
        let mut current: Option<Exchange> = None;
        loop {
            if let Some(c) = &current {
                if !Self::should_fail_over(options, c) {
                    self.state.lock().unwrap().last_good = index as i64;
                    copy_results(exchange, c);
                    return;
                }
                attempts += 1;
                if options.maximum_failover_attempts > -1
                    && attempts > options.maximum_failover_attempts
                {
                    copy_results(exchange, c);
                    return;
                }
                index += 1;
                let mut state = self.state.lock().unwrap();
                state.counter += 1;
            }
            if index >= size {
                if options.round_robin {
                    index = 0;
                    self.state.lock().unwrap().counter = 0;
                } else if options.sticky && tried < size {
                    index = 0;
                } else {
                    if let Some(c) = &current {
                        copy_results(exchange, c);
                    }
                    return;
                }
            }
            if options.sticky && !options.round_robin && tried >= size {
                if let Some(c) = &current {
                    copy_results(exchange, c);
                }
                return;
            }
            tried += 1;
            // a copy with the same id, as Java ExchangeHelper.createCopy
            let mut copy = exchange.clone();
            copy.set_exception(None);
            process_and_capture(self.processors[index].as_ref(), &mut copy).await;
            current = Some(copy);
        }
    }
}

impl Processor for LoadBalancerProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { start_processors(&self.processors).await })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(&self.processors).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            if self.processors.is_empty() {
                return Ok(());
            }
            match &self.balance {
                LoadBalance::Topic => self.topic(exchange).await,
                LoadBalance::Failover(options) => self.failover(options, exchange).await,
                _ => {
                    let i = self.choose(exchange)?;
                    process_and_capture(self.processors[i].as_ref(), exchange).await;
                }
            }
            Ok(())
        })
    }
}
