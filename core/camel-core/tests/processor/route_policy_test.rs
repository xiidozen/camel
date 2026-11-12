//! Ports of `CustomRoutePolicyTest`, `RoutePoliciesTest`, `RoutePoliciesRefTest`,
//! `impl.RoutePolicyCallbackTest`, `impl.RoutePolicyRefCallbackTest` and
//! `impl.RoutePolicyFactoryTest`.

use crate::support::ContextTestSupport;
use camel_core::api::route::SharedRoute;
use camel_core::api::spi::route_policy::{
    route_policy_value, RoutePolicy, RoutePolicyFactory, SharedRoutePolicy,
};
use camel_core::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Stops the consumer of the route when the body is `stop`.
#[derive(Default)]
struct StopPolicy {
    stopped: AtomicBool,
}

impl RoutePolicy for StopPolicy {
    fn on_exchange_done(&self, route: &SharedRoute, exchange: &Exchange) {
        if matches!(exchange.message().body(), Value::Text(t) if t == "stop") {
            self.stopped.store(true, Ordering::SeqCst);
            if let Some(consumer) = route.consumer() {
                camel_core::support::route_policy_support::stop_consumer(consumer);
            }
        }
    }
}

#[tokio::test]
async fn test_custom_policy() {
    let policy = Arc::new(StopPolicy::default());
    let p: SharedRoutePolicy = policy.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:foo")
                .route_id("foo")
                .route_policy(p.clone())
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:foo", "Hello World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["stop"]);
    t.template.send_body("direct:foo", "stop").await.unwrap();
    mock.assert_is_satisfied().await;
    assert!(policy.stopped.load(Ordering::SeqCst), "Should be stopped");
}

/// Sets the header of its name on the exchanges.
struct HeaderPolicy(String);

impl RoutePolicy for HeaderPolicy {
    fn on_exchange_begin(&self, _: &SharedRoute, exchange: &mut Exchange) {
        exchange.message_mut().set_header(&self.0, self.0.clone());
    }
}

