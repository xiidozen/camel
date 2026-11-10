//! Port of `SedaInOutWithErrorTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_in_out_with_error() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to("seda:foo"));
        r.add(
            from("seda:foo")
                .transform(constant("Bye World"))
                .throw_exception(CamelError::illegal_argument("Damn I cannot do this"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(0);
    let e = t
        .template
        .request_body("direct:start", "Hello World")
        .await
        .unwrap_err();
    assert!(e.is(ErrorKind::CamelExecution));
    let cause = std::error::Error::source(&e)
        .and_then(|c| c.downcast_ref::<CamelError>())
        .unwrap();
    assert!(cause.is(ErrorKind::IllegalArgument));
    assert_eq!(Some("Damn I cannot do this"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}
