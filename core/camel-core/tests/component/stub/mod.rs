//! Ports of the stub component tests: `StubTest`, `StubConsumerTest`, `StubInOutTest` and
//! `StubInOnlyTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_stub() {
    for consumer in [false, true] {
        for in_out in [false, true] {
            let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
                r.add(
                    from("direct:InOnly")
                        .set_exchange_pattern(ExchangePattern::InOnly)
                        .to("stub:foo")
                        .to("mock:result"),
                );
                r.add(
                    from("direct:InOut")
                        .set_exchange_pattern(ExchangePattern::InOut)
                        .to("stub:foo")
                        .to("mock:result"),
                );
                if consumer {
                    r.add(from("stub:foo").transform(constant("Bye World")));
                }
            })
            .await;
            let expected = if consumer && in_out {
                "Bye World"
            } else {
                "Hello World"
            };
            t.mock("mock:result").expected_bodies_received([expected]);
            let uri = if in_out {
                "direct:InOut"
            } else {
                "direct:InOnly"
            };
            t.template.send_body(uri, "Hello World").await.unwrap();
            t.assert_mock_endpoints_satisfied().await;
        }
    }
}

/// Any uri and parameters are stubbed.
#[tokio::test]
async fn test_stub_any_uri() {
    for in_out in [false, true] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(
                from("direct:start").to("stub:smtp://some.server.com?something=bar&whatnot=cheese"),
            );
            r.add(
                from("stub:smtp://some.server.com?something=bar&whatnot=cheese").to("mock:result"),
            );
        })
        .await;
        t.mock("mock:result")
            .expected_bodies_received(["Hello World"]);
        if in_out {
            t.template
                .request_body("direct:start", "Hello World")
                .await
                .unwrap();
        } else {
            t.template
                .send_body("direct:start", "Hello World")
                .await
                .unwrap();
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}
