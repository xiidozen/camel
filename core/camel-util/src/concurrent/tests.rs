//! Port of the `org.apache.camel.util.concurrent` tests.

use super::*;
use std::sync::atomic::AtomicI32;

/// Waits until the condition is true (the Java tests used Awaitility).
fn await_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        thread::yield_now();
        thread::sleep(Duration::from_millis(5));
    }
    cond()
}

const SECS5: Duration = Duration::from_secs(5);

// --- AsyncCompletionServiceTest ---

fn service() -> AsyncCompletionService<String> {
    AsyncCompletionService::new(Arc::new(ThreadPoolExecutor::fixed(5, "test")), true)
}

fn result(r: &str) -> impl FnOnce(Box<dyn FnOnce(String) + Send>) + Send + 'static {
    let r = r.to_string();
    move |cb| cb(r)
}

fn result_delay(
    r: &str,
    delay: u64,
) -> impl FnOnce(Box<dyn FnOnce(String) + Send>) + Send + 'static {
    let r = r.to_string();
    move |cb| {
        // the delay is what is being tested (a slow task), not a wait for a condition
        thread::sleep(Duration::from_millis(delay));
        cb(r)
    }
}

fn result_latch(
    r: &str,
    latch: Arc<CountDownLatch>,
) -> impl FnOnce(Box<dyn FnOnce(String) + Send>) + Send + 'static {
    let r = r.to_string();
    move |cb| {
        latch.await_timeout(SECS5);
        cb(r)
    }
}

#[test]
fn test_submit_ordered() {
    let s = service();
    s.submit(result("A")).unwrap();
    s.submit(result("B")).unwrap();
    assert_eq!("A", s.take());
    assert_eq!("B", s.take());
}

#[test]
fn test_submit_ordered_first_task_is_slow() {
    let s = service();
    s.submit(result_delay("A", 200)).unwrap();
    s.submit(result("B")).unwrap();
    assert_eq!("A", s.take());
    assert_eq!("B", s.take());
}

#[test]
fn test_submit_ordered_first_task_is_slow_using_poll_timeout() {
    let s = service();
    s.submit(result_delay("A", 200)).unwrap();
    s.submit(result("B")).unwrap();
    assert_eq!(Some("A".to_string()), s.poll_timeout(SECS5));
    assert_eq!(Some("B".to_string()), s.poll_timeout(SECS5));
}

#[test]
fn test_submit_ordered_first_task_is_slow_using_poll() {
    let s = service();
    let latch = Arc::new(CountDownLatch::new(1));
    s.submit(result_latch("A", latch.clone())).unwrap();
    s.submit(result("B")).unwrap();
    assert_eq!(None, s.poll());
    assert_eq!(None, s.poll());
    latch.count_down();
    assert_eq!("A", s.take());
    assert_eq!("B", s.take());
}

#[test]
fn test_submit_ordered_second_task_is_slow() {
    let s = service();
    s.submit(result("A")).unwrap();
    s.submit(result_delay("B", 100)).unwrap();
    assert_eq!("A", s.take());
    assert_eq!("B", s.take());
}

#[test]
fn test_submit_ordered_second_task_is_slow_using_poll_timeout() {
    let s = service();
    s.submit(result("A")).unwrap();
    s.submit(result_delay("B", 100)).unwrap();
    assert_eq!(Some("A".to_string()), s.poll_timeout(SECS5));
    assert_eq!(Some("B".to_string()), s.poll_timeout(SECS5));
}

#[test]
fn test_submit_ordered_last_task_is_slow_using_poll() {
    let s = service();
    let latch = Arc::new(CountDownLatch::new(1));
    s.submit(result("A")).unwrap();
    s.submit(result_latch("B", latch.clone())).unwrap();
    assert_eq!("A", s.take());
    assert_eq!(None, s.poll());
    assert_eq!(None, s.poll());
    latch.count_down();
    assert_eq!("B", s.take());
}

