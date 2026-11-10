//! Tests ported from the Java camel-core package `org.apache.camel.processor.onexception`.

mod on_exception_continue_test;
mod on_exception_handled_test;

use camel_core::prelude::*;
use std::sync::Arc;

/// The predicate: the `CamelExceptionCaught` property is an error of the kind.
pub fn caught_is(kind: ErrorKind) -> Arc<dyn Predicate> {
    Arc::new(camel_core::api::processor::predicate_fn(
        move |e: &Exchange| {
            Ok(camel_core::processor::try_catch::exception_caught(e).is_some_and(|c| c.is(kind)))
        },
    ))
}
