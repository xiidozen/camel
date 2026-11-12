//! Ports of `CustomRoutePolicyTest`, `RoutePoliciesTest`, `RoutePoliciesRefTest`,
//! `impl.RoutePolicyCallbackTest`, `impl.RoutePolicyRefCallbackTest` and
//! `impl.RoutePolicyFactoryTest`.

use crate::support::ContextTestSupport;
use camel_core::api::route::Route;
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
    fn on_exchange_done(&self, route: &dyn Route, exchange: &Exchange) {
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
    fn on_exchange_begin(&self, _: &dyn Route, exchange: &mut Exchange) {
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
    fn on_init(&self, _: &dyn Route) {
        self.add("init");
    }
    fn on_remove(&self, _: &dyn Route) {
        self.add("remove");
    }
    fn on_start(&self, _: &dyn Route) {
        self.add("start");
    }
    fn on_stop(&self, _: &dyn Route) {
        self.add("stop");
    }
    fn on_suspend(&self, _: &dyn Route) {
        self.add("suspend");
    }
    fn on_resume(&self, _: &dyn Route) {
        self.add("resume");
    }
    fn on_exchange_begin(&self, _: &dyn Route, _: &mut Exchange) {
        self.add("begin");
    }
    fn on_exchange_done(&self, _: &dyn Route, _: &Exchange) {
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
            fn on_exchange_begin(&self, _: &dyn Route, exchange: &mut Exchange) {
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
