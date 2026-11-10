//! Port of `FilterTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start").filter(header("foo").is_equal_to("bar"), |f| f.to("mock:result")),
        );
    })
    .await
}

#[tokio::test]
async fn test_send_matching_message() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    t.template
        .send_body_and_header("direct:start", "<matched/>", "foo", "bar")
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_send_not_matching_message() {
    let t = setup().await;
    let result = t.mock("mock:result");
    result.expected_message_count(0);
    t.template
        .send_body_and_header(
            "direct:start",
            "<notMatched/>",
            "foo",
            "notMatchedHeaderValue",
        )
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}