#[test]
fn test_unordered_and_skip() {
    let s: AsyncCompletionService<i32> =
        AsyncCompletionService::new(Arc::new(SynchronousExecutorService::new()), true);
    s.submit(|cb| cb(1)).unwrap();
    s.skip();
    assert_eq!(None, s.poll());
    assert_eq!(1, s.size());
    assert_eq!(Some(1), s.poll_unordered());
}

// --- BoundedExecutorServiceTest ---

fn blocking_task(latch: &Arc<CountDownLatch>) -> Task {
    let l = latch.clone();
    Box::new(move || {
        l.await_timeout(Duration::from_secs(10));
    })
}

#[test]
fn test_caller_runs_on_timeout() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let sized = BoundedExecutorService::new(
        delegate.clone(),
        1,
        Duration::from_millis(200),
        ThreadPoolRejectedPolicy::CallerRuns,
    );
    let block = Arc::new(CountDownLatch::new(1));
    sized.execute(blocking_task(&block)).unwrap();
    let ran_on_caller = Arc::new(AtomicBool::new(false));
    let caller = thread::current().id();
    let r = ran_on_caller.clone();
    sized
        .execute(Box::new(move || {
            r.store(thread::current().id() == caller, Ordering::SeqCst)
        }))
        .unwrap();
    assert!(
        ran_on_caller.load(Ordering::SeqCst),
        "After timeout, task should run on the caller's thread"
    );
    assert_eq!(1, sized.caller_runs_count());
    assert_eq!(0, sized.rejected_count());
    block.count_down();
    sized.shutdown();
}

#[test]
fn test_abort_on_timeout() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let sized = BoundedExecutorService::new(
        delegate,
        1,
        Duration::from_millis(200),
        ThreadPoolRejectedPolicy::Abort,
    );
    let block = Arc::new(CountDownLatch::new(1));
    sized.execute(blocking_task(&block)).unwrap();
    assert!(
        sized.execute(Box::new(|| {})).is_err(),
        "Should reject after timeout with Abort policy"
    );
    assert_eq!(0, sized.caller_runs_count());
    assert_eq!(1, sized.rejected_count());
    block.count_down();
    sized.shutdown();
}

#[test]
fn test_block_forever_policy() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let sized = Arc::new(BoundedExecutorService::new(
        delegate,
        1,
        Duration::from_secs(60),
        ThreadPoolRejectedPolicy::Block,
    ));
    let block = Arc::new(CountDownLatch::new(1));
    let second_started = Arc::new(CountDownLatch::new(1));
    sized.execute(blocking_task(&block)).unwrap();
    let (s2, ss) = (sized.clone(), second_started.clone());
    let submitter = thread::spawn(move || {
        s2.execute(Box::new(move || ss.count_down())).unwrap();
    });
    assert!(
        await_until(SECS5, || sized.waiting_count() > 0),
        "Submitter should be blocked waiting for a permit"
    );
    block.count_down();
    assert!(
        second_started.await_timeout(SECS5),
        "Second task should run after first completes"
    );
    submitter.join().unwrap();
    sized.shutdown();
}

#[test]
fn test_concurrency_bounded() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let max = 3;
    let sized = Arc::new(BoundedExecutorService::new(
        delegate,
        max,
        Duration::from_secs(60),
        ThreadPoolRejectedPolicy::Block,
    ));
    let in_flight = Arc::new(AtomicI32::new(0));
    let peak = Arc::new(AtomicI32::new(0));
    let total = 20;
    let hold = Arc::new(CountDownLatch::new(1));
    let all_done = Arc::new(CountDownLatch::new(total));
    let senders = ThreadPoolExecutor::fixed(total, "senders");
    for _ in 0..total {
        let (s, f, p, h, d) = (
            sized.clone(),
            in_flight.clone(),
            peak.clone(),
            hold.clone(),
            all_done.clone(),
        );
        senders
            .execute(Box::new(move || {
                s.execute(Box::new(move || {
                    let current = f.fetch_add(1, Ordering::SeqCst) + 1;
                    p.fetch_max(current, Ordering::SeqCst);
                    h.await_timeout(Duration::from_secs(10));
                    f.fetch_sub(1, Ordering::SeqCst);
                    d.count_down();
                }))
                .unwrap();
            }))
            .unwrap();
    }
    assert!(await_until(SECS5, || sized.active_count() == max));
    hold.count_down();
    assert!(
        all_done.await_timeout(Duration::from_secs(30)),
        "All tasks should complete"
    );
    assert!(
        peak.load(Ordering::SeqCst) as usize <= max,
        "Peak concurrency should be <= {max}"
    );
    senders.shutdown();
    sized.shutdown();
}

