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
