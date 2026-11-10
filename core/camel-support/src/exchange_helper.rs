//! Helpers on exchanges (Java `org.apache.camel.support.ExchangeHelper`).

use camel_api::exchange::Exchange;
use camel_api::exchange_constants::CORRELATION_ID;

/// A copy of the exchange with a new id, correlated to the original by the
/// `CamelCorrelationId` property (Java `ExchangeHelper.createCorrelatedCopy`).
pub fn create_correlated_copy(exchange: &Exchange) -> Exchange {
    let id = exchange.exchange_id().to_string();
    let mut copy = exchange.copy();
    copy.set_property(CORRELATION_ID, id);
    copy
}

/// Copies the results of the source exchange to the target (Java `ExchangeHelper.copyResults`):
/// the out message (or the in message, as the out of an out capable target), the exception, the
/// properties, the variables and the routing state.
pub fn copy_results(target: &mut Exchange, source: &Exchange) {
    match source.out_message() {
        Some(out) => {
            let out = out.clone();
            *target.out_message_mut() = out;
        }
        None => {
            let message = source.in_message().clone();
            if target.pattern().is_out_capable() {
                *target.out_message_mut() = message;
            } else {
                target.set_in(message);
                target.set_out(None);
            }
        }
    }
    target.set_exception(source.exception().cloned());
    for (k, v) in source.properties() {
        target.set_property(k, v.clone());
    }
    for (k, v) in source.variables() {
        target.set_variable(k, v.clone());
    }
    target.set_route_stop(source.is_route_stop());
    target.set_rollback_only(source.is_rollback_only());
    target.set_rollback_only_last(source.is_rollback_only_last());
    target.set_redelivery_exhausted(source.is_redelivery_exhausted());
    target.set_redelivery_counter(source.redelivery_counter());
    target.set_redelivery_max_counter(source.redelivery_max_counter());
    target.set_error_handler_handled(source.error_handler_handled());
    target.set_failure_handled(source.is_failure_handled());
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_api::context::StandaloneContext;
    use camel_api::enums::ExchangePattern;

    #[test]
    fn copies() {
        let mut a = Exchange::with_pattern(StandaloneContext::shared(), ExchangePattern::InOut);
        a.message_mut().set_body("in");
        let mut b = create_correlated_copy(&a);
        assert_ne!(a.exchange_id(), b.exchange_id());
        assert_eq!(
            Some(&camel_util::value::Value::from(a.exchange_id())),
            b.property(CORRELATION_ID)
        );
        b.message_mut().set_body("reply");
        b.set_variable("v", 1);
        copy_results(&mut a, &b);
        assert_eq!("reply", a.out_message().unwrap().body().to_string());
        assert_eq!(Some(&camel_util::value::Value::from(1)), a.variable("v"));
        let mut c = Exchange::new(StandaloneContext::shared());
        copy_results(&mut c, &b);
        assert_eq!("reply", c.in_message().body().to_string());
        assert!(c.out_message().is_none());
    }
}
