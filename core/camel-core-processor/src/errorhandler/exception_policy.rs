//! Exception policies (`onException`, Java `ExceptionPolicy`, `ExceptionPolicyKey` and
//! `DefaultExceptionPolicyStrategy`).

use super::redelivery_policy::RedeliveryPolicyOverrides;
use camel_api::error::{CamelError, ErrorKind, ErrorType};
use camel_api::exchange::Exchange;
use camel_api::processor::{SharedPredicate, SharedProcessor};
use std::error::Error;
use std::sync::Arc;

/// What to do for some errors (an `onException` clause).
#[derive(Default, Clone)]
pub struct ExceptionPolicy {
    /// The id (Java the onException node id).
    pub id: String,
    /// The types of the errors.
    pub exceptions: Vec<ErrorType>,
    /// The route the policy applies to (`None` for all routes).
    pub route_id: Option<String>,
    /// An extra predicate the exchange must match.
    pub on_when: Option<SharedPredicate>,
    /// Redeliver while the predicate matches (instead of the maximum redeliveries).
    pub retry_while: Option<SharedPredicate>,
    /// Whether the error is handled (the exchange continues as successful back to the caller).
    pub handled: Option<SharedPredicate>,
    /// Whether routing continues after the failed step.
    pub continued: Option<SharedPredicate>,
    /// The redelivery policy overrides.
    pub redelivery: RedeliveryPolicyOverrides,
    /// The steps of the onException block.
    pub processor: Option<SharedProcessor>,
    /// A processor called before each redelivery.
    pub on_redelivery: Option<SharedProcessor>,
    /// A processor called when the error occurs.
    pub on_exception_occurred: Option<SharedProcessor>,
    /// Whether the failure processor gets the original in message.
    pub use_original_message: bool,
    /// Whether the failure processor gets the original in message body.
    pub use_original_body: bool,
}

impl std::fmt::Debug for ExceptionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExceptionPolicy[{}: {:?}]", self.id, self.exceptions)
    }
}

/// The inheritance level of an error type (Java `getInheritanceLevel` of the class): `Any` is
/// Java `Exception` (level 2), a kind is below its parent kinds.
fn type_level(t: &ErrorType) -> i32 {
    match t {
        ErrorType::Any => 2,
        ErrorType::Kind(k) => 3 + kind_depth(*k),
        ErrorType::Class(_) => 3,
    }
}

fn kind_depth(k: ErrorKind) -> i32 {
    let mut depth = 0;
    let mut current = k.parent();
    while let Some(p) = current {
        depth += 1;
        current = p.parent();
    }
    depth
}

fn error_level(e: &(dyn Error + 'static)) -> i32 {
    match e.downcast_ref::<CamelError>() {
        Some(c) => 3 + kind_depth(c.kind()) + i32::from(c.subtype().is_some()),
        None => 3,
    }
}

fn exact(t: &ErrorType, e: &(dyn Error + 'static)) -> bool {
    match (t, e.downcast_ref::<CamelError>()) {
        (ErrorType::Kind(k), Some(c)) => c.kind() == *k && c.subtype().is_none(),
        (ErrorType::Class(_), Some(c)) => c.subtype().is_some() && t.matches(e),
        (ErrorType::Class(_), None) => t.matches(e),
        _ => false,
    }
}

/// Finds the best policy for the error (Java `DefaultExceptionPolicyStrategy`): route scoped
/// policies first, the error and then its causes (from the root cause), an exact type match
/// wins, else the closest super type.
pub fn find_exception_policy(
    policies: &[Arc<ExceptionPolicy>],
    exchange: &Exchange,
    error: &CamelError,
) -> Option<Arc<ExceptionPolicy>> {
    let route_id = exchange.route_id();
    let (route_scoped, context_scoped): (Vec<_>, Vec<_>) =
        policies.iter().partition(|p| p.route_id.is_some());
    let chain = camel_util::object_helper::create_exception_iterable(error);
    let mut candidates: std::collections::BTreeMap<i32, Arc<ExceptionPolicy>> =
        std::collections::BTreeMap::new();
    for scope in [route_scoped, context_scoped] {
        for e in &chain {
            let target = error_level(*e);
            let mut candidate: Option<&Arc<ExceptionPolicy>> = None;
            let mut candidate_diff = i32::MAX;
            for p in &scope {
                if let (Some(r), Some(current)) = (&p.route_id, route_id) {
                    if r != current {
                        continue;
                    }
                }
                for t in &p.exceptions {
                    if !t.matches(*e) {
                        continue;
                    }
                    let when = match &p.on_when {
                        Some(w) => w.matches(exchange).unwrap_or(false),
                        None => true,
                    };
                    if !when {
                        continue;
                    }
                    if exact(t, *e) {
                        candidate = Some(p);
                        candidate_diff = 0;
                        break;
                    }
                    let diff = target - type_level(t);
                    if diff < candidate_diff {
                        candidate = Some(p);
                        candidate_diff = diff;
                    }
                }
                if candidate_diff == 0 {
                    break;
                }
            }
            if let Some(c) = candidate {
                candidates
                    .entry(candidate_diff)
                    .or_insert_with(|| c.clone());
            }
            if candidate_diff == 0 {
                return candidates.values().next().cloned();
            }
        }
    }
    candidates.values().next().cloned()
}
