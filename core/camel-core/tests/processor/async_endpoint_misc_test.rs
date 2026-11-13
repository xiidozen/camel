//! Ports of more async endpoint tests of `processor/async` with the `async:` test component:
//! `AsyncEndpointTest`, `AsyncInOnlyEndpointTest`, `AsyncTwoEndpointTest`,
//! `AsyncEndpointTransformTest`, `AsyncEndpointSplitTest`, `AsyncEndpointSamplingTest`,
//! `AsyncEndpointSedaTest`, `AsyncEndpointSedaInOnlyTest`,
//! `AsyncEndpointRoundRobinLoadBalanceTest`, `AsyncEndpointDualTopicLoadBalanceTest`,
//! `AsyncEndpointMulticastSynchronousTest`, `AsyncEndpointFailOverLoadBalanceMixed2Test`,
//! `AsyncEndpointFailOverLoadBalanceMixed3Test`, `AsyncEndpointFailureProcessorTest`,
//! `AsyncEndpointFailureProcessorContinueTest` and the `AsyncEndpointTryCatchFinally*Test`s.
//! The Java assertions on the threads have no equivalent on a tokio runtime (and the
//! synchronous multicast is the multicast).

use crate::support::my_async::async_context;
use crate::support::ContextTestSupport;
use camel_core::api::error::ErrorKind;
use camel_core::prelude::*;
use std::sync::{Arc, Mutex};

fn noop(_: &mut Exchange) -> CamelResult<()> {
    Ok(())
}

