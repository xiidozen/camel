//! Port of `ChoiceNoOtherwiseTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

async fn run(routes: impl RouteBuilder) {
    let t = ContextTestSupport::new();
    t.context.add_routes(routes).await.unwrap();
    t.context.start().await.unwrap();
    let x = t.mock("mock:x");
    let end = t.mock("mock:end");
    x.expected_bodies_received(["a"]);
    end.expected_bodies_received(["a", "b"]);
    t.template
        .send_body_and_header("direct:start", "a", "foo", "bar")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "b", "foo", "cheese")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_no_otherwise() {
    run(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .choice(|c| c.when(simple("${header.foo} == 'bar'"), |w| w.to("mock:x")))
                .to("mock:end"),
        );
    })
    .await;
}

// testNoOtherwiseTwo is the same route written with when(simple(..)) in Java

#[tokio::test]
async fn test_empty_otherwise() {
    run(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .choice(|c| {
                    c.when(simple("${header.foo} == 'bar'"), |w| w.to("mock:x"))
                        .otherwise(|o| o)
                })
                .to("mock:end"),
        );
    })
    .await;
}
