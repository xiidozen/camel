//! Ports of the tests of the threads EIP: `ThreadsDefaultTest`, `ThreadsCorePoolTest`,
//! `ThreadsCoreAndMaxPoolTest`, `ThreadsCoreAndMaxPoolKeepAliveTimeTest`,
//! `ThreadsMaxQueueSizeTest`, `ThreadsZeroMaxQueueSizeTest`, `ThreadsRejectedPolicyTest`,
//! `ThreadsCoreAndMaxPoolInvalidTest`, `ThreadsZeroInCoreAndMaxPoolTest`,
//! `ThreadsRequestReplyTest`, `Camel715ThreadProcessorTest` and the `ThreadsRejectedExecutionTest`
//! cases. Java compares the thread names before and after `threads()`; the Rust tests compare
//! the tokio task ids.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::api::enums::ThreadPoolRejectedPolicy;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type TaskIds = Arc<Mutex<(Option<tokio::task::Id>, Option<tokio::task::Id>)>>;

fn before(ids: &TaskIds) -> impl Fn(&mut Exchange) -> CamelResult<()> + Send + Sync + 'static {
    let ids = ids.clone();
    move |_| {
        ids.lock().unwrap().0 = tokio::task::try_id();
        Ok(())
    }
}

fn after(ids: &TaskIds) -> impl Fn(&mut Exchange) -> CamelResult<()> + Send + Sync + 'static {
    let ids = ids.clone();
    move |_| {
        ids.lock().unwrap().1 = tokio::task::try_id();
        Ok(())
    }
}

fn assert_other_task(ids: &TaskIds) {
    let (b, a) = *ids.lock().unwrap();
    assert!(a.is_some(), "The camel task id should be set");
    assert_ne!(b, a, "Should use different tasks");
}

#[tokio::test]
async fn test_threads_default() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .threads()
                .to("log:foo")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_threads_core_pool() {
    let ids: TaskIds = Arc::default();
    let t = ContextTestSupport::with_routes({
        let ids = ids.clone();
        move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .to("log:before")
                    .process_fn(before(&ids))
                    .threads_pool(5)
                    .process_fn(after(&ids))
                    .to("log:after")
                    .to("mock:result"),
            );
            r.add(
                from("direct:foo")
                    .process_fn(before(&ids))
                    .threads()
                    .pool_size(5)
                    .process_fn(after(&ids))
                    .to("mock:result"),
            );
        }
    })
    .await;
    for uri in ["direct:start", "direct:foo"] {
        *ids.lock().unwrap() = (None, None);
        let mock = t.mock("mock:result");
        mock.reset();
        mock.expected_message_count(1);
        t.template.send_body(uri, "Hello World").await.unwrap();
        mock.assert_is_satisfied().await;
        assert_other_task(&ids);
    }
}

/// The routes of `ThreadsCoreAndMaxPoolTest`, `ThreadsMaxQueueSizeTest`,
/// `ThreadsZeroMaxQueueSizeTest`, `ThreadsRejectedPolicyTest` and
/// `ThreadsCoreAndMaxPoolKeepAliveTimeTest`: each routes one message.
#[tokio::test]
async fn test_threads_options() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").threads_pools(5, 10).to("mock:result"));
        r.add(
            from("direct:b")
                .threads()
                .pool_size(5)
                .max_pool_size(10)
                .thread_name("myPool")
                .to("mock:result"),
        );
        r.add(
            from("direct:c")
                .threads_pools(5, 10)
                .max_queue_size(2000)
                .to("mock:result"),
        );
        r.add(
            from("direct:d")
                .threads()
                .pool_size(5)
                .max_pool_size(10)
                .max_queue_size(2000)
                .thread_name("myPool")
                .to("mock:result"),
        );
        r.add(
            from("direct:e")
                .threads_pools(5, 10)
                .max_queue_size(0)
                .to("mock:result"),
        );
        r.add(
            from("direct:f")
                .threads()
                .pool_size(5)
                .max_pool_size(10)
                .max_queue_size(0)
                .thread_name("myPool")
                .to("mock:result"),
        );
        r.add(
            from("direct:g")
                .threads_pools(5, 10)
                .rejected_policy(ThreadPoolRejectedPolicy::Abort)
                .to("mock:result"),
        );
        r.add(
            from("direct:h")
                .threads()
                .pool_size(5)
                .max_pool_size(10)
                .rejected_policy(ThreadPoolRejectedPolicy::Abort)
                .thread_name("myPool")
                .to("mock:result"),
        );
        r.add(
            from("direct:i")
                .threads_pools(5, 10)
                .keep_alive_time(10)
                .to("mock:result"),
        );
        r.add(
            from("direct:j")
                .threads()
                .max_pool_size(20)
                .thread_name("myPool")
                .to("mock:result"),
        );
    })
    .await;
    for uri in "abcdefghij".chars().map(|c| format!("direct:{c}")) {
        let mock = t.mock("mock:result");
        mock.reset();
        mock.expected_message_count(1);
        t.template.send_body(&uri, "Hello World").await.unwrap();
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_invalid_pool_sizes() {
    let t = ContextTestSupport::new();
    let e = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").threads_pools(5, 2).to("mock:result"));
        })
        .await
        .expect_err("invalid max pool size");
    let iae = cause_of(&e);
    assert!(iae.is(ErrorKind::IllegalArgument), "{e}");
    assert_eq!(
        Some("MaxPoolSize must be >= corePoolSize, was 2 >= 5"),
        iae.message()
    );
    let e = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:core").threads_pools(-1, 2).to("mock:result"));
        })
        .await
        .expect_err("invalid core pool size");
    assert!(cause_of(&e).is(ErrorKind::IllegalArgument), "{e}");
}

