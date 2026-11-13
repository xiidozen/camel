//! Ports of the control bus tests: `ControlBusStartRouteTest`, `ControlBusStartRouteAsyncTest`,
//! `ControlBusRestartRouteTest` and `ControlBusFailRouteTest`.

use crate::support::ContextTestSupport;
use camel_core::api::enums::ServiceStatus;
use camel_core::api::route::{RouteErrorPhase, SharedRoute};
use camel_core::api::spi::route_policy::RoutePolicy;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

async fn start_route_context() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:foo")
                .route_id("foo")
                .auto_startup(false)
                .to("mock:foo"),
        );
        r.add(
            from("seda:current")
                .route_id("current")
                .to("controlbus:route?routeId=current&action=status&loggingLevel=WARN")
                .to("mock:current"),
        );
    })
    .await
}

fn status(t: &ContextTestSupport, id: &str) -> Option<ServiceStatus> {
    t.context.route_status(id)
}

#[tokio::test]
async fn test_control_bus_start_stop() {
    let t = start_route_context().await;
    assert_eq!(Some(ServiceStatus::Stopped), status(&t, "foo"));
    t.mock("mock:foo").expected_bodies_received(["Hello World"]);
    t.template
        .send_body("seda:foo", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("controlbus:route?routeId=foo&action=start", Value::Null)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    t.template
        .send_body("controlbus:route?routeId=foo&action=stop", Value::Null)
        .await
        .unwrap();
    assert_eq!(Some(ServiceStatus::Stopped), status(&t, "foo"));
}

#[tokio::test]
async fn test_control_bus_suspend_resume() {
    let t = start_route_context().await;
    t.mock("mock:foo").expected_bodies_received(["Hello World"]);
    t.template
        .send_body("seda:foo", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body("controlbus:route?routeId=foo&action=start", Value::Null)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    t.template
        .send_body("controlbus:route?routeId=foo&action=suspend", Value::Null)
        .await
        .unwrap();
    assert_eq!(Some(ServiceStatus::Suspended), status(&t, "foo"));
    t.template
        .send_body("controlbus:route?routeId=foo&action=resume", Value::Null)
        .await
        .unwrap();
    assert_eq!(Some(ServiceStatus::Started), status(&t, "foo"));
}

#[tokio::test]
async fn test_control_bus_status() {
    for uri in [
        "controlbus:route?routeId=foo&action=status",
        "controlbus:route?routeId=foo&action=status&loggingLevel=WARN",
    ] {
        let t = start_route_context().await;
        assert_eq!(
            "Stopped",
            t.template
                .request_body(uri, Value::Null)
                .await
                .unwrap()
                .to_string()
        );
        t.context.start_route("foo").await.unwrap();
        assert_eq!(
            "Started",
            t.template
                .request_body(uri, Value::Null)
                .await
                .unwrap()
                .to_string()
        );
    }
}

#[tokio::test]
async fn test_control_bus_current_route_status() {
    let t = start_route_context().await;
    assert_eq!(Some(ServiceStatus::Started), status(&t, "current"));
    let mock = t.mock("mock:current");
    mock.expected_bodies_received(["Started"]);
    t.template
        .send_body("seda:current", Value::Null)
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_control_bus_async() {
    let t = start_route_context().await;
    t.mock("mock:foo").expected_bodies_received(["Hello World"]);
    t.template
        .send_body("seda:foo", "Hello World")
        .await
        .unwrap();
    t.template
        .send_body(
            "controlbus:route?routeId=foo&action=start&async=true",
            Value::Null,
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let status = t
        .template
        .request_body(
            "controlbus:route?routeId=foo&action=status&async=true",
            Value::Null,
        )
        .await
        .unwrap();
    assert!(status.is_null(), "Cannot get result if async");
}

#[derive(Default)]
struct MyRoutePolicy {
    start: AtomicUsize,
    stop: AtomicUsize,
}

impl RoutePolicy for MyRoutePolicy {
    fn on_start(&self, _: &SharedRoute) {
        self.start.fetch_add(1, Ordering::SeqCst);
    }

    fn on_stop(&self, _: &SharedRoute) {
        self.stop.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_control_bus_restart() {
    let policy = Arc::new(MyRoutePolicy::default());
    let p = policy.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("seda:foo")
                .route_id("foo")
                .route_policy(p.clone())
                .to("mock:foo"),
        );
    })
    .await;
    assert_eq!(
        (1, 0),
        (
            policy.start.load(Ordering::SeqCst),
            policy.stop.load(Ordering::SeqCst)
        )
    );
    assert_eq!(Some(ServiceStatus::Started), status(&t, "foo"));
    t.template
        .send_body(
            "controlbus:route?routeId=foo&action=restart&restartDelay=0",
            Value::Null,
        )
        .await
        .unwrap();
    assert_eq!(Some(ServiceStatus::Started), status(&t, "foo"));
    assert_eq!(
        (2, 1),
        (
            policy.start.load(Ordering::SeqCst),
            policy.stop.load(Ordering::SeqCst)
        )
    );
}

#[tokio::test]
async fn test_control_bus_fail() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.error_handler(dead_letter_channel(
            "controlbus:route?routeId=current&action=fail&async=true",
        ));
        r.add(
            from("direct:foo")
                .route_id("foo")
                .throw_exception(CamelError::illegal_argument("Forced by Donkey Kong")),
        );
    })
    .await;
    assert_eq!(Some(ServiceStatus::Started), status(&t, "foo"));
    t.template
        .send_body("direct:foo", "Hello World")
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while status(&t, "foo") != Some(ServiceStatus::Stopped) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the route should stop"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let re = t
        .context
        .route("foo")
        .unwrap()
        .last_error()
        .expect("a route error");
    assert!(re.unhealthy);
    assert_eq!(RouteErrorPhase::Stop, re.phase);
    assert_eq!(Some("Forced by Donkey Kong"), re.exception.message());
}