#[test]
fn test_permits_released_after_completion() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let sized = BoundedExecutorService::new(
        delegate,
        2,
        Duration::from_secs(60),
        ThreadPoolRejectedPolicy::Block,
    );
    for _ in 0..2 {
        let batch = Arc::new(CountDownLatch::new(2));
        for _ in 0..2 {
            let b = batch.clone();
            sized.execute(Box::new(move || b.count_down())).unwrap();
        }
        assert!(batch.await_timeout(SECS5));
        assert!(await_until(SECS5, || sized.available_permits() == 2));
    }
    assert_eq!(4, sized.delegated_task_count());
    sized.shutdown();
}

#[test]
fn test_submit_after_shutdown() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let sized = BoundedExecutorService::new(
        delegate.clone(),
        5,
        Duration::from_secs(60),
        ThreadPoolRejectedPolicy::CallerRuns,
    );
    sized.shutdown();
    assert!(sized.is_shutdown());
    assert!(delegate.is_shutdown());
    assert!(
        sized.execute(Box::new(|| {})).is_err(),
        "Should reject after shutdown"
    );
}

#[test]
fn test_submit_returns_future() {
    let delegate: Arc<dyn Executor> = Arc::new(ThreadPoolExecutor::cached("bounded"));
    let sized = BoundedExecutorService::new(
        delegate,
        5,
        Duration::from_secs(60),
        ThreadPoolRejectedPolicy::CallerRuns,
    );
    let executed = Arc::new(AtomicBool::new(false));
    let e = executed.clone();
    let future = submit(&sized, move || e.store(true, Ordering::SeqCst)).unwrap();
    assert!(future.get(SECS5).unwrap().is_ok());
    assert!(executed.load(Ordering::SeqCst));
    assert!(future.is_done());
    sized.shutdown();
}

// --- ContextValueTest ---

#[test]
fn test_basic_usage() {
    let route_id: ContextValue<String> = ContextValue::new_instance("routeId");
    assert!(!route_id.is_bound());
    assert_eq!(None, route_id.get());
    assert_eq!("default", route_id.or_else("default".into()));
    route_id.where_("myRoute".into(), || {
        assert!(route_id.is_bound());
        assert_eq!(Some("myRoute".to_string()), route_id.get());
        assert_eq!("myRoute", route_id.or_else("default".into()));
    });
}

#[test]
fn test_nested_scopes() {
    let route_id: ContextValue<String> = ContextValue::new_instance("routeId");
    route_id.where_("outer".into(), || {
        assert_eq!(Some("outer".to_string()), route_id.get());
        route_id.where_("inner".into(), || {
            assert_eq!(Some("inner".to_string()), route_id.get())
        });
        assert_eq!(Some("outer".to_string()), route_id.get());
    });
    assert!(!route_id.is_bound());
}

#[test]
fn test_multiple_context_values() {
    let route_id: ContextValue<String> = ContextValue::new_instance("routeId");
    let exchange_id: ContextValue<String> = ContextValue::new_instance("exchangeId");
    route_id.where_("route1".into(), || {
        exchange_id.where_("exchange1".into(), || {
            assert_eq!(Some("route1".to_string()), route_id.get());
            assert_eq!(Some("exchange1".to_string()), exchange_id.get());
        })
    });
}

#[test]
fn test_thread_local_context_value() {
    let v: ContextValue<String> = ContextValue::new_thread_local("mutable");
    assert!(!v.is_bound());
    v.set("value1".into());
    assert!(v.is_bound());
    assert_eq!(Some("value1".to_string()), v.get());
    v.set("value2".into());
    assert_eq!(Some("value2".to_string()), v.get());
    v.remove();
    assert!(!v.is_bound());
    let with: ContextValue<i32> = ContextValue::new_thread_local_with("init", || 42);
    assert_eq!(Some(42), with.get());
}

