//! Tests ported from the Java camel-core package `org.apache.camel.processor` (one module per
//! Java test class).

#[path = "../support/mod.rs"]
mod support;

mod choice_no_otherwise_test;
mod dead_letter_channel_redelivery_test;
mod default_error_handler_redelivery_test;
mod filter_test;
mod onexception;
mod pipeline_test;
mod try_processor_test;
