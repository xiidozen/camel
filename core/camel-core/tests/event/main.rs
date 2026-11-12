//! Tests ported from the Java camel-core package `org.apache.camel.impl.event`: the events of
//! the context, the routes and the exchanges.

#[path = "../support/mod.rs"]
mod support;

use camel_core::api::spi::event::{CamelEvent, EventNotifier, EventNotifierSupport, EventType};
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::ContextTestSupport;

/// A recorded event: its type, the endpoint uri and the time taken of the sent events.
#[derive(Debug, Clone)]
struct Recorded {
    event_type: EventType,
    endpoint: Option<String>,
    time_taken: Option<Duration>,
    from_endpoint: Option<String>,
    created: Option<i64>,
}

type Events = Arc<Mutex<Vec<Recorded>>>;

fn record(events: &Events, e: &CamelEvent<'_>) {
    let (endpoint, time_taken) = match e {
        CamelEvent::ExchangeSending { endpoint, .. } => (Some(endpoint.to_string()), None),
        CamelEvent::ExchangeSent {
            endpoint,
            time_taken,
            ..
        } => (Some(endpoint.to_string()), Some(*time_taken)),
        _ => (None, None),
    };
    events.lock().unwrap().push(Recorded {
        event_type: e.event_type(),
        endpoint,
        time_taken,
        from_endpoint: e
            .exchange()
            .and_then(|x| x.from_endpoint().map(str::to_string)),
        created: e.exchange().map(|x| x.created()),
    });
}

fn types(events: &Events) -> Vec<EventType> {
    events
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.event_type)
        .collect()
}

/// A context with the notifier (added before the routes, as Java `createCamelContext`).
async fn context_with(
    notifier: impl EventNotifier + 'static,
    routes: impl RouteBuilder,
) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    t.context.add_event_notifier(Arc::new(notifier));
    t.context.add_routes(routes).await.unwrap();
    t.context.start().await.unwrap();
    t
}

fn events_routes(r: &mut Routes) {
    r.add(from("direct:start").to("log:foo").to("mock:result"));
    r.add(from("direct:fail").throw_exception(CamelError::illegal_argument("Damn")));
}

use EventType as T;

const STARTED: [EventType; 12] = [
    T::CamelContextInitializing,
    T::CamelContextInitialized,
    T::CamelContextStarting,
    T::CamelContextRoutesStarting,
    T::RouteAdded,
    T::RouteAdded,
    T::RouteStarting,
    T::RouteStarted,
    T::RouteStarting,
    T::RouteStarted,
    T::CamelContextRoutesStarted,
    T::CamelContextStarted,
];

const STOPPED: [EventType; 10] = [
    T::CamelContextStopping,
    T::CamelContextRoutesStopping,
    T::RouteStopping,
    T::RouteStopped,
    T::RouteRemoved,
    T::RouteStopping,
    T::RouteStopped,
    T::RouteRemoved,
    T::CamelContextRoutesStopped,
    T::CamelContextStopped,
];

