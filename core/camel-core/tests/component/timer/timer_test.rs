//! Ports of the camel-core timer tests: `TimerRepeatCountTest`, `TimerDelayTest`,
//! `TimerDelaySecondsTest`, `TimerNegativeDelayTest`, `TimerNegativeNoRepeatCountDelayTest`,
//! `TimerFiredTimeTest`, `TimerNameTest`, `TimerMultipleConsumerTest`,
//! `TimerMultipleConsumerStopRouteTest`, `TimerNegativeDelayStopRouteTest`, `TimerRestartTest`,
//! `TimerRouteTest`, `TimerGracefulShutdownTest` and `TimerEndpointTest` (the endpoint created
//! from its uri; Rust endpoints are created by their component).

use crate::support::ContextTestSupport;
use camel_bean::MethodBean;
use camel_core::api::exchange_constants::{TIMER_COUNTER, TIMER_FIRED_TIME, TIMER_NAME};
use camel_core::prelude::*;
use camel_timer::TimerEndpoint;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_repeat_count() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("timer://hello?delay=0&repeatCount=3&period=10&includeMetadata=true")
                .route_id("hello")
                .auto_startup(false)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    mock.set_assert_period(100);
    for (i, n) in [1, 2, 3].into_iter().enumerate() {
        mock.message_matches(i, header(TIMER_COUNTER).is_equal_to(n));
    }
    // we should only get 3 messages as we have a repeat count limit at 3
    t.context.start_route("hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_delay() {
    for uri in ["timer://foo?delay=10&period=10", "timer://foo?delay=1s"] {
        let uri = uri.to_string();
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).to("mock:result"));
        })
        .await;
        t.mock("mock:result").expected_minimum_message_count(1);
        t.assert_mock_endpoints_satisfied().await;
        t.context.stop().await.unwrap();
    }
}

#[tokio::test]
async fn test_negative_delay() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("timer:foo?delay=-1&period=0&repeatCount=10")
                .route_id("foo")
                .auto_startup(false)
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(10);
    t.context.start_route("foo").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_negative_no_repeat_count_delay() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("timer://negativeDelay?delay=-1&repeatCount=10&includeMetadata=true")
                .route_id("routeTest")
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
    t.context.stop_route("routeTest").await.unwrap();
    for e in mock.received_exchanges() {
        assert_eq!(Some(&Value::from("negativeDelay")), e.property(TIMER_NAME));
        assert!(e.property(TIMER_FIRED_TIME).is_some());
    }
}

#[tokio::test]
async fn test_fired_time() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("timer://hello?period=10&delay=10&includeMetadata=true").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
    let e = &mock.received_exchanges()[0];
    assert_eq!(Some(&Value::from("hello")), e.property(TIMER_NAME));
    assert!(e.property(TIMER_FIRED_TIME).is_some());
    assert!(e.message().header(TIMER_FIRED_TIME).is_some());
    assert_eq!(Some(&Value::Int(1)), e.property(TIMER_COUNTER));
}

#[tokio::test]
async fn test_timer_name() {
    let uri = "timer://foo?delay=0&period=10&timerName=bar";
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(uri)
                .route_id("foo")
                .auto_startup(false)
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_minimum_message_count(1);
    t.context.start_route("foo").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let e = t.context.endpoint(uri).unwrap();
    let te = e.as_any().downcast_ref::<TimerEndpoint>().unwrap();
    assert_eq!("bar", te.timer_name());
}

#[tokio::test]
async fn test_multiple_consumers() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("timer:mytimer?period=0&delay=10").to("mock:foo"));
        r.add(from("timer:mytimer?period=0&delay=10").to("mock:bar"));
    })
    .await;
    t.mock("mock:foo").expected_minimum_message_count(1);
    t.mock("mock:bar").expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_multiple_consumers_stop_route() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("timer:mytimer?period=100")
                .route_id("foo")
                .to("mock:foo"),
        );
        r.add(
            from("timer:mytimer?period=100")
                .route_id("bar")
                .to("mock:bar"),
        );
    })
    .await;
    t.mock("mock:foo").expected_minimum_message_count(1);
    t.mock("mock:bar").expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
    t.context.stop_route("bar").await.unwrap();
    t.reset_mocks();
    // stopping bar route, we should still keep getting messages to foo
    t.mock("mock:foo").expected_minimum_message_count(2);
    t.mock("mock:bar").expected_message_count(0);
    t.assert_mock_endpoints_satisfied().await;
}

async fn negative_delay_stop_route(restart: bool) {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("timer:mytimer?period=100")
                .route_id("foo")
                .to("mock:foo"),
        );
        r.add(
            from("timer:mytimer?delay=-1&repeatCount=1")
                .route_id("bar")
                .auto_startup(false)
                .to("mock:bar"),
        );
    })
    .await;
    t.mock("mock:foo").expected_minimum_message_count(1);
    t.mock("mock:bar").expected_message_count(1);
    t.context.start_route("bar").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    t.context.stop_route("bar").await.unwrap();
    if restart {
        t.context.start_route("bar").await.unwrap();
        t.context.stop_route("bar").await.unwrap();
    }
    t.reset_mocks();
    // stopping bar route, we should still keep getting messages to foo
    t.mock("mock:foo").expected_minimum_message_count(2);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_stop_negative_delay_route() {
    negative_delay_stop_route(false).await;
}

#[tokio::test]
async fn test_restart_negative_delay_route() {
    negative_delay_stop_route(true).await;
}

#[tokio::test]
async fn test_timer_restart() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("timer://foo?fixedRate=true&delay=0&period=10").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
    t.context.stop().await.unwrap();
    mock.reset();
    t.context.start().await.unwrap();
    mock.expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_timer_invokes_bean_method() {
    // Java MyBean.someMethod(), a void method
    let counter = Arc::new(AtomicI64::new(0));
    let c = counter.clone();
    let bean = MethodBean::new("MyBean").void_method("someMethod", move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("myBean", bean.into_value())
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("timer://foo?fixedRate=true&delay=0&period=10")
                    .log("Fired timer")
                    .to("bean:myBean")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result").expected_minimum_message_count(2);
    t.assert_mock_endpoints_satisfied().await;
    let fired = counter.load(Ordering::SeqCst);
    assert!(fired >= 2, "Should have fired 2 or more times was: {fired}");
}

#[tokio::test]
async fn test_timer_graceful_shutdown() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("timer:foo?period=10&delay=10")
                .delay(constant(10))
                .to("log:time")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_minimum_message_count(1);
    t.assert_mock_endpoints_satisfied().await;
    // stopping waits for the message in flight
    tokio::time::timeout(Duration::from_secs(5), t.context.stop())
        .await
        .expect("the timer stops")
        .unwrap();
}

#[tokio::test]
async fn test_timer_endpoint_no_producer() {
    let t = ContextTestSupport::new();
    let e = t.context.endpoint("timer://foo").unwrap();
    assert!(
        e.clone().create_producer().is_err(),
        "Should have thrown an exception"
    );
    let te = e.as_any().downcast_ref::<TimerEndpoint>().unwrap();
    assert_eq!(
        (1000, 1000, 0),
        (te.period(), te.delay(), te.repeat_count())
    );
}