/// The expected bodies of the mocks (none: no message).
type Expected<'a> = &'a [(&'a str, &'a [&'a str])];

/// Expects the bodies of the mocks (reset first).
fn expect(t: &ContextTestSupport, expected: Expected<'_>) {
    for (m, bodies) in expected {
        let mock = t.mock(m);
        mock.reset();
        if bodies.is_empty() {
            mock.expected_message_count(0);
        } else {
            mock.expected_bodies_received(bodies.iter().copied());
        }
    }
}

async fn request(t: &ContextTestSupport, uri: &str, body: &str) -> Value {
    t.template.request_body(uri, body).await.unwrap()
}

#[tokio::test]
async fn test_async_endpoints() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:one")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:two")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:hi:camel")
                .process_fn(noop)
                .to("log:middle")
                .to("mock:middle")
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:transform")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:hi:camel")
                .transform(simple("Hello World"))
                .to("async:bye:camel")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:sample")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .sample()
                .to("async:bye:camel")
                .to("log:after")
                .to("mock:after")
                .process_fn(noop)
                .to("mock:result"),
        );
        r.add(
            from("direct:seda")
                .to("mock:before")
                .process_fn(noop)
                .to("async:bye:camel")
                .process_fn(noop)
                .to("seda:foo"),
        );
        r.add(from("seda:foo").to("mock:after").to("mock:result"));
    })
    .await;
    let standard: Expected<'_> = &[
        ("mock:before", &["Hello Camel"]),
        ("mock:after", &["Bye Camel"]),
        ("mock:result", &["Bye Camel"]),
    ];
    // AsyncEndpointTest, AsyncEndpointTransformTest, AsyncEndpointSamplingTest, AsyncEndpointSedaTest
    for uri in [
        "direct:one",
        "direct:transform",
        "direct:sample",
        "direct:seda",
    ] {
        expect(&t, standard);
        assert_eq!(
            Value::from("Bye Camel"),
            request(&t, uri, "Hello Camel").await,
            "{uri}"
        );
        t.assert_mock_endpoints_satisfied().await;
    }
    // AsyncInOnlyEndpointTest
    expect(&t, standard);
    t.template
        .send_body("direct:one", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    // AsyncTwoEndpointTest
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel"]),
            ("mock:middle", &["Hi Camel"]),
            ("mock:after", &["Bye Camel"]),
            ("mock:result", &["Bye Camel"]),
        ],
    );
    assert_eq!(
        Value::from("Bye Camel"),
        request(&t, "direct:two", "Hello Camel").await
    );
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_async_endpoint_split_and_seda_in_only() {
    let route = Arc::new(Mutex::new(String::new()));
    let r2 = route.clone();
    let t = async_context(move |r: &mut Routes| {
        r.add(
            from("direct:split")
                .split(body(), |s| {
                    s.to("mock:before")
                        .to("log:before")
                        .process_fn(noop)
                        .to("async:bye:camel")
                        .process_fn(noop)
                        .to("log:after")
                        .to("mock:after")
                })
                .to("mock:result"),
        );
        r.add(
            from("direct:inOnly")
                .to("mock:before")
                .process_fn(noop)
                .to("async:bye:camel")
                .process_fn(noop)
                .to("seda:bar"),
        );
        let route = r2.clone();
        r.add(
            from("seda:bar")
                .to("mock:after")
                .to("log:after")
                .delay(constant(1000))
                .process_fn(move |_| {
                    route.lock().unwrap().push('B');
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    // AsyncEndpointSplitTest
    expect(
        &t,
        &[
            ("mock:before", &["A", "B"]),
            ("mock:after", &["Bye Camel", "Bye Camel"]),
            ("mock:result", &["A,B"]),
        ],
    );
    assert_eq!(Value::from("A,B"), request(&t, "direct:split", "A,B").await);
    t.assert_mock_endpoints_satisfied().await;
    // AsyncEndpointSedaInOnlyTest
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel"]),
            ("mock:after", &["Bye Camel"]),
            ("mock:result", &["Bye Camel"]),
        ],
    );
    t.template
        .send_body("direct:inOnly", "Hello Camel")
        .await
        .unwrap();
    route.lock().unwrap().push('A');
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!("AB", route.lock().unwrap().as_str());
}

#[tokio::test]
async fn test_async_endpoint_load_balancers_and_multicast() {
    let t = async_context(|r: &mut Routes| {
        r.add(
            from("direct:roundRobin")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .load_balance(LoadBalancerDefinition::RoundRobin, |b| {
                    b.to("async:bye:camel").to("async:bye:world")
                })
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:topic")
                .to("mock:before")
                .to("log:before")
                .load_balance(LoadBalancerDefinition::Topic, |b| {
                    b.to("async:bye:camel").to("async:bye:world")
                })
                .to("log:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:multicast")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .multicast(|m| m.to("async:hi:moon").to("direct:foo"))
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(from("direct:foo").set_body(constant("Bye Camel")));
        r.add(
            from("direct:mixed2")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .load_balance(LoadBalancerDefinition::failover(), |b| {
                    b.to("direct:fail").to("async:bye:world")
                })
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:mixed3")
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .load_balance(LoadBalancerDefinition::failover(), |b| {
                    b.to("async:bye:camel?failFirstAttempts=5").to("direct:ok")
                })
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .to("mock:result"),
        );
        r.add(
            from("direct:fail")
                .to("log:fail")
                .to("mock:fail")
                .throw_exception(CamelError::illegal_argument("Damn")),
        );
        r.add(
            from("direct:ok")
                .to("log:pok")
                .to("mock:ok")
                .transform(constant("Bye World")),
        );
    })
    .await;
    // AsyncEndpointRoundRobinLoadBalanceTest
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel", "Hello World"]),
            ("mock:after", &["Bye Camel", "Bye World"]),
            ("mock:result", &["Bye Camel", "Bye World"]),
        ],
    );
    assert_eq!(
        Value::from("Bye Camel"),
        request(&t, "direct:roundRobin", "Hello Camel").await
    );
    assert_eq!(
        Value::from("Bye World"),
        request(&t, "direct:roundRobin", "Hello World").await
    );
    t.assert_mock_endpoints_satisfied().await;
    // AsyncEndpointDualTopicLoadBalanceTest
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel"]),
            ("mock:result", &["Hello Camel"]),
        ],
    );
    t.template
        .send_body("direct:topic", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    // AsyncEndpointMulticastSynchronousTest
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel"]),
            ("mock:after", &["Bye Camel"]),
            ("mock:result", &["Bye Camel"]),
        ],
    );
    assert_eq!(
        Value::from("Bye Camel"),
        request(&t, "direct:multicast", "Hello Camel").await
    );
    t.assert_mock_endpoints_satisfied().await;
    // AsyncEndpointFailOverLoadBalanceMixed2Test and Mixed3Test
    for (uri, extra) in [("direct:mixed2", "mock:fail"), ("direct:mixed3", "mock:ok")] {
        expect(
            &t,
            &[
                ("mock:before", &["Hello Camel"]),
                (extra, &["Hello Camel"]),
                ("mock:after", &["Bye World"]),
                ("mock:result", &["Bye World"]),
            ],
        );
        assert_eq!(
            Value::from("Bye World"),
            request(&t, uri, "Hello Camel").await,
            "{uri}"
        );
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_async_endpoint_failure_processors() {
    // AsyncEndpointFailureProcessorTest
    let t = async_context(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .handled(true)
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:MyFailureHandler")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after")
                .transform(constant("Bye Camel")),
        );
        r.add(
            from("direct:start")
                .throw_exception(CamelError::illegal_argument("Damn"))
                .to("mock:result"),
        );
    })
    .await;
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel"]),
            ("mock:after", &["MyFailureHandler"]),
            ("mock:result", &[]),
        ],
    );
    assert_eq!(
        Value::from("Bye Camel"),
        request(&t, "direct:start", "Hello Camel").await
    );
    t.assert_mock_endpoints_satisfied().await;
    // AsyncEndpointFailureProcessorContinueTest
    let t = async_context(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .continued(true)
                .to("mock:before")
                .to("log:before")
                .process_fn(noop)
                .to("async:MyFailureHandler")
                .process_fn(noop)
                .to("log:after")
                .to("mock:after"),
        );
        r.add(
            from("direct:start")
                .throw_exception(CamelError::illegal_argument("Damn"))
                .to("mock:result")
                .transform(constant("Bye Camel")),
        );
    })
    .await;
    expect(
        &t,
        &[
            ("mock:before", &["Hello Camel"]),
            ("mock:after", &["MyFailureHandler"]),
            ("mock:result", &["MyFailureHandler"]),
        ],
    );
    assert_eq!(
        Value::from("Bye Camel"),
        request(&t, "direct:start", "Hello Camel").await
    );
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_async_endpoint_try_catch_finally() {
    let t = async_context(|r: &mut Routes| {
        // AsyncEndpointTryCatchFinallyTest
        r.add(
            from("direct:one")
                .to("mock:before")
                .to("log:before")
                .do_try(|t| {
                    t.process_fn(noop)
                        .to("async:bye:camel?failFirstAttempts=1")
                        .do_catch(ErrorKind::General, |c| c.process_fn(noop))
                        .do_finally(|f| {
                            f.to("log:after")
                                .to("mock:after")
                                .transform(constant("Bye World"))
                        })
                })
                .to("mock:result"),
        );
        // AsyncEndpointTryCatchFinally2Test
        r.add(
            from("direct:two")
                .to("mock:before")
                .to("log:before")
                .do_try(|t| {
                    t.process_fn(noop)
                        .to("async:bye:camel?failFirstAttempts=1")
                        .do_catch(ErrorKind::General, |c| {
                            c.to("log:catch")
                                .to("mock:catch")
                                .process_fn(noop)
                                .to("async:bye:world")
                        })
                        .do_finally(|f| {
                            f.to("log:finally")
                                .process_fn(noop)
                                .to("log:after")
                                .to("mock:after")
                        })
                })
                .to("mock:result"),
        );
        // AsyncEndpointTryCatchFinally3Test
        r.add(
            from("direct:three")
                .to("mock:before")
                .to("log:before")
                .do_try(|t| {
                    t.process_fn(noop)
                        .to("async:bye:camel?failFirstAttempts=1")
                        .do_catch(ErrorKind::General, |c| {
                            c.to("log:catch")
                                .to("mock:catch")
                                .process_fn(noop)
                                .to("async:bye:world")
                        })
                        .do_finally(|f| {
                            f.process_fn(noop)
                                .to("log:after")
                                .to("mock:after")
                                .to("async:bye:camel")
                        })
                })
                .process_fn(noop)
                .to("log:result")
                .to("mock:result"),
        );
        // AsyncEndpointTryCatchFinally4Test
        r.add(
            from("direct:four")
                .do_try(|t| {
                    t.to("mock:try")
                        .do_catch(ErrorKind::IllegalArgument, |c| {
                            c.to("mock:catch").to("async:bye:camel")
                        })
                        .do_finally(|f| f.to("mock:finally").to("async:bye:world"))
                })
                .to("mock:result"),
        );
        // AsyncEndpointTryCatchFinally5Test
        r.add(
            from("direct:five")
                .do_try(|t| {
                    t.to("log:try")
                        .to("mock:try")
                        .throw_exception(CamelError::illegal_argument("Damn"))
                        .do_catch(ErrorKind::IllegalArgument, |c| {
                            c.to("mock:catch").to("log:catch").to("async:bye:camel")
                        })
                        .do_finally(|f| {
                            f.to("mock:finally").to("log:finally").to("async:bye:world")
                        })
                })
                .to("mock:result")
                .to("log:result"),
        );
    })
    .await;
    let cases: [(&str, Expected<'_>, &str); 5] = [
        (
            "direct:one",
            &[
                ("mock:before", &["Hello Camel"]),
                ("mock:after", &["Hello Camel"]),
                ("mock:result", &["Bye World"]),
            ],
            "Bye World",
        ),
        (
            "direct:two",
            &[
                ("mock:before", &["Hello Camel"]),
                ("mock:catch", &["Hello Camel"]),
                ("mock:after", &["Bye World"]),
                ("mock:result", &["Bye World"]),
            ],
            "Bye World",
        ),
        (
            "direct:three",
            &[
                ("mock:before", &["Hello Camel"]),
                ("mock:catch", &["Hello Camel"]),
                ("mock:after", &["Bye World"]),
                ("mock:result", &["Bye Camel"]),
            ],
            "Bye Camel",
        ),
        (
            "direct:four",
            &[
                ("mock:try", &["Hello Camel"]),
                ("mock:catch", &[]),
                ("mock:finally", &["Hello Camel"]),
                ("mock:result", &["Bye World"]),
            ],
            "Bye World",
        ),
        (
            "direct:five",
            &[
                ("mock:try", &["Hello Camel"]),
                ("mock:catch", &["Hello Camel"]),
                ("mock:finally", &["Bye Camel"]),
                ("mock:result", &["Bye World"]),
            ],
            "Bye World",
        ),
    ];
    for (uri, expected, reply) in cases {
        for m in [
            "mock:before",
            "mock:after",
            "mock:catch",
            "mock:try",
            "mock:finally",
            "mock:result",
        ] {
            t.mock(m).reset();
        }
        expect(&t, expected);
        assert_eq!(
            Value::from(reply),
            request(&t, uri, "Hello Camel").await,
            "{uri}"
        );
        t.assert_mock_endpoints_satisfied().await;
    }
}
