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

/// A directory of a test, removed when dropped (Java `testDirectory()`, `fileUri()`,
/// `testFile()`).
pub struct TestDirectory {
    path: std::path::PathBuf,
}

impl TestDirectory {
    /// A new empty directory named after the test.
    pub fn new(name: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "camel-test-{name}-{}-{nanos}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("the test directory");
        TestDirectory { path }
    }

    /// The directory (with `/` separators).
    pub fn path(&self) -> String {
        self.path.to_string_lossy().replace('\\', "/")
    }

    /// The file uri of the directory with the path (and options) appended: `fileUri("a?noop=true")`.
    pub fn file_uri(&self, path: &str) -> String {
        if path.is_empty() {
            format!("file:{}", self.path())
        } else if path.starts_with('?') {
            format!("file:{}{path}", self.path())
        } else {
            format!("file:{}/{path}", self.path())
        }
    }

    /// The path of the file in the directory.
    pub fn test_file(&self, name: &str) -> std::path::PathBuf {
        self.path.join(name)
    }

    /// Writes the file (creating its directories).
    pub fn write(&self, name: &str, content: &str) {
        let f = self.test_file(name);
        if let Some(p) = f.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(f, content).unwrap();
    }

    /// The content of the file, if it exists.
    pub fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.test_file(name)).ok()
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
