//! Port of `MulticastTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

/// Java `AppendingProcessor`: appends `+output` to the body.
fn appending(e: &mut Exchange) -> CamelResult<()> {
    let body = e.message().body().to_string();
    e.message_mut().set_body(format!("{body}+output"));
    Ok(())
}

#[tokio::test]
async fn test_sending_a_message_using_multicast_receives_its_own_exchange() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").multicast(|m| m.to("direct:x").to("direct:y").to("direct:z")));
        r.add(from("direct:x").process_fn(appending).to("mock:x"));
        r.add(from("direct:y").process_fn(appending).to("mock:y"));
        r.add(from("direct:z").process_fn(appending).to("mock:z"));
    })
    .await;
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["input+output"]);
    }
    let mut e = t
        .template
        .create_exchange("direct:a", ExchangePattern::InOnly)
        .unwrap();
    e.message_mut().set_body("input");
    e.message_mut().set_header("foo", "bar");
    t.template.send("direct:a", e).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
