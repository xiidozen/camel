//! Port of `OnExceptionHandledTest`.

use super::caught_is;
use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::EXCEPTION_CAUGHT;
use camel_core::prelude::*;

#[tokio::test]
async fn test_handled() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .handled(true)
                .to("log:foo?showAll=true")
                .to("mock:handled"),
        );
        r.add(from("direct:start").throw_exception(CamelError::illegal_argument("Forced")));
    })
    .await;
    let mock = t.mock("mock:handled");
    mock.expected_bodies_received(["Hello World"]);
    mock.message_matches(0, exchange_property(EXCEPTION_CAUGHT).is_not_null());
    mock.message_matches(0, caught_is(ErrorKind::IllegalArgument));
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