#[tokio::test]
async fn test_custom_policies_and_refs() {
    for by_ref in [false, true] {
        let (a, b): (SharedRoutePolicy, SharedRoutePolicy) = (
            Arc::new(HeaderPolicy("A".into())),
            Arc::new(HeaderPolicy("B".into())),
        );
        let t = ContextTestSupport::new();
        if by_ref {
            let registry = t.context.registry().unwrap();
            registry
                .bind("policy-a", route_policy_value(a.clone()))
                .unwrap();
            registry
                .bind("policy-b", route_policy_value(b.clone()))
                .unwrap();
        }
        t.context
            .add_routes(move |r: &mut Routes| {
                let route = from("direct:start");
                let route = if by_ref {
                    route.route_policy_ref("policy-a, policy-b")
                } else {
                    route.route_policy(a.clone()).route_policy(b.clone())
                };
                r.add(route.to("mock:result"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Hello World"]);
        mock.expected_header_received("A", "A");
        mock.expected_header_received("B", "B");
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }
}

/// Records the callbacks.
#[derive(Default)]
struct CallbackPolicy {
    events: Mutex<Vec<&'static str>>,
}

impl CallbackPolicy {
    fn has(&self, event: &str) -> bool {
        self.events.lock().unwrap().contains(&event)
    }

    fn clear(&self, event: &str) {
        self.events.lock().unwrap().retain(|e| *e != event);
    }

    fn add(&self, event: &'static str) {
        self.events.lock().unwrap().push(event);
    }
}

impl RoutePolicy for CallbackPolicy {
    fn on_init(&self, _: &SharedRoute) {
        self.add("init");
    }
    fn on_remove(&self, _: &SharedRoute) {
        self.add("remove");
    }
    fn on_start(&self, _: &SharedRoute) {
        self.add("start");
    }
    fn on_stop(&self, _: &SharedRoute) {
        self.add("stop");
    }
    fn on_suspend(&self, _: &SharedRoute) {
        self.add("suspend");
    }
    fn on_resume(&self, _: &SharedRoute) {
        self.add("resume");
    }
    fn on_exchange_begin(&self, _: &SharedRoute, _: &mut Exchange) {
        self.add("begin");
    }
    fn on_exchange_done(&self, _: &SharedRoute, _: &Exchange) {
        self.add("done");
    }
    fn start(&self) {
        self.add("doStart");
    }
    fn stop(&self) {
        self.add("doStop");
    }
}

#[tokio::test]
async fn test_callback() {
    for by_ref in [false, true] {
        let policy = Arc::new(CallbackPolicy::default());
        let p: SharedRoutePolicy = policy.clone();
        let t = ContextTestSupport::new();
        if by_ref {
            t.context
                .registry()
                .unwrap()
                .bind("myPolicy", route_policy_value(p.clone()))
                .unwrap();
        }
        t.context
            .add_routes(move |r: &mut Routes| {
                let route = from("direct:start").route_id("foo");
                let route = if by_ref {
                    route.route_policy_ref("myPolicy")
                } else {
                    route.route_policy(p.clone())
                };
                r.add(route.to("mock:result"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        assert!(policy.has("doStart"));
        assert!(policy.has("init"));
        assert!(!policy.has("begin"));
        assert!(!policy.has("done"));
        t.mock("mock:result").expected_message_count(1);
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
        assert!(policy.has("begin"));
        assert!(policy.has("done"));
        assert!(!policy.has("suspend"));
        t.context.suspend_route("foo").await.unwrap();
        assert!(policy.has("suspend"));
        assert!(!policy.has("resume"));
        t.context.resume_route("foo").await.unwrap();
        assert!(policy.has("resume"));
        assert!(!policy.has("stop"));
        t.context.stop_route("foo").await.unwrap();
        assert!(policy.has("stop"));
        policy.clear("start");
        t.context.start_route("foo").await.unwrap();
        assert!(policy.has("start"));
        assert!(!policy.has("remove"));
        t.context.stop_route("foo").await.unwrap();
        assert!(t.context.remove_route("foo"));
        assert!(policy.has("remove"));
        t.context.stop().await.unwrap();
        assert!(policy.has("doStop"));
    }
}

/// Creates a [`HeaderPolicy`] per route, with the route id.
struct MyRoutePolicyFactory;

impl RoutePolicyFactory for MyRoutePolicyFactory {
    fn create_route_policy(
        &self,
        _: &Arc<dyn CamelContext>,
        route_id: &str,
    ) -> Option<SharedRoutePolicy> {
        struct IdPolicy(String);
        impl RoutePolicy for IdPolicy {
            fn on_exchange_begin(&self, _: &SharedRoute, exchange: &mut Exchange) {
                exchange
                    .message_mut()
                    .set_header("RoutePolicy", self.0.clone());
            }
        }
        Some(Arc::new(IdPolicy(route_id.to_string())))
    }
}

#[tokio::test]
async fn test_route_policy_factory() {
    let t = ContextTestSupport::new();
    t.context
        .add_route_policy_factory(Arc::new(MyRoutePolicyFactory));
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:foo").route_id("foo-route").to("mock:foo"));
            r.add(from("direct:bar").route_id("bar-route").to("mock:bar"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:foo")
        .expected_header_received("RoutePolicy", "foo-route");
    t.mock("mock:bar")
        .expected_header_received("RoutePolicy", "bar-route");
    t.template
        .send_body("direct:foo", "Hello Foo")
        .await
        .unwrap();
    t.template
        .send_body("direct:bar", "Hello Bar")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `throttle.ThrottlingInflightRoutePolicyTest`.
#[tokio::test]
async fn test_throttling_inflight_route_policy() {
    use camel_core::support::throttling::ThrottlingInflightRoutePolicy;
    let url = "seda:foo?concurrentConsumers=20";
    let policy: SharedRoutePolicy =
        Arc::new(ThrottlingInflightRoutePolicy::new().with_max_inflight_exchanges(10));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(url)
                .route_policy(policy.clone())
                .delay(constant(3))
                .to("log:foo?groupSize=10")
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(90);
    for i in 0..100 {
        t.template
            .send_body(url, format!("Message {i}"))
            .await
            .unwrap();
    }
    mock.assert_is_satisfied().await;
}

/// Ports of `impl.DurationRoutePolicyMaxMessagesTest`, `impl.DurationRoutePolicyMaxSecondsTest`
/// and `impl.DurationRoutePolicyFactoryTest`: the route is stopped.
#[tokio::test]
async fn test_duration_route_policy() {
    use camel_core::engine::duration_route_policy::{
        DurationRoutePolicy, DurationRoutePolicyFactory,
    };
    for (max_messages, max_seconds, factory, minimum) in
        [(5, 0, false, 5), (0, 2, false, 1), (25, 4, true, 10)]
    {
        let t = ContextTestSupport::new();
        if factory {
            t.context
                .add_route_policy_factory(Arc::new(DurationRoutePolicyFactory {
                    max_messages,
                    max_seconds,
                    ..Default::default()
                }));
        }
        let policy: SharedRoutePolicy = Arc::new(DurationRoutePolicy::new(
            &t.context,
            max_messages,
            max_seconds,
        ));
        t.context
            .add_routes(move |r: &mut Routes| {
                let route = from("timer:foo?delay=100&period=100").route_id("foo");
                let route = if factory {
                    route
                } else {
                    route.route_policy(policy.clone())
                };
                r.add(route.to("mock:foo"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        assert!(t.context.route_status("foo").unwrap().is_started());
        let mock = t.mock("mock:foo");
        mock.expected_minimum_message_count(minimum);
        mock.set_result_wait_time(10000);
        mock.assert_is_satisfied().await;
        let context = t.context.clone();
        crate::support::await_until(std::time::Duration::from_secs(10), move || {
            context.route_status("foo").unwrap().is_stopped()
        })
        .await;
    }
}

/// Fails the exchanges (as the Java tests throwing a `ThrottlingException`).
fn failing() -> camel_core::api::processor::SharedProcessor {
    Arc::new(camel_core::api::processor::processor_fn(
        |e: &mut Exchange| Err(CamelError::runtime(e.message().body().to_string())),
    ))
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyTest`.
#[tokio::test]
async fn test_throttling_exception_route_policy() {
    use camel_core::support::throttling::{
        ThrottlingExceptionHalfOpenHandler, ThrottlingExceptionRoutePolicy,
    };
    struct NeverCloseHandler;
    impl ThrottlingExceptionHalfOpenHandler for NeverCloseHandler {
        fn is_ready_to_be_closed(&self) -> bool {
            false
        }
    }
    let url = "seda:foo?concurrentConsumers=2";
    for open in [false, true] {
        let policy: SharedRoutePolicy = Arc::new(
            ThrottlingExceptionRoutePolicy::new(2, 30, 1000, Vec::new())
                .with_half_open_handler(Arc::new(NeverCloseHandler)),
        );
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from(url)
                    .route_id("foo")
                    .route_policy(policy.clone())
                    .log("${body}")
                    .to("log:foo?groupSize=10")
                    .to("mock:result"),
            );
        })
        .await;
        let result = t.mock("mock:result");
        if !open {
            // closed: all the messages
            result.expected_minimum_message_count(100);
            for i in 0..100 {
                t.template
                    .send_body(url, format!("Message {i}"))
                    .await
                    .unwrap();
            }
            result.assert_is_satisfied().await;
            continue;
        }
        // two failures open the circuit before the third message
        result.expected_bodies_received_in_any_order(["Message One", "Message Two"]);
        result.when_any_exchange_received(failing());
        t.template.send_body(url, "Message One").await.unwrap();
        t.template.send_body(url, "Message Two").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        t.template.send_body(url, "Message Three").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        result.assert_is_satisfied().await;
        assert_eq!(2, result.received_counter());
    }
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyHalfOpenTest`: after the half open delay
/// the consumer is started again.
#[tokio::test]
async fn test_throttling_exception_route_policy_half_open() {
    use camel_core::support::throttling::ThrottlingExceptionRoutePolicy;
    let url = "direct:start?block=false";
    let policy: SharedRoutePolicy =
        Arc::new(ThrottlingExceptionRoutePolicy::new(2, 30, 250, Vec::new()));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(url)
                .route_id("foo")
                .route_policy(policy.clone())
                .to("log:foo?groupSize=10")
                .to("mock:result"),
        );
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_bodies_received_in_any_order(["Message One", "Message Two"]);
    result.when_any_exchange_received(failing());
    let _ = t.template.send_body(url, "Message One").await;
    let _ = t.template.send_body(url, "Message Two").await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // the circuit is open: no consumer
    let _ = t.template.send_body(url, "Message Three").await;
    result.assert_is_satisfied().await;
    assert_eq!(2, result.received_counter());
    // half opened: the consumer is back
    result.reset();
    result.expected_bodies_received(["Message Four"]);
    let mut delivered = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if t.template.send_body(url, "Message Four").await.is_ok() || result.received_counter() > 0
        {
            delivered = true;
            break;
        }
    }
    assert!(delivered, "the consumer should be started again");
    result.assert_is_satisfied().await;
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyKeepOpenOnInitTest`.
#[tokio::test]
async fn test_throttling_exception_route_policy_keep_open() {
    use camel_core::support::throttling::ThrottlingExceptionRoutePolicy;
    let url = "seda:foo?concurrentConsumers=20";
    for close in [false, true] {
        let policy = Arc::new(
            ThrottlingExceptionRoutePolicy::new(2, 30, 100, Vec::new()).with_keep_open(true),
        );
        let p: SharedRoutePolicy = policy.clone();
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from(url)
                    .route_policy(p.clone())
                    .log("${body}")
                    .to("log:foo?groupSize=10")
                    .to("mock:result"),
            );
        })
        .await;
        // the consumer is stopped on another task when the route starts
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        for i in 0..5 {
            t.template
                .send_body(url, format!("Message {i}"))
                .await
                .unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let result = t.mock("mock:result");
        assert_eq!(0, result.received_counter());
        if close {
            policy.set_keep_open(false);
            result.expected_message_count(5);
            result.set_result_wait_time(1500);
            result.assert_is_satisfied().await;
        }
    }
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyHalfOpenHandlerTest`: the handler closes
/// the circuit.
#[tokio::test]
async fn test_throttling_exception_route_policy_half_open_handler() {
    use camel_core::support::throttling::{
        ThrottlingExceptionHalfOpenHandler, ThrottlingExceptionRoutePolicy,
    };
    struct AlwaysCloseHandler;
    impl ThrottlingExceptionHalfOpenHandler for AlwaysCloseHandler {
        fn is_ready_to_be_closed(&self) -> bool {
            true
        }
    }
    let url = "direct:start?block=false";
    let policy: SharedRoutePolicy = Arc::new(
        ThrottlingExceptionRoutePolicy::new(2, 30, 250, Vec::new())
            .with_half_open_handler(Arc::new(AlwaysCloseHandler)),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(url)
                .route_id("foo")
                .route_policy(policy.clone())
                .log("${body}")
                .to("mock:result"),
        );
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_bodies_received_in_any_order(["Message One", "Message Two"]);
    result.when_any_exchange_received(failing());
    let _ = t.template.send_body(url, "Message One").await;
    let _ = t.template.send_body(url, "Message Two").await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _ = t.template.send_body(url, "Message Three").await;
    result.assert_is_satisfied().await;
    assert_eq!(2, result.received_counter());
    result.reset();
    result.expected_bodies_received(["Message Four"]);
    let mut delivered = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if t.template.send_body(url, "Message Four").await.is_ok() {
            delivered = true;
            break;
        }
    }
    assert!(delivered, "the circuit should be closed");
    result.assert_is_satisfied().await;
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyOpenViaConfigTest`: keepOpen toggled.
#[tokio::test]
async fn test_throttling_exception_route_policy_open_via_config() {
    use camel_core::support::throttling::ThrottlingExceptionRoutePolicy;
    let url = "seda:foo?concurrentConsumers=20";
    let policy = Arc::new(ThrottlingExceptionRoutePolicy::new(2, 30, 100, Vec::new()));
    let p: SharedRoutePolicy = policy.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(url)
                .route_id("foo")
                .route_policy(p.clone())
                .log("${body}")
                .to("mock:result"),
        );
    })
    .await;
    let result = t.mock("mock:result");
    for i in 0..5 {
        t.template
            .send_body(url, format!("MessageRound1 {i}"))
            .await
            .unwrap();
    }
    t.template.send_body(url, "MessageTrigger").await.unwrap();
    result.expected_message_count(6);
    result.assert_is_satisfied().await;
    policy.set_keep_open(true);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    for i in 0..5 {
        t.template
            .send_body(url, format!("MessageRound2 {i}"))
            .await
            .unwrap();
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(6, result.received_counter());
    policy.set_keep_open(false);
    result.expected_message_count(11);
    result.set_result_wait_time(10000);
    result.assert_is_satisfied().await;
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyReopenTest`.
#[tokio::test]
async fn test_throttling_exception_route_policy_reopen() {
    use camel_core::support::throttling::{
        ThrottlingExceptionHalfOpenHandler, ThrottlingExceptionRoutePolicy,
    };
    use std::sync::atomic::AtomicUsize;
    struct Handler {
        calls: AtomicUsize,
        ready: AtomicBool,
    }
    impl ThrottlingExceptionHalfOpenHandler for Handler {
        fn is_ready_to_be_closed(&self) -> bool {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.ready.load(Ordering::SeqCst)
        }
    }
    let handler = Arc::new(Handler {
        calls: AtomicUsize::new(0),
        ready: AtomicBool::new(false),
    });
    let handler_policy: SharedRoutePolicy = Arc::new(
        ThrottlingExceptionRoutePolicy::new(1, 60000, 200, Vec::new())
            .with_half_open_handler(handler.clone()),
    );
    let keep_open: SharedRoutePolicy = Arc::new(
        ThrottlingExceptionRoutePolicy::new(1, 60000, 200, Vec::new()).with_keep_open(true),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("seda:handler")
                .route_id("handler")
                .route_policy(handler_policy.clone())
                .to("mock:result"),
        );
        r.add(
            from("seda:keepOpen")
                .route_id("keepOpen")
                .route_policy(keep_open.clone())
                .to("mock:keepOpen"),
        );
    })
    .await;
    // the handler not ready keeps the circuit open, and is asked again
    let result = t.mock("mock:result");
    result.when_exchange_received(1, failing());
    result.expected_bodies_received(["fail", "ok"]);
    t.template.send_body("seda:handler", "fail").await.unwrap();
    let h = handler.clone();
    crate::support::await_until(std::time::Duration::from_secs(10), move || {
        h.calls.load(Ordering::SeqCst) >= 2
    })
    .await;
    handler.ready.store(true, Ordering::SeqCst);
    // closed again once the handler is ready
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    t.template.send_body("seda:handler", "ok").await.unwrap();
    result.assert_is_satisfied().await;
    // kept open after a restart of the route
    t.context.stop_route("keepOpen").await.unwrap();
    t.context.start_route("keepOpen").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let keep = t.mock("mock:keepOpen");
    keep.expected_message_count(0);
    t.template
        .send_body("seda:keepOpen", "should not be consumed")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    keep.assert_is_satisfied().await;
}

/// Port of `throttle.ThrottlingExceptionRoutePolicyHalfOpenHandlerSedaTest`: the message sent
/// while the circuit is open is consumed once it closes.
#[tokio::test]
async fn test_throttling_exception_route_policy_half_open_handler_seda() {
    use camel_core::support::throttling::{
        ThrottlingExceptionHalfOpenHandler, ThrottlingExceptionRoutePolicy,
    };
    struct AlwaysCloseHandler;
    impl ThrottlingExceptionHalfOpenHandler for AlwaysCloseHandler {
        fn is_ready_to_be_closed(&self) -> bool {
            true
        }
    }
    let url = "seda:foo?concurrentConsumers=2";
    let policy: SharedRoutePolicy = Arc::new(
        ThrottlingExceptionRoutePolicy::new(2, 30, 1000, Vec::new())
            .with_half_open_handler(Arc::new(AlwaysCloseHandler)),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(url)
                .route_id("foo")
                .route_policy(policy.clone())
                .log("${body}")
                .to("mock:result"),
        );
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_bodies_received_in_any_order(["Message One", "Message Two"]);
    result.when_any_exchange_received(failing());
    t.template.send_body(url, "Message One").await.unwrap();
    t.template.send_body(url, "Message Two").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    t.template.send_body(url, "Message Three").await.unwrap();
    result.assert_is_satisfied().await;
    result.reset();
    result.expected_bodies_received_in_any_order(["Message Three", "Message Four"]);
    result.set_result_wait_time(10000);
    t.template.send_body(url, "Message Four").await.unwrap();
    result.assert_is_satisfied().await;
}

/// Port of `throttle.ThrottlingExceptionRoutePolicySuspendRouteTest`: the half open circuit
/// does not resume the consumer of a suspended route.
#[tokio::test]
async fn test_throttling_exception_route_policy_suspend_route() {
    use camel_core::support::throttling::ThrottlingExceptionRoutePolicy;
    let policy = Arc::new(ThrottlingExceptionRoutePolicy::new(
        1,
        100,
        3000,
        Vec::new(),
    ));
    let p: SharedRoutePolicy = policy.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start?block=false")
                .route_id("foo")
                .route_policy(p.clone())
                .filter(body().is_equal_to("Kaboom"), |s| {
                    s.throw_exception(CamelError::illegal_argument("Forced"))
                })
                .to("mock:result"),
        );
    })
    .await;
    assert!(t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .is_err());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!("opened", policy.state_as_string());
    t.context.suspend_route("foo").await.unwrap();
    assert!(t.context.route_status("foo").unwrap().is_suspended());
    assert_eq!(
        "opened",
        policy.state_as_string(),
        "The half open timer should not have fired yet"
    );
    let p2 = policy.clone();
    crate::support::await_until(std::time::Duration::from_secs(10), move || {
        p2.state_as_string() == "half opened"
    })
    .await;
    assert!(t.context.route_status("foo").unwrap().is_suspended());
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    t.context.resume_route("foo").await.unwrap();
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    assert_eq!("closed", policy.state_as_string());
}
