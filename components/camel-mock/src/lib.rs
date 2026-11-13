//! # camel-mock
//!
//! The mock component for testing routes: a `mock:name` endpoint records the exchanges it
//! receives, and checks expectations set up front (message count, bodies, headers, properties,
//! ordering, custom predicates). [`MockEndpoint::assert_is_satisfied`] waits for the expected
//! messages (10 seconds by default) and then checks the expectations.
//!
//! This crate is the Rust port of the Java `components/camel-mock` module.
//!
//! # Endpoint options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `expectedCount` | `-1` | The number of messages expected (-1 for no expectation). |
//! | `assertPeriod` | `0` | A grace period (millis) after which the endpoint re-asserts, to ensure the assertion is still valid (for example no more messages arrived). |
//! | `resultWaitTime` | `0` | The maximum time (millis) to wait for the expected messages (0 means 10 seconds). |
//! | `resultMinimumWaitTime` | `0` | The minimum time (millis) the expected messages must take to arrive. |
//! | `sleepForEmptyTest` | `0` | How long (millis) to wait before checking that no message arrived, when expecting 0 messages. |
//! | `retainFirst` | `-1` | Only keep the first n received exchanges (-1 keeps all). |
//! | `retainLast` | `-1` | Only keep the last n received exchanges (-1 keeps all). |
//! | `copyOnExchange` | `true` | Whether to keep a copy of the received exchange (instead of the exchange routed further). |
//! | `log` | `false` | Whether to log every received message. |
//!
//! Unknown options are accepted (the endpoint is lenient, as in Java).
//!
//! # Examples
//!
//! ```
//! use camel_api::context::CamelContext;
//! use camel_base_engine::context::DefaultCamelContext;
//! use camel_mock::{MockComponent, MockEndpoint};
//! use std::sync::Arc;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = DefaultCamelContext::new();
//! context.add_component("mock", Arc::new(MockComponent::new())).unwrap();
//! context.start().await.unwrap();
//!
//! let mock = MockEndpoint::resolve(context.as_ref(), "mock:result").unwrap();
//! mock.expected_bodies_received(["Hello World"]);
//! context.create_producer_template().send_body("mock:result", "Hello World").await.unwrap();
//! mock.assert_is_satisfied().await;
//! # });
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod endpoint;

pub use endpoint::{MockAssertionError, MockEndpoint, MockExtension};

use camel_api::context::CamelContext;
use camel_api::endpoint::{Component, SharedEndpoint};
use camel_api::error::CamelResult;
use camel_api::service::Service;
use camel_support::component_support::get_and_remove_parameter;
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The mock component (Java `MockComponent`).
#[derive(Default)]
pub struct MockComponent {
    log: AtomicBool,
}

impl MockComponent {
    /// Creates the component.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the endpoints log every received message (default `false`).
    pub fn set_log(&self, log: bool) {
        self.log.store(log, Ordering::Relaxed);
    }
}

impl Service for MockComponent {}

impl Component for MockComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let mock = MockEndpoint::new(uri, remaining, Arc::downgrade(context));
        mock.set_log(
            get_and_remove_parameter(parameters, "log")?
                .unwrap_or(self.log.load(Ordering::Relaxed)),
        );
        if let Some(v) = get_and_remove_parameter::<i64>(parameters, "expectedCount")? {
            mock.expected_message_count(v as i32);
        }
        if let Some(v) = get_and_remove_parameter::<u64>(parameters, "assertPeriod")? {
            mock.set_assert_period(v);
        }
        if let Some(v) = get_and_remove_parameter::<u64>(parameters, "resultWaitTime")? {
            mock.set_result_wait_time(v);
        }
        if let Some(v) = get_and_remove_parameter::<u64>(parameters, "resultMinimumWaitTime")? {
            mock.set_result_minimum_wait_time(v);
        }
        if let Some(v) = get_and_remove_parameter::<u64>(parameters, "sleepForEmptyTest")? {
            mock.set_sleep_for_empty_test(v);
        }
        if let Some(v) = get_and_remove_parameter::<i64>(parameters, "retainFirst")? {
            mock.set_retain_first(v as i32);
        }
        if let Some(v) = get_and_remove_parameter::<i64>(parameters, "retainLast")? {
            mock.set_retain_last(v as i32);
        }
        if let Some(v) = get_and_remove_parameter::<bool>(parameters, "copyOnExchange")? {
            mock.set_copy_on_exchange(v);
        }
        Ok(Arc::new(mock))
    }

    fn default_name(&self) -> Option<&str> {
        Some("mock")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
