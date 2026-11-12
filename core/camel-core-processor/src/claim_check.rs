//! The claim check EIP (Java `ClaimCheckProcessor`, `ClaimCheckAggregationStrategy` and
//! `DefaultClaimCheckRepository`): stores a copy of the message on the exchange (by key, or on
//! a stack) and merges it back later.

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::CLAIM_CHECK_REPOSITORY;
use camel_api::processor::{BoxFuture, Processor, SharedAggregationStrategy, SharedExpression};
use camel_support::exchange_helper::{copy_results, create_correlated_copy};
use camel_support::pattern_helper::match_pattern;
use camel_util::value::{ObjectRef, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The operation of a claim check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimCheckOperation {
    /// Stores the message with the key, replacing an existing one.
    Set,
    /// Merges the message of the key back, keeping it.
    Get,
    /// Merges the message of the key back and removes it.
    GetAndRemove,
    /// Pushes the message on the stack.
    Push,
    /// Merges the message on the top of the stack back and removes it.
    Pop,
}

impl ClaimCheckOperation {
    /// The operation of its Java name (`Set`, `Get`, `GetAndRemove`, `Push`, `Pop`).
    ///
    /// # Errors
    ///
    /// An unknown name.
    pub fn of(name: &str) -> CamelResult<Self> {
        Ok(match name {
            "Set" => ClaimCheckOperation::Set,
            "Get" => ClaimCheckOperation::Get,
            "GetAndRemove" => ClaimCheckOperation::GetAndRemove,
            "Push" => ClaimCheckOperation::Push,
            "Pop" => ClaimCheckOperation::Pop,
            other => {
                return Err(CamelError::illegal_argument(format!(
                    "Unsupported operation: {other}"
                )))
            }
        })
    }
}

/// The messages stored by the claim checks of an exchange (Java
/// `DefaultClaimCheckRepository`), an exchange property shared by the copies of the exchange.
#[derive(Default)]
struct Repository {
    map: HashMap<String, Exchange>,
    stack: Vec<Exchange>,
}

type SharedRepository = Arc<Mutex<Repository>>;

fn repository(exchange: &mut Exchange) -> SharedRepository {
    if let Some(Value::Object(o)) = exchange.property(CLAIM_CHECK_REPOSITORY) {
        if let Some(r) = o.downcast_ref::<SharedRepository>() {
            return r.clone();
        }
    }
    let r: SharedRepository = Arc::default();
    exchange.set_property(
        CLAIM_CHECK_REPOSITORY,
        Value::Object(ObjectRef::new(r.clone())),
    );
    r
}

/// A copy of the exchange without the repository (Java `createClaimCheckCopy`).
fn claim_check_copy(exchange: &mut Exchange) -> Exchange {
    let repo = exchange.remove_property(CLAIM_CHECK_REPOSITORY);
    let copy = create_correlated_copy(exchange);
    if let Some(r) = repo {
        exchange.set_property(CLAIM_CHECK_REPOSITORY, r);
    }
    copy
}

/// The claim check processor.
pub struct ClaimCheckProcessor {
    operation: ClaimCheckOperation,
    key: Option<SharedExpression>,
    filter: Option<String>,
    strategy: Option<SharedAggregationStrategy>,
}

impl ClaimCheckProcessor {
    /// Creates the processor; the key is needed by `Set`, `Get` and `GetAndRemove`.
    ///
    /// # Errors
    ///
    /// A missing key.
    pub fn new(
        operation: ClaimCheckOperation,
        key: Option<SharedExpression>,
        filter: Option<String>,
        strategy: Option<SharedAggregationStrategy>,
    ) -> CamelResult<Self> {
        if key.is_none()
            && matches!(
                operation,
                ClaimCheckOperation::Set
                    | ClaimCheckOperation::Get
                    | ClaimCheckOperation::GetAndRemove
            )
        {
            return Err(CamelError::illegal_argument(format!(
                "Key must be specified for {operation:?} operation"
            )));
        }
        Ok(ClaimCheckProcessor {
            operation,
            key,
            filter,
            strategy,
        })
    }

    /// Merges the stored message into the exchange.
    fn merge(&self, exchange: &mut Exchange, stored: Exchange) -> CamelResult<()> {
        match &self.strategy {
            Some(s) => {
                if let Some(result) = s.aggregate(Some(exchange.clone()), Some(stored))? {
                    copy_results(exchange, &result);
                }
            }
            None => aggregate_filtered(exchange, &stored, self.filter.as_deref()),
        }
        Ok(())
    }

    fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let repo = repository(exchange);
        let key = match &self.key {
            Some(k) => Some(k.evaluate_mut(exchange)?.to_string()),
            None => None,
        };
        let key = key.unwrap_or_default();
        match self.operation {
            ClaimCheckOperation::Set => {
                let copy = claim_check_copy(exchange);
                repo.lock().unwrap().map.insert(key, copy);
            }
            ClaimCheckOperation::Push => {
                let copy = claim_check_copy(exchange);
                repo.lock().unwrap().stack.push(copy);
            }
            ClaimCheckOperation::Get => {
                let stored = repo.lock().unwrap().map.get(&key).cloned();
                if let Some(s) = stored {
                    self.merge(exchange, s)?;
                }
            }
            ClaimCheckOperation::GetAndRemove => {
                let stored = repo.lock().unwrap().map.remove(&key);
                if let Some(s) = stored {
                    self.merge(exchange, s)?;
                }
            }
            ClaimCheckOperation::Pop => {
                let stored = repo.lock().unwrap().stack.pop();
                if let Some(s) = stored {
                    self.merge(exchange, s)?;
                }
            }
        }
        Ok(())
    }
}

