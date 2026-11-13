//! Ports of `CBRHeaderPredicateTest`, `CBRWithMulticastTest`, `CBRWithRecipientListTest`,
//! `CBRWithWireTapTest` and `CBRWithLoadBalancerTest`.

use crate::support::ContextTestSupport;
use camel_bean::MethodBean;
use camel_core::prelude::*;

/// Expects the counts of the mocks.
fn expect(t: &ContextTestSupport, counts: &[(&str, i32)]) {
    for (m, n) in counts {
        let mock = t.mock(m);
        mock.reset();
        mock.expected_message_count(*n);
    }
}

#[tokio::test]
async fn test_cbr_header_predicate() {
    // Java MyCBRBean.checkHeader(Exchange)
    let bean = MethodBean::new("MyCBRBean").method("checkHeader", |inv| {
        let foo = inv
            .exchange()
            .message()
            .header("foo")
            .map(|v| v.to_string());
        Ok(Value::Bool(foo.as_deref() == Some("bar")))
    });
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("cbrBean", bean.into_value())
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").choice(|c| {
                c.when(method("cbrBean", "checkHeader"), |w| w.to("mock:foo"))
                    .otherwise(|o| o.to("mock:bar"))
            }));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:foo").expected_bodies_received(["Hello Foo"]);
    t.mock("mock:bar").expected_bodies_received(["Hello Bar"]);
    t.template
        .send_body_and_header("direct:start", "Hello Foo", "foo", "bar")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Hello Bar", "foo", "other")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_cbr_with_multicast_and_load_balancer() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:multicast").choice(|c| {
            c.when(body().contains("Camel"), |w| {
                w.multicast(|m| m.to("mock:foo").to("mock:bar"))
            })
            .otherwise(|o| o.to("mock:result"))
        }));
        r.add(from("direct:balance").choice(|c| {
            c.when(body().contains("Camel"), |w| {
                w.load_balance(LoadBalancerDefinition::RoundRobin, |b| {
                    b.to("mock:foo").to("mock:bar")
                })
            })
            .otherwise(|o| o.to("mock:result"))
        }));
    })
    .await;
    // CBRWithMulticastTest
    for (bodies, foo_bar, result) in [
        (&["Camel rules"][..], 1, 0),
        (&["Camel rules", "Camel rocks"][..], 2, 0),
        (&["Hello World"][..], 0, 1),
    ] {
        expect(
            &t,
            &[
                ("mock:foo", foo_bar),
                ("mock:bar", foo_bar),
                ("mock:result", result),
            ],
        );
        for b in bodies {
            t.template.send_body("direct:multicast", *b).await.unwrap();
        }
        t.assert_mock_endpoints_satisfied().await;
    }
    // CBRWithLoadBalancerTest
    for (bodies, foo, bar, result) in [
        (&["Camel rules"][..], 1, 0, 0),
        (&["Camel rules", "Camel rocks"][..], 1, 1, 0),
        (&["Hello World"][..], 0, 0, 1),
    ] {
        expect(
            &t,
            &[
                ("mock:foo", foo),
                ("mock:bar", bar),
                ("mock:result", result),
            ],
        );
        for b in bodies {
            t.template.send_body("direct:balance", *b).await.unwrap();
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_cbr_with_recipient_list_and_wire_tap() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:recipients").choice(|c| {
            c.when(body().contains("Camel"), |w| {
                w.recipient_list(header("foo"))
            })
            .when(body().contains("Donkey"), |w| {
                w.recipient_list(header("bar"))
            })
            .otherwise(|o| o.to("mock:result"))
        }));
        r.add(from("direct:wireTap").choice(|c| {
            c.when(body().contains("Camel"), |w| w.wire_tap("mock:camel"))
                .when(body().contains("Donkey"), |w| w.wire_tap("mock:donkey"))
                .otherwise(|o| o.to("mock:other"))
        }));
    })
    .await;
    // CBRWithRecipientListTest
    for (body, header, value, counts) in [
        ("Camel rules", "foo", "mock:foo", [1, 0, 0]),
        ("Donkey Kong", "bar", "mock:bar", [0, 1, 0]),
        ("Hello World", "none", "", [0, 0, 1]),
    ] {
        expect(
            &t,
            &[
                ("mock:foo", counts[0]),
                ("mock:bar", counts[1]),
                ("mock:result", counts[2]),
            ],
        );
        t.template
            .send_body_and_header("direct:recipients", body, header, value)
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
    // CBRWithWireTapTest
    for (body, counts) in [
        ("Camel rules", [1, 0, 0]),
        ("Donkey kong", [0, 1, 0]),
        ("Hello World", [0, 0, 1]),
    ] {
        expect(
            &t,
            &[
                ("mock:camel", counts[0]),
                ("mock:donkey", counts[1]),
                ("mock:other", counts[2]),
            ],
        );
        t.template.send_body("direct:wireTap", body).await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}
