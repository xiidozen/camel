//! The base of the camel-core tests (Java `ContextTestSupport`): a context with the core
//! components, the routes of the test, a producer template and helpers for mock endpoints.

#![allow(dead_code)]

use camel_core::prelude::*;
use std::sync::Arc;

/// A started CamelContext for a test.
pub struct ContextTestSupport {
    /// The context.
    pub context: Arc<DefaultCamelContext>,
    /// A producer template.
    pub template: ProducerTemplate,
}

impl ContextTestSupport {
    /// A context with the routes, started (Java `createRouteBuilder` + `setUp`).
    pub async fn with_routes(routes: impl RouteBuilder) -> Self {
        let t = Self::new();
        t.context
            .add_routes(routes)
            .await
            .expect("the routes of the test");
        t.context.start().await.expect("the context starts");
        t
    }

    /// A context without routes, not started (Java `isUseRouteBuilder() == false`).
    pub fn new() -> Self {
        let context = camel_core::default_context();
        let template = context.create_producer_template();
        ContextTestSupport { context, template }
    }

    /// The mock endpoint for the uri (Java `getMockEndpoint`).
    pub fn mock(&self, uri: &str) -> MockEndpoint {
        MockEndpoint::resolve(self.context.as_ref(), uri).expect("a mock endpoint")
    }

    /// Asserts all mock endpoints (Java `assertMockEndpointsSatisfied`).
    pub async fn assert_mock_endpoints_satisfied(&self) {
        MockEndpoint::assert_is_satisfied_all(self.context.endpoints()).await;
    }

    /// Resets all mock endpoints (Java `resetMocks`).
    pub fn reset_mocks(&self) {
        MockEndpoint::reset_mocks(self.context.endpoints());
    }
}

/// Sets the expected message count on the mock endpoints (Java `expectsMessageCount`).
pub fn expects_message_count(count: i32, mocks: &[&MockEndpoint]) {
    for m in mocks {
        m.expected_message_count(count);
    }
}

/// An exchange of a context with the core components and languages (Java
/// `ExchangeTestSupport`).
pub fn exchange() -> Exchange {
    let context = camel_core::default_context();
    Exchange::new(context.as_shared())
}

/// The cause of the error as a Camel error (Java `e.getCause()`), panicking if there is none.
#[allow(dead_code)]
pub fn cause_of(e: &CamelError) -> &CamelError {
    e.cause()
        .and_then(|c| c.downcast_ref::<CamelError>())
        .unwrap_or_else(|| panic!("no Camel error cause: {e:?}"))
}

/// Java `BodyInAggregatingStrategy`: joins the bodies with `+`.
#[allow(dead_code)]
pub fn body_in_aggregating_strategy() -> camel_core::api::processor::SharedAggregationStrategy {
    Arc::new(camel_core::api::processor::aggregation_fn(
        |old: Option<Exchange>, new: Option<Exchange>| {
            let Some(new) = new else { return Ok(old) };
            let Some(mut old) = old else {
                return Ok(Some(new));
            };
            let body = format!("{}+{}", old.message().body(), new.message().body());
            old.message_mut().set_body(body);
            Ok(Some(old))
        },
    ))
}

/// The aggregation strategy many Java tests define inline: appends the body of the new
/// exchange to the body of the old one (no separator).
#[allow(dead_code)]
pub fn concat_strategy() -> camel_core::api::processor::SharedAggregationStrategy {
    Arc::new(camel_core::api::processor::aggregation_fn(
        |old: Option<Exchange>, new: Option<Exchange>| {
            let Some(new) = new else { return Ok(old) };
            let Some(mut old) = old else {
                return Ok(Some(new));
            };
            let body = format!("{}{}", old.message().body(), new.message().body());
            old.message_mut().set_body(body);
            Ok(Some(old))
        },
    ))
}

/// Waits until the condition holds, polling every 10 millis, and panics after the timeout
/// (Java Awaitility `await().atMost(timeout).until(condition)`).
#[allow(dead_code)]
pub async fn await_until(timeout: std::time::Duration, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + timeout;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the condition was not met within {timeout:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// The predicate: the `CamelExceptionCaught` property is an error of the kind (Java
/// `exchangeProperty(Exchange.EXCEPTION_CAUGHT).isInstanceOf(..)`).
#[allow(dead_code)]
pub fn caught_is(kind: ErrorKind) -> Arc<dyn Predicate> {
    Arc::new(camel_core::api::processor::predicate_fn(
        move |e: &Exchange| {
            Ok(camel_core::processor::try_catch::exception_caught(e).is_some_and(|c| c.is(kind)))
        },
    ))
}