impl Processor for ClaimCheckProcessor {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(std::future::ready(self.run(exchange)))
    }
}

fn parts(filter: &str) -> Vec<&str> {
    filter
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

fn after<'a>(part: &'a str, prefix: &str) -> &'a str {
    part.split_once(prefix).map_or(part, |(_, a)| a)
}

/// Merges the stored message into the exchange by the filter rules (Java
/// `ClaimCheckAggregationStrategy`): `body`, `headers`, `header:pattern`, prefixed with `+` to
/// include, `-` to exclude, `--` to remove; empty or `*` merges everything.
pub fn aggregate_filtered(exchange: &mut Exchange, stored: &Exchange, filter: Option<&str>) {
    let new = stored.message();
    let filter = filter.unwrap_or("").trim();
    if filter.is_empty() || filter == "*" {
        exchange.message_mut().set_body(new.body().clone());
        for (k, v) in new.headers().iter() {
            exchange.message_mut().set_header(k, v.clone());
        }
        return;
    }
    let all = parts(filter);
    let rules: Vec<&str> = all
        .iter()
        .copied()
        .filter(|p| !p.starts_with("--"))
        .collect();
    let is_header_rule = |p: &str| {
        p.starts_with("header:") || p.starts_with("+header:") || p.starts_with("-header:")
    };
    // the body is enabled unless excluded, or when there are only exclude rules
    let body_enabled = {
        let mut only_exclude = true;
        let mut answer = None;
        for p in &rules {
            if *p == "body" || *p == "+body" {
                answer = Some(true);
                break;
            } else if *p == "-body" {
                answer = Some(false);
                break;
            }
            only_exclude &= p.starts_with('-');
        }
        answer.unwrap_or(only_exclude)
    };
    let headers_enabled = {
        let mut only_exclude = true;
        let mut answer = None;
        for p in &rules {
            if is_header_rule(p) {
                answer = Some(false);
                break;
            }
            if *p == "headers" || *p == "+headers" {
                answer = Some(true);
                break;
            } else if *p == "-headers" {
                answer = Some(false);
                break;
            }
            only_exclude &= p.starts_with('-');
        }
        answer.unwrap_or(only_exclude)
    };
    if body_enabled {
        exchange.message_mut().set_body(new.body().clone());
    }
    if headers_enabled {
        for (k, v) in new.headers().iter() {
            exchange.message_mut().set_header(k, v.clone());
        }
    }
    if rules.iter().any(|p| is_header_rule(p)) {
        let exclude_only = !rules
            .iter()
            .any(|p| p.starts_with("header:") || p.starts_with("+header:"));
        let has_pattern = |key: &str| {
            rules.iter().any(|p| {
                let pattern = if p.starts_with("header:") || p.starts_with("+header:") {
                    Some(after(p, "header:"))
                } else if p.starts_with("-header:") {
                    Some(after(p, "-header:"))
                } else {
                    None
                };
                pattern.is_some_and(|pat| match_pattern(key, pat))
            })
        };
        let included = |key: &str| {
            rules.iter().any(|p| {
                let pat = if p.starts_with("header:") || p.starts_with("+header:") {
                    after(p, "header:")
                } else {
                    p
                };
                match_pattern(key, pat)
            })
        };
        for (k, v) in new.headers().iter() {
            if has_pattern(k) {
                if included(k) {
                    exchange.message_mut().set_header(k, v.clone());
                }
            } else if exclude_only {
                exchange.message_mut().set_header(k, v.clone());
            }
        }
    }
    for p in &rules {
        if *p == "body" || *p == "+body" {
            exchange.message_mut().set_body(new.body().clone());
        } else if *p == "headers" || *p == "+headers" {
            for (k, v) in new.headers().iter() {
                exchange.message_mut().set_header(k, v.clone());
            }
        }
    }
    // the remove rules (--) at the end
    for p in &all {
        if *p == "--body" {
            exchange.message_mut().set_body(Value::Null);
        } else if *p == "--headers" {
            exchange.message_mut().headers_mut().clear();
        } else if let Some(patterns) = p.strip_prefix("--header:") {
            let keys: Vec<String> = exchange
                .message()
                .headers()
                .iter()
                .map(|(k, _)| k.to_string())
                .filter(|k| patterns.split(',').any(|pat| match_pattern(k, pat)))
                .collect();
            for k in keys {
                exchange.message_mut().remove_header(&k);
            }
        }
    }
}