fn recorder() -> (Events, impl EventNotifier + 'static) {
    let events: Events = Arc::default();
    let e = events.clone();
    (
        events,
        EventNotifierSupport::new(move |event: &CamelEvent<'_>| record(&e, event)),
    )
}

/// Port of `EventNotifierEventsTest.testExchangeDone`.
#[tokio::test]
async fn test_exchange_done() {
    let (events, notifier) = recorder();
    let t = context_with(notifier, events_routes).await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    let mut expected = STARTED.to_vec();
    expected.extend([
        T::ExchangeSending,
        T::ExchangeCreated,
        T::ExchangeSending,
        T::ExchangeSent,
        T::ExchangeSending,
        T::ExchangeSent,
        T::ExchangeCompleted,
        // the sent of the producer template starting the test
        T::ExchangeSent,
    ]);
    assert_eq!(expected, types(&events));
    t.context.stop().await.unwrap();
    expected.extend(STOPPED);
    assert_eq!(expected, types(&events));
}

/// Port of `EventNotifierEventsTest.testExchangeFailed`.
#[tokio::test]
async fn test_exchange_failed() {
    let (events, notifier) = recorder();
    let t = context_with(notifier, events_routes).await;
    let e = t
        .template
        .send_body("direct:fail", "Hello World")
        .await
        .expect_err("Should have thrown an exception");
    assert!(support::cause_of(&e).is(ErrorKind::IllegalArgument), "{e}");
    let mut expected = STARTED.to_vec();
    expected.extend([
        T::ExchangeSending,
        T::ExchangeCreated,
        T::ExchangeFailed,
        T::ExchangeSent,
    ]);
    assert_eq!(expected, types(&events));
    t.context.stop().await.unwrap();
    expected.extend(STOPPED);
    assert_eq!(expected, types(&events));
}

/// Port of `EventNotifierIgnoreCamelContextInitEventsTest`.
#[tokio::test]
async fn test_ignore_init_events() {
    let (events, _) = recorder();
    let e = events.clone();
    let notifier = EventNotifierSupport::new(move |event: &CamelEvent<'_>| record(&e, event))
        .ignore_camel_context_init_events(true);
    let t = context_with(notifier, events_routes).await;
    assert_eq!(STARTED[2..].to_vec(), types(&events));
    t.context.stop().await.unwrap();
    let mut expected = STARTED[2..].to_vec();
    expected.extend(STOPPED);
    assert_eq!(expected, types(&events));
}

struct CompletedOnly(Events);

impl EventNotifier for CompletedOnly {
    fn notify(&self, event: &CamelEvent<'_>) {
        record(&self.0, event);
    }

    fn is_enabled(&self, event: &CamelEvent<'_>) -> bool {
        event.event_type() == T::ExchangeCompleted
    }
}

fn sent_routes(r: &mut Routes) {
    r.add(
        from("direct:start")
            .to("log:foo")
            .to("direct:bar")
            .to("mock:result"),
    );
    r.add(from("direct:bar").delay(constant(500)));
    r.add(from("direct:foo").recipient_list(header("foo")));
    r.add(from("direct:cool").delay(constant(1000)));
    r.add(from("direct:tap").wire_tap("log:foo").to("mock:result"));
}

/// Port of `EventNotifierExchangeCompletedTest`.
#[tokio::test]
async fn test_exchange_completed() {
    let events: Events = Arc::default();
    let t = context_with(CompletedOnly(events.clone()), |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("log:foo")
                .to("direct:bar")
                .to("mock:result"),
        );
        r.add(from("direct:bar").delay(constant(500)).to("mock:bar"));
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.mock("mock:result").assert_is_satisfied().await;
    let recorded = events.lock().unwrap().clone();
    assert_eq!(1, recorded.len());
    assert_eq!(Some("direct://start"), recorded[0].from_endpoint.as_deref());
    let created = recorded[0].created.unwrap();
    assert!(created > 0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let elapsed = now - created;
    assert!(elapsed > 400, "Should be > 400, was: {elapsed}");
}

fn sent_notifier(events: &Events) -> impl EventNotifier + 'static {
    let e = events.clone();
    EventNotifierSupport::new(move |event: &CamelEvent<'_>| record(&e, event))
        .ignore_camel_context_events(true)
        .ignore_route_events(true)
        .ignore_exchange_created_event(true)
        .ignore_exchange_completed_event(true)
        .ignore_exchange_failed_events(true)
        .ignore_exchange_redelivery_events(true)
}

fn sent(events: &Events) -> Vec<(EventType, String)> {
    events
        .lock()
        .unwrap()
        .iter()
        .map(|r| (r.event_type, r.endpoint.clone().unwrap_or_default()))
        .collect()
}

/// Port of `EventNotifierExchangeSentTest.testExchangeSent`.
#[tokio::test]
async fn test_exchange_sent() {
    let events: Events = Arc::default();
    let t = context_with(sent_notifier(&events), sent_routes).await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.mock("mock:result").assert_is_satisfied().await;
    let s = |t: EventType, uri: &str| (t, uri.to_string());
    assert_eq!(
        vec![
            s(T::ExchangeSending, "direct://start"),
            s(T::ExchangeSending, "log://foo"),
            s(T::ExchangeSent, "log://foo"),
            s(T::ExchangeSending, "direct://bar"),
            s(T::ExchangeSent, "direct://bar"),
            s(T::ExchangeSending, "mock://result"),
            s(T::ExchangeSent, "mock://result"),
            s(T::ExchangeSent, "direct://start"),
        ],
        sent(&events)
    );
    let recorded = events.lock().unwrap().clone();
    for i in [4, 7] {
        let time = recorded[i].time_taken.unwrap();
        assert!(
            time > Duration::from_millis(400),
            "Should take about 0.5 sec, was: {time:?}"
        );
    }
}

/// Port of `EventNotifierExchangeSentTest.testExchangeSentRecipient`.
#[tokio::test]
async fn test_exchange_sent_recipient() {
    let events: Events = Arc::default();
    let t = context_with(sent_notifier(&events), sent_routes).await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body_and_header(
            "direct:foo",
            "Hello World",
            "foo",
            "direct:cool,direct:start",
        )
        .await
        .unwrap();
    t.mock("mock:result").assert_is_satisfied().await;
    let s = |t: EventType, uri: &str| (t, uri.to_string());
    assert_eq!(
        vec![
            s(T::ExchangeSending, "direct://foo"),
            s(T::ExchangeSending, "direct://cool"),
            s(T::ExchangeSent, "direct://cool"),
            s(T::ExchangeSending, "direct://start"),
            s(T::ExchangeSending, "log://foo"),
            s(T::ExchangeSent, "log://foo"),
            s(T::ExchangeSending, "direct://bar"),
            s(T::ExchangeSent, "direct://bar"),
            s(T::ExchangeSending, "mock://result"),
            s(T::ExchangeSent, "mock://result"),
            s(T::ExchangeSent, "direct://start"),
            s(T::ExchangeSent, "direct://foo"),
        ],
        sent(&events)
    );
}

/// Port of `EventNotifierExchangeSentTest.testExchangeWireTap`.
#[tokio::test]
async fn test_exchange_wire_tap() {
    let events: Events = Arc::default();
    let t = context_with(sent_notifier(&events), sent_routes).await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:tap", "Hello World")
        .await
        .unwrap();
    t.mock("mock:result").assert_is_satisfied().await;
    support::await_until(Duration::from_secs(1), || events.lock().unwrap().len() == 6).await;
    let all = sent(&events);
    assert!(
        all.contains(&(T::ExchangeSending, "log://foo".into())),
        "We should find log:foo being wire tapped"
    );
    assert!(
        all.contains(&(T::ExchangeSent, "log://foo".into())),
        "We should find log:foo being wire tapped"
    );
}

/// A recorded failure event: its type, whether a dead letter channel, its uri, handled.
fn failures(events: &Events) -> Vec<EventType> {
    types(events)
}

struct FailureRecorder {
    events: Events,
    details: Details,
}

impl EventNotifier for FailureRecorder {
    fn notify(&self, event: &CamelEvent<'_>) {
        record(&self.events, event);
        let d = match event {
            CamelEvent::ExchangeFailureHandling {
                dead_letter,
                dead_letter_uri,
                ..
            } => Some((*dead_letter, dead_letter_uri.map(str::to_string), None, 0)),
            CamelEvent::ExchangeFailureHandled {
                dead_letter,
                dead_letter_uri,
                handled,
                ..
            } => Some((
                *dead_letter,
                dead_letter_uri.map(str::to_string),
                Some(*handled),
                0,
            )),
            CamelEvent::ExchangeRedelivery { attempt, .. } => Some((false, None, None, *attempt)),
            _ => None,
        };
        if let Some((dlc, uri, handled, attempt)) = d {
            self.details
                .lock()
                .unwrap()
                .push((event.event_type(), dlc, uri, handled, attempt));
        }
    }
}

/// The details of a failure event: its type, whether a dead letter channel, its uri, whether
/// handled, the redelivery attempt.
type Detail = (EventType, bool, Option<String>, Option<bool>, i32);

type Details = Arc<Mutex<Vec<Detail>>>;

async fn failure_context(routes: impl RouteBuilder) -> (ContextTestSupport, Events, Details) {
    let events: Events = Arc::default();
    let details: Details = Arc::default();
    let t = context_with(
        FailureRecorder {
            events: events.clone(),
            details: details.clone(),
        },
        routes,
    )
    .await;
    (t, events, details)
}

const STARTED_ONE: [EventType; 9] = [
    T::CamelContextInitializing,
    T::CamelContextInitialized,
    T::CamelContextStarting,
    T::CamelContextRoutesStarting,
    T::RouteAdded,
    T::RouteStarting,
    T::RouteStarted,
    T::CamelContextRoutesStarted,
    T::CamelContextStarted,
];

fn failure_handled_expected() -> Vec<EventType> {
    let mut expected = STARTED_ONE.to_vec();
    expected.extend([
        T::ExchangeSending,
        T::ExchangeCreated,
        T::ExchangeFailureHandling,
        T::ExchangeSending,
        T::ExchangeSent,
        T::ExchangeFailureHandled,
        T::ExchangeCompleted,
        T::ExchangeSent,
    ]);
    expected
}

/// Port of `EventNotifierFailureHandledEventsTest.testExchangeDeadLetterChannel`.
#[tokio::test]
async fn test_exchange_dead_letter_channel() {
    let (t, events, details) = failure_context(|r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:dead"));
        r.add(from("direct:start").throw_exception(CamelError::illegal_argument("Damn")));
    })
    .await;
    t.mock("mock:dead").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.mock("mock:dead").assert_is_satisfied().await;
    assert_eq!(failure_handled_expected(), failures(&events));
    let d = details.lock().unwrap().clone();
    assert_eq!(
        (
            T::ExchangeFailureHandling,
            true,
            Some("mock:dead".to_string()),
            None,
            0
        ),
        d[0],
        "should be DLC"
    );
    assert_eq!(
        (
            T::ExchangeFailureHandled,
            true,
            Some("mock:dead".to_string()),
            Some(true),
            0
        ),
        d[1],
        "should be DLC and marked as failure handled"
    );
    let recorded = events.lock().unwrap().clone();
    assert_eq!(Some("mock://dead"), recorded[12].endpoint.as_deref());
    assert_eq!(Some("direct://start"), recorded[16].endpoint.as_deref());
}

/// Port of `EventNotifierFailureHandledEventsTest.testExchangeOnException`.
#[tokio::test]
async fn test_exchange_on_exception() {
    let (t, events, details) = failure_context(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .handled(true)
                .to("mock:dead"),
        );
        r.add(from("direct:start").throw_exception(CamelError::illegal_argument("Damn")));
    })
    .await;
    t.mock("mock:dead").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.mock("mock:dead").assert_is_satisfied().await;
    assert_eq!(failure_handled_expected(), failures(&events));
    let d = details.lock().unwrap().clone();
    assert!(!d[0].1, "should NOT be DLC");
    assert_eq!(
        (T::ExchangeFailureHandled, false, Some(true)),
        (d[1].0, d[1].1, d[1].3)
    );
}