#[test]
fn test_where_with_supplier() {
    let route_id: ContextValue<String> = ContextValue::new_instance("routeId");
    let r = route_id.where_("myRoute".into(), || {
        format!("Result: {}", route_id.get().unwrap())
    });
    assert_eq!("Result: myRoute", r);
}

#[test]
fn test_thread_isolation() {
    let route_id: Arc<ContextValue<String>> = Arc::new(ContextValue::new_instance("routeId"));
    route_id.where_("mainThread".into(), || {
        assert_eq!(Some("mainThread".to_string()), route_id.get());
        let r = route_id.clone();
        thread::spawn(move || assert_eq!(None, r.get()))
            .join()
            .unwrap();
        assert_eq!(Some("mainThread".to_string()), route_id.get());
    });
}

#[test]
fn test_name() {
    let v: ContextValue<String> = ContextValue::new_instance("testName");
    assert_eq!("testName", v.name());
    assert!(v.to_string().contains("testName"));
}

// --- SizedScheduledExecutorServiceTest ---

#[test]
fn test_sized_scheduled_executor_service() {
    let delegate = ScheduledThreadPoolExecutor::new(5, "sized");
    let sized = SizedScheduledExecutorService::new(delegate.clone(), 2);
    sized
        .schedule(Duration::from_secs(2), Box::new(|| {}))
        .unwrap();
    sized
        .schedule(Duration::from_secs(3), Box::new(|| {}))
        .unwrap();
    let e = sized
        .schedule(Duration::from_secs(4), Box::new(|| {}))
        .unwrap_err();
    assert_eq!(QUEUE_SIZE_LIMIT_REACHED, e.0);
    sized.shutdown_now();
    assert!(sized.is_shutdown());
    assert!(delegate.is_shutdown());
}

// --- SynchronousExecutorServiceTest ---

#[test]
fn test_synchronous_executor_service() {
    let caller = thread::current().id();
    let service = SynchronousExecutorService::new();
    let same = Arc::new(AtomicBool::new(false));
    let s = same.clone();
    service
        .execute(Box::new(move || {
            s.store(thread::current().id() == caller, Ordering::SeqCst)
        }))
        .unwrap();
    assert!(same.load(Ordering::SeqCst), "Should use same thread");
    service.shutdown();
}

#[test]
fn test_synchronous_executor_service_shutdown() {
    let service = SynchronousExecutorService::new();
    service.execute(Box::new(|| {})).unwrap();
    service.shutdown();
    assert!(service.is_shutdown());
    assert!(service.is_terminated());
}

// --- additional coverage ---

#[test]
fn test_thread_pool_policies() {
    // one thread, a queue of one: the third task is rejected (Abort) or run by the caller
    let block = Arc::new(CountDownLatch::new(1));
    let pool = ThreadPoolExecutor::new(
        1,
        1,
        Duration::from_secs(60),
        Some(1),
        ThreadPoolRejectedPolicy::Abort,
        "abort",
    );
    pool.execute(blocking_task(&block)).unwrap();
    pool.execute(Box::new(|| {})).unwrap();
    assert!(pool.execute(Box::new(|| {})).is_err());
    let rejected = Arc::new(AtomicBool::new(false));
    let r = rejected.clone();
    pool.execute_rejectable(
        Box::new(|| {}),
        Box::new(move || r.store(true, Ordering::SeqCst)),
    );
    assert!(rejected.load(Ordering::SeqCst));
    block.count_down();
    pool.shutdown();
    assert!(pool.await_termination(SECS5));
    assert!(pool.is_terminated());
    assert!(pool.execute(Box::new(|| {})).is_err());

    let block = Arc::new(CountDownLatch::new(1));
    let pool = ThreadPoolExecutor::new(
        1,
        1,
        Duration::from_secs(60),
        Some(1),
        ThreadPoolRejectedPolicy::CallerRuns,
        "caller",
    );
    pool.execute(blocking_task(&block)).unwrap();
    pool.execute(Box::new(|| {})).unwrap();
    let caller = thread::current().id();
    let same = Arc::new(AtomicBool::new(false));
    let s = same.clone();
    pool.execute(Box::new(move || {
        s.store(thread::current().id() == caller, Ordering::SeqCst)
    }))
    .unwrap();
    assert!(same.load(Ordering::SeqCst));
    block.count_down();
    assert!(await_until(SECS5, || pool.completed_task_count() == 2));
    assert_eq!(1, pool.largest_pool_size());
    assert!(pool.to_string().contains("caller"));
    pool.shutdown_now();

    // grows above the core size when the queue is full
    let block = Arc::new(CountDownLatch::new(1));
    let pool = ThreadPoolExecutor::new(
        1,
        3,
        Duration::from_millis(50),
        Some(1),
        ThreadPoolRejectedPolicy::Abort,
        "grow",
    );
    for _ in 0..4 {
        pool.execute(blocking_task(&block)).unwrap();
    }
    assert_eq!(3, pool.pool_size());
    assert_eq!(1, pool.queue_size());
    block.count_down();
    // threads above the core size stop after the keep alive time
    assert!(await_until(SECS5, || pool.pool_size() == 1));
    pool.shutdown();
}

