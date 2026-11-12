//! Tests of the load balancer EIP (Java `processor/loadbalancer` tests: round robin, random,
//! sticky, topic, weighted and failover).

use crate::support::ContextTestSupport;
use camel_core::api::error::ErrorType;
use camel_core::api::processor::processor_fn;
use camel_core::prelude::*;
use std::sync::Arc;

fn routes(balancer: LoadBalancerDefinition) -> impl RouteBuilder {
    move |r: &mut Routes| {
        r.add(from("direct:start").load_balance(balancer.clone(), |b| {
            b.to("mock:x").to("mock:y").to("mock:z")
        }));
    }
}

async fn send(t: &ContextTestSupport, bodies: &[&str]) {
    for b in bodies {
        t.template.send_body("direct:start", *b).await.unwrap();
    }
}

#[tokio::test]
async fn test_round_robin() {
    let t = ContextTestSupport::with_routes(routes(LoadBalancerDefinition::RoundRobin)).await;
    t.mock("mock:x").expected_bodies_received(["a", "d"]);
    t.mock("mock:y").expected_bodies_received(["b", "e"]);
    t.mock("mock:z").expected_bodies_received(["c"]);
    send(&t, &["a", "b", "c", "d", "e"]).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_random() {
    let t = ContextTestSupport::with_routes(routes(LoadBalancerDefinition::Random)).await;
    send(&t, &["a"; 30]).await;
    let total: usize = ["mock:x", "mock:y", "mock:z"]
        .iter()
        .map(|m| t.mock(m).received_counter())
        .sum();
    assert_eq!(30, total);
}

#[tokio::test]
async fn test_sticky() {
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .load_balance(LoadBalancerDefinition::Sticky(header("foo").into()), |b| {
                    b.to("mock:x").to("mock:y").to("mock:z")
                }),
        );
    })
    .await;
    t.mock("mock:x").expected_bodies_received(["a", "c", "e"]);
    t.mock("mock:y").expected_bodies_received(["b", "d"]);
    for (body, key) in [("a", "1"), ("b", "2"), ("c", "1"), ("d", "2"), ("e", "1")] {
        t.template
            .send_body_and_header("direct:start", body, "foo", key)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_topic() {
    let t = ContextTestSupport::with_routes(routes(LoadBalancerDefinition::Topic)).await;
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["a", "b"]);
    }
    send(&t, &["a", "b"]).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_weighted_round_robin() {
    let t =
        ContextTestSupport::with_routes(routes(LoadBalancerDefinition::weighted(true, "4,2,1")))
            .await;
    t.mock("mock:x").expected_message_count(5);
    t.mock("mock:y").expected_message_count(2);
    t.mock("mock:z").expected_message_count(1);
    send(&t, &["a"; 8]).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_weighted_random() {
    let t =
        ContextTestSupport::with_routes(routes(LoadBalancerDefinition::weighted(false, "4,2,1")))
            .await;
    // a full round of 7 messages follows the ratios exactly
    t.mock("mock:x").expected_message_count(4);
    t.mock("mock:y").expected_message_count(2);
    t.mock("mock:z").expected_message_count(1);
    send(&t, &["a"; 7]).await;
    t.assert_mock_endpoints_satisfied().await;
    let bad = ContextTestSupport::new()
        .context
        .add_routes(routes(LoadBalancerDefinition::weighted(false, "4,2")))
        .await;
    assert!(bad.is_err(), "the ratios must match the steps");
}

fn failing() -> Arc<dyn Processor> {
    Arc::new(processor_fn(|_| {
        Err(CamelError::illegal_argument("Forced"))
    }))
}

#[tokio::test]
async fn test_failover() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .load_balance(LoadBalancerDefinition::failover(), |b| {
                    b.pipeline(|p| p.to("mock:x").process(failing()))
                        .pipeline(|p| p.to("mock:y").process(failing()))
                        .to("mock:z")
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:x").expected_message_count(1);
    t.mock("mock:y").expected_message_count(1);
    t.mock("mock:z").expected_bodies_received(["Hello"]);
    t.mock("mock:result").expected_bodies_received(["Hello"]);
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_failover_only_listed_exceptions_and_attempts() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:io").load_balance(
            LoadBalancerDefinition::Failover {
                exceptions: vec![ErrorType::Kind(ErrorKind::Io)],
                round_robin: false,
                sticky: false,
                maximum_failover_attempts: -1,
            },
            |b| {
                b.pipeline(|p| p.to("mock:x").process(failing()))
                    .to("mock:y")
            },
        ));
        r.add(from("direct:attempts").load_balance(
            LoadBalancerDefinition::Failover {
                exceptions: vec![],
                round_robin: false,
                sticky: false,
                maximum_failover_attempts: 1,
            },
            |b| {
                b.pipeline(|p| p.to("mock:a").process(failing()))
                    .pipeline(|p| p.to("mock:b").process(failing()))
                    .to("mock:c")
            },
        ));
    })
    .await;
    // an IllegalArgumentException is not an IOException: no failover
    t.mock("mock:x").expected_message_count(1);
    t.mock("mock:y").expected_message_count(0);
    assert!(t.template.send_body("direct:io", "Hello").await.is_err());
    // one failover attempt, then the error
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:b").expected_message_count(1);
    t.mock("mock:c").expected_message_count(0);
    assert!(t
        .template
        .send_body("direct:attempts", "Hello")
        .await
        .is_err());
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        "LoadBalanceType[RoundRobinLoadBalancer, [to[mock:x], to[mock:y], to[mock:z]]]",
        from("direct:x")
            .load_balance(LoadBalancerDefinition::RoundRobin, |b| {
                b.to("mock:x").to("mock:y").to("mock:z")
            })
            .build()
            .steps[0]
            .to_string()
    );
}