/// Port of `EventNotifierFailureHandledEventsTest.testExchangeDoTryDoCatch`.
#[tokio::test]
async fn test_exchange_do_try_do_catch() {
    let (t, events, details) = failure_context(|r: &mut Routes| {
        r.add(from("direct:start").do_try(|b| {
            b.throw_exception(CamelError::illegal_argument("Damn"))
                .do_catch(ErrorKind::IllegalArgument, |c| c.to("mock:dead"))
        }));
    })
    .await;
    t.mock("mock:dead").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.mock("mock:dead").assert_is_satisfied().await;
    assert_eq!(failure_handled_expected(), failures(&events));
    let d = details.lock().unwrap().clone();
    assert!(!d[0].1, "should NOT be DLC");
    // not handled as it was continued instead
    assert_eq!(
        (T::ExchangeFailureHandled, false, Some(false)),
        (d[1].0, d[1].1, d[1].3)
    );
}

/// Port of `EventNotifierRedeliveryEventsTest` (the async delayed redelivery is the same in
/// Rust: the delays never block a thread).
#[tokio::test]
async fn test_exchange_redelivery() {
    for delay in [0, 10] {
        let events: Events = Arc::default();
        let details: Details = Arc::default();
        let e = events.clone();
        let d = details.clone();
        struct Filtered(FailureRecorder);
        impl EventNotifier for Filtered {
            fn notify(&self, event: &CamelEvent<'_>) {
                self.0.notify(event);
            }
            fn is_enabled(&self, event: &CamelEvent<'_>) -> bool {
                !event.is_context_event() && !event.is_route_event()
            }
        }
        let t = context_with(
            Filtered(FailureRecorder {
                events: e,
                details: d,
            }),
            move |r: &mut Routes| {
                r.error_handler(
                    dead_letter_channel("mock:dead")
                        .maximum_redeliveries(4)
                        .redelivery_delay(delay),
                );
                r.add(from("direct:start").throw_exception(CamelError::illegal_argument("Damn")));
            },
        )
        .await;
        t.mock("mock:dead").expected_message_count(1);
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.mock("mock:dead").assert_is_satisfied().await;
        assert_eq!(
            vec![
                T::ExchangeSending,
                T::ExchangeCreated,
                T::ExchangeRedelivery,
                T::ExchangeRedelivery,
                T::ExchangeRedelivery,
                T::ExchangeRedelivery,
                T::ExchangeFailureHandling,
                T::ExchangeSending,
                T::ExchangeSent,
                T::ExchangeFailureHandled,
                T::ExchangeCompleted,
                T::ExchangeSent,
            ],
            types(&events)
        );
        let attempts: Vec<i32> = details.lock().unwrap()[..4].iter().map(|d| d.4).collect();
        assert_eq!(vec![1, 2, 3, 4], attempts);
    }
}