#[test]
fn test_scheduled() {
    let scheduler = ScheduledThreadPoolExecutor::new(2, "sched");
    let count = Arc::new(AtomicI32::new(0));
    let c = count.clone();
    let rate = scheduler
        .schedule_at_fixed_rate(Duration::ZERO, Duration::from_millis(10), move || {
            c.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    assert!(await_until(SECS5, || count.load(Ordering::SeqCst) >= 3));
    assert!(rate.cancel());
    assert!(rate.is_cancelled() && rate.is_done());
    assert!(!rate.cancel());

    let count2 = Arc::new(AtomicI32::new(0));
    let c2 = count2.clone();
    let delay = scheduler
        .schedule_with_fixed_delay(Duration::ZERO, Duration::from_millis(5), move || {
            c2.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    assert!(await_until(SECS5, || count2.load(Ordering::SeqCst) >= 3));
    delay.cancel();

    let ran = Arc::new(AtomicBool::new(false));
    let r = ran.clone();
    let once = scheduler
        .schedule(
            Duration::from_secs(60),
            Box::new(move || r.store(true, Ordering::SeqCst)),
        )
        .unwrap();
    assert!(once.cancel());
    let latch = Arc::new(CountDownLatch::new(1));
    let l = latch.clone();
    scheduler.execute(Box::new(move || l.count_down())).unwrap();
    assert!(latch.await_timeout(SECS5));
    assert!(!ran.load(Ordering::SeqCst));
    scheduler.shutdown();
    assert!(scheduler.is_shutdown());
    assert!(scheduler.schedule(Duration::ZERO, Box::new(|| {})).is_err());
}

#[test]
fn test_thread_names() {
    let factory = CamelThreadFactory::new(Some("#name# - #counter#"), "myThread?x=1", true);
    let name = factory
        .new_thread(|| thread::current().name().unwrap().to_string())
        .unwrap()
        .join()
        .unwrap();
    assert!(name.starts_with("myThread - "));
    assert!(!factory.is_virtual());
    assert!(factory.is_daemon());
    assert_eq!("CamelThreadFactory[myThread?x=1]", factory.to_string());
    let long = resolve_thread_name(Some("#longName#"), "a?b").unwrap();
    assert_eq!("a?b", long);
    assert_eq!(
        Err("Pattern is invalid: [#bad#] in resolved thread name: [#bad#]".to_string()),
        resolve_thread_name(Some("#bad#"), "x")
    );
    assert_eq!(Ok(ThreadPoolRejectedPolicy::Block), "Block".parse());
    assert!("x".parse::<ThreadPoolRejectedPolicy>().is_err());
    assert_eq!(
        "CallerRuns",
        ThreadPoolRejectedPolicy::default().to_string()
    );
    assert_eq!(ThreadType::Platform, ThreadType::current());
}