#[tokio::test]
async fn test_threads_in_out() {
    let ids: TaskIds = Arc::default();
    let t = ContextTestSupport::with_routes({
        let ids = ids.clone();
        move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .to("log:before")
                    .process_fn(before(&ids))
                    .threads_pool(5)
                    .process_fn(after(&ids))
                    .to("log:after")
                    .to("mock:result")
                    .transform(constant("Bye World")),
            );
        }
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    let reply = t
        .template
        .request_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!(Value::from("Bye World"), reply);
    t.assert_mock_endpoints_satisfied().await;
    assert_other_task(&ids);
}

/// Port of `Camel715ThreadProcessorTest`: the exchanges keep their own headers on the pool.
#[tokio::test]
async fn test_thread_processor() {
    const SIZE: usize = 100;
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .threads_pool(4)
                .to("mock:input")
                .process_fn(|e| {
                    let n = e.message().header("count").cloned().unwrap_or(Value::Null);
                    e.message_mut().set_header("count2", n);
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(SIZE as i32);
    let template = Arc::new(t.template);
    let mut handles = Vec::new();
    for i in 0..SIZE {
        let template = template.clone();
        handles.push(tokio::spawn(async move {
            template
                .send_body_and_header("direct:a", "Hello", "count", i as i64)
                .await
                .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    mock.assert_is_satisfied().await;
    for e in mock.received_exchanges() {
        assert_eq!(e.message().header("count"), e.message().header("count2"));
    }
}

/// The `ThreadsRejectedExecutionTest` cases (with a seda consumer: Rust routes the exchanges
/// of one consumer one after the other, the pool is never full, so all of them get through).
#[tokio::test]
async fn test_threads_rejected_execution() {
    for policy in [
        ThreadPoolRejectedPolicy::Abort,
        ThreadPoolRejectedPolicy::CallerRuns,
    ] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from("seda:start")
                    .to("log:before")
                    .threads_pools(1, 1)
                    .max_pool_size(1)
                    .max_queue_size(2)
                    .rejected_policy(policy)
                    .delay(constant(10))
                    .to("log:after")
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_message_count(10);
        for i in 0..10 {
            t.template
                .send_body("seda:start", format!("Message {i}"))
                .await
                .unwrap();
        }
        mock.assert_is_satisfied().await;
    }
}

/// The pool is full: the exchanges over the pool and queue sizes are rejected (Abort) or run
/// by the caller (CallerRuns).
#[tokio::test]
async fn test_threads_full_pool() {
    for policy in [
        ThreadPoolRejectedPolicy::Abort,
        ThreadPoolRejectedPolicy::CallerRuns,
    ] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .threads_pools(1, 1)
                    .max_queue_size(1)
                    .rejected_policy(policy)
                    .delay(constant(200))
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        let template = Arc::new(t.template);
        let mut handles = Vec::new();
        for i in 0..3 {
            let template = template.clone();
            handles.push(tokio::spawn(async move {
                // the third one comes when one runs and one waits
                tokio::time::sleep(Duration::from_millis(20 * i)).await;
                template
                    .send_body("direct:start", format!("Message {i}"))
                    .await
            }));
        }
        let mut failures = Vec::new();
        for h in handles {
            if let Err(e) = h.await.unwrap() {
                failures.push(e);
            }
        }
        match policy {
            ThreadPoolRejectedPolicy::Abort => {
                assert_eq!(1, failures.len());
                assert!(cause_of(&failures[0]).is(ErrorKind::RejectedExecution));
                assert_eq!(2, mock.received_counter());
            }
            _ => {
                assert!(failures.is_empty(), "{failures:?}");
                assert_eq!(3, mock.received_counter());
            }
        }
    }
}
