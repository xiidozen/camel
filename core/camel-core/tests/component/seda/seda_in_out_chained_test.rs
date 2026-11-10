//! Port of `SedaInOutChainedTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_in_out_seda_chained() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:a")
                .to("mock:a")
                .transform(simple("${body}-a"))
                .to("seda:b"),
        );
        r.add(
            from("seda:b")
                .to("mock:b")
                .transform(simple("${body}-b"))
                .to("seda:c"),
        );
        r.add(from("seda:c").to("mock:c").transform(simple("${body}-c")));
    })
    .await;
    t.mock("mock:a").expected_bodies_received(["start"]);
    t.mock("mock:b").expected_bodies_received(["start-a"]);
    t.mock("mock:c").expected_bodies_received(["start-a-b"]);
    let reply = t.template.request_body("seda:a", "start").await.unwrap();
    assert_eq!("start-a-b-c", reply.to_string());
    t.assert_mock_endpoints_satisfied().await;
}
