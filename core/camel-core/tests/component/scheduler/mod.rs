//! Ports of the scheduler component tests: `SchedulerRouteTest`, `SchedulerRepeatCountTest`,
//! `SchedulerNoPolledMessagesTest`, `SchedulerMulticastParallelGreedyTest`, `TwoSchedulerTest`,
//! `TwoSchedulerConcurrentTasksTest` and `TwoSchedulerConcurrentTasksOneRouteTest`
//! (`SchedulerBlockingManualTest` is a manual test).

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::SCHEDULER_POLLED_MESSAGES;
use camel_core::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_scheduler_route() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let c = c.clone();
        r.add(
            from("scheduler://foo?delay=100")
                .log("Fired scheduler")
                .process_fn(move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(2);
    mock.set_result_wait_time(30_000);
    mock.assert_is_satisfied().await;
    assert!(counter.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn test_scheduler_repeat_count() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("scheduler://foo?delay=10&initialDelay=10&repeatCount=3").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    mock.set_assert_period(1000);
    mock.assert_is_satisfied().await;
}

/// Two idle polls trigger the backoff of 10 polls (about a second).
#[tokio::test]
async fn test_scheduler_no_polled_messages() {
    let times = Arc::new(Mutex::new(Vec::new()));
    let tm = times.clone();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let tm = tm.clone();
        r.add(
            from("scheduler://foo?delay=100&initialDelay=10&backoffMultiplier=10&backoffIdleThreshold=2&poolSize=2")
                .log("Fired scheduler")
                .process_fn(move |e| {
                    tm.lock().unwrap().push(Instant::now());
                    e.set_property(SCHEDULER_POLLED_MESSAGES, false);
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(3);
    mock.set_result_wait_time(30_000);
    mock.assert_is_satisfied().await;
    let times = times.lock().unwrap().clone();
    let gap01 = times[1] - times[0];
    let gap12 = times[2] - times[1];
    assert!(gap01 <= Duration::from_millis(2000), "{gap01:?}");
    assert!(gap12 >= Duration::from_millis(200), "backoff gap {gap12:?}");
}

/// Greedy polling stops when the route tells nothing was polled.
#[tokio::test]
async fn test_scheduler_multicast_parallel_greedy() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("scheduler:testBug?initialDelay=1&useFixedDelay=true&delay=20000&greedy=true&synchronous=true")
                .multicast(|m| m.log("test"))
                .parallel_processing()
                .set_property(SCHEDULER_POLLED_MESSAGES, constant(false))
                .to("mock:parentComplete"),
        );
    })
    .await;
    let mock = t.mock("mock:parentComplete");
    mock.expected_message_count(1);
    mock.set_assert_period(200);
    mock.assert_is_satisfied().await;
}

/// Two schedulers of the same name, with a slow first exchange.
#[tokio::test]
async fn test_two_schedulers() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("scheduler://foo?delay=100&initialDelay=10")
                .to("log:a")
                .to("mock:a"),
        );
        r.add(
            from("scheduler://foo?delay=200&initialDelay=10")
                .to("log:b")
                .to("mock:b"),
        );
    })
    .await;
    t.mock("mock:a").expected_minimum_message_count(4);
    t.mock("mock:b").expected_minimum_message_count(2);
    t.assert_mock_endpoints_satisfied().await;

    let sleep = Arc::new(AtomicBool::new(true));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let sleep = sleep.clone();
        r.add(
            from("scheduler://foo?delay=250&initialDelay=10")
                .process_fn(move |_| {
                    if sleep.swap(false, Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(1000));
                    }
                    Ok(())
                })
                .to("log:done")
                .to("mock:done"),
        );
    })
    .await;
    let done = t.mock("mock:done");
    done.expected_minimum_message_count(10);
    done.set_result_wait_time(30_000);
    done.assert_is_satisfied().await;
}
