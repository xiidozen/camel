//! Port of `OnExceptionContinueTest`.

use crate::support::caught_is;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_continued() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(on_exception(ErrorKind::IllegalArgument).continued(true));
        r.add(
            from("direct:start")
                .to("mock:start")
                .throw_exception(CamelError::illegal_argument("Forced"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:start").expected_message_count(1);
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.message_matches(0, caught_is(ErrorKind::IllegalArgument));
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
