//! Tests ported from the Java camel-core package `org.apache.camel.processor` (one module per
//! Java test class).

#[path = "../support/mod.rs"]
mod support;

mod aggregator;
mod choice_no_otherwise_test;
mod converter;
mod dead_letter_channel_redelivery_test;
mod default_error_handler_redelivery_test;
mod delayer_test;
mod filter_test;
mod multicast_test;
mod onexception;
mod pipeline_test;
mod recipient_list_test;
mod splitter_test;
mod to_dynamic_test;
mod try_processor_test;
mod wire_tap_test;
