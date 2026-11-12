//! Tests ported from the Java camel-core package `org.apache.camel.processor` (one module per
//! Java test class).

#[path = "../support/mod.rs"]
mod support;

mod aggregator;
mod choice_no_otherwise_test;
mod claim_check_test;
mod converter;
mod dead_letter_channel_redelivery_test;
mod dead_letter_channel_use_original_in_body_test;
mod default_error_handler_redelivery_test;
mod delayer_test;
mod enricher;
mod filter_test;
mod idempotent_consumer_test;
mod intercept;
mod load_balancer_test;
mod loop_test;
mod message_history_test;
mod multicast_parallel_test;
mod multicast_test;
mod on_completion_test;
mod onexception;
mod pipeline_test;
mod recipient_list_parallel_test;
mod recipient_list_test;
mod resequencer_test;
mod rollback_throttle_test;
mod routing_slip_test;
mod sampling_throttler_test;
mod sort_step_script_test;
mod split_parallel_test;
mod splitter_misc_test;
mod splitter_test;
mod threads_test;
mod to_dynamic_test;
mod try_processor_test;
mod validate_test;
mod wire_tap_test;
