//! Tests of the DefaultCamelContext lifecycle, component and endpoint resolution, and routes.

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{
    Component, Consumer, Endpoint, SharedComponent, SharedConsumer, SharedEndpoint, SharedProducer,
};
use camel_api::enums::ServiceStatus;
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::processor::{processor_fn, BoxFuture, SharedProcessor};
use camel_api::route::Route;
use camel_api::service::Service;
use camel_api::spi::registry::Registry;
use camel_base_engine::context::DefaultCamelContext;
use camel_base_engine::route::RouteSpec;
use camel_util::uri_support::Parameters;
use camel_util::value::Value;
use std::any::Any;
use std::sync::{Arc, Mutex};

type Log = Arc<Mutex<Vec<String>>>;

/// A component whose consumers record their start and stop, and fail to start for `fail:`
/// paths.
struct TestComponent(Log);

impl Service for TestComponent {}

impl Component for TestComponent {
    fn create_endpoint(
        &self,
        _c: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        _p: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        Ok(Arc::new(TestEndpoint {
            uri: uri.to_string(),
            name: remaining.to_string(),
            log: self.0.clone(),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct TestEndpoint {
    uri: String,
    name: String,
    log: Log,
}

impl Service for TestEndpoint {}

impl Endpoint for TestEndpoint {
    fn endpoint_uri(&self) -> &str {
        &self.uri
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        StandaloneContext::shared()
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Err(CamelError::new(
            ErrorKind::UnsupportedOperation,
            "no producer",
        ))
    }

    fn create_consumer(self: Arc<Self>, processor: SharedProcessor) -> CamelResult<SharedConsumer> {
        Ok(Arc::new(TestConsumer {
            endpoint: self,
            processor,
        }))
    }

    fn is_lenient_properties(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct TestConsumer {
    endpoint: Arc<TestEndpoint>,
    processor: SharedProcessor,
}

impl Service for TestConsumer {
    fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
        let name = self.endpoint.name.clone();
        self.endpoint
            .log
            .lock()
            .unwrap()
            .push(format!("start {name}"));
        let r = if name.starts_with("fail") {
            Err(CamelError::runtime("Forced"))
        } else {
            Ok(())
        };
        Box::pin(std::future::ready(r))
    }

    fn stop(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.endpoint
            .log
            .lock()
            .unwrap()
            .push(format!("stop {}", self.endpoint.name));
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Consumer for TestConsumer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }

    fn processor(&self) -> SharedProcessor {
        self.processor.clone()
    }
}

fn noop() -> SharedProcessor {
    Arc::new(processor_fn(|_| Ok(())))
}

fn setup() -> (Arc<DefaultCamelContext>, Log) {
    let log: Log = Arc::default();
    let context = DefaultCamelContext::new();
    let l = log.clone();
    context.register_component_factory(
        "test",
        Arc::new(move || Arc::new(TestComponent(l.clone())) as SharedComponent),
    );
    (context, log)
}

#[tokio::test]
async fn components_and_endpoints() {
    let (context, log) = setup();
    assert!(context.name().starts_with("camel-"));
    assert!(context.component("test").is_some());
    assert!(context.component_as::<TestComponent>("test").is_some());
    assert!(context.component("nope").is_none());
    assert_eq!(vec!["test"], context.component_names());
    let e = context
        .add_component("test", Arc::new(TestComponent(log.clone())))
        .unwrap_err();
    assert_eq!(
        Some("Cannot add component as its already previously added: test"),
        e.message()
    );

    // components bound in the registry are found too
    let registered: SharedComponent = Arc::new(TestComponent(log));
    context
        .simple_registry()
        .bind("reg", Value::object(registered))
        .unwrap();
    assert!(context.component("reg").is_some());
    assert!(context.registry().is_some());

    let a = context.endpoint("test:foo?b=2&a=1").unwrap();
    assert_eq!("test://foo?a=1&b=2", a.endpoint_uri());
    assert!(Arc::ptr_eq(
        &a,
        &context.endpoint("test://foo?a=1&b=2").unwrap()
    ));
    assert!(context.has_endpoint("test:foo?a=1&b=2").is_some());
    assert!(context
        .endpoint_as::<TestEndpoint>("test:foo?a=1&b=2")
        .is_ok());
    assert_eq!(1, context.endpoints().len());

    let e = context.endpoint("nope:foo").err().unwrap();
    assert!(e.is(ErrorKind::NoSuchEndpoint));
    assert!(context
        .endpoint("")
        .err()
        .unwrap()
        .is(ErrorKind::NoSuchEndpoint));
    assert!(context.remove_component("reg").is_some());
}

#[tokio::test]
async fn routes_start_in_order_and_stop_in_reverse() {
    let (context, log) = setup();
    let a = context
        .add_route(RouteSpec::new("test:a", noop()).with_startup_order(20))
        .await
        .unwrap();
    let b = context
        .add_route(RouteSpec::new("test:b", noop()).with_startup_order(10))
        .await
        .unwrap();
    context
        .add_route(
            RouteSpec::new("test:manual", noop())
                .with_id("manual")
                .with_auto_startup(false),
        )
        .await
        .unwrap();
    assert_eq!(("route1", "route2"), (a.as_str(), b.as_str()));
    context.start().await.unwrap();
    assert_eq!(vec!["start b", "start a"], *log.lock().unwrap());
    assert_eq!(Some(ServiceStatus::Started), context.route_status("route1"));
    assert_eq!(
        Some(ServiceStatus::Initializing),
        context.route_status("manual")
    );
    assert!(context.route("route1").unwrap().consumer().is_some());
    assert!(!context.route("route1").unwrap().is_custom_id());
    assert!(context.route("manual").unwrap().is_custom_id());

    // a route added to a started context starts right away
    context
        .add_route(RouteSpec::new("test:late", noop()).with_id("late"))
        .await
        .unwrap();
    assert_eq!(Some(ServiceStatus::Started), context.route_status("late"));
    context.start_route("manual").await.unwrap();

    log.lock().unwrap().clear();
    context.stop_route("late").await.unwrap();
    assert!(context.remove_route("late"));
    assert!(
        !context.remove_route("route1"),
        "a started route is not removed"
    );
    context.stop().await.unwrap();
    assert_eq!(
        vec!["stop late", "stop manual", "stop a", "stop b"],
        *log.lock().unwrap()
    );
    assert!(context.status().is_stopped());
    assert!(context.route("route1").unwrap().consumer().is_none());
    assert_eq!(0, context.route("route1").unwrap().uptime_millis());
}

#[tokio::test]
async fn route_failures() {
    let (context, log) = setup();
    context
        .add_route(RouteSpec::new("test:ok", noop()).with_id("ok"))
        .await
        .unwrap();
    let dup = context
        .add_route(RouteSpec::new("test:ok2", noop()).with_id("ok"))
        .await
        .unwrap_err();
    assert!(dup.is(ErrorKind::FailedToStartRoute));
    assert_eq!(
        Some("Failed to start route: ok because: duplicate id detected: ok. Please correct ids to be unique among all your routes."),
        dup.message()
    );
    let bad = context
        .add_route(RouteSpec::new("nope:x", noop()).with_id("bad"))
        .await
        .unwrap_err();
    assert!(bad.is(ErrorKind::FailedToCreateRoute), "{bad:?}");

    context
        .add_route(RouteSpec::new("test:fail", noop()).with_id("failing"))
        .await
        .unwrap();
    let e = context.start().await.unwrap_err();
    assert_eq!(
        Some("Failed to start route: failing because: Forced"),
        e.message()
    );
    // the routes started before the failure are stopped again
    assert_eq!(
        vec!["start ok", "start fail", "stop ok"],
        *log.lock().unwrap()
    );
    assert!(context.status().is_stopped());
    assert!(context
        .start_route("missing")
        .await
        .unwrap_err()
        .message()
        .unwrap()
        .contains("Cannot find route with id: missing"));
}
