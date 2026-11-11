//! Port of `OnExceptionUseOriginalMessageTest` (`testOnExceptionStreamReset` is not
//! applicable: bodies are not streams that need a reset).

use crate::support::caught_is;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

const HELLO_WORLD: &str = "Hello World";

#[tokio::test]
async fn test_on_exception_error() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .use_original_message()
                .handled(true)
                .to("seda:test"),
        );
        r.add(
            from("direct:a")
                .set_exchange_pattern(ExchangePattern::InOut)
                .process_fn(|e| {
                    // Java MyProcessor
                    if e.in_message().body() != HELLO_WORLD {
                        e.in_message_mut().set_body(HELLO_WORLD);
                    }
                    // set the out message
                    e.message_mut().set_body("Error body");
                    Err(CamelError::illegal_argument("Get a wrong message"))
                }),
        );
        r.add(from("seda:test").to("mock:middle").to("mock:end"));
    })
    .await;
    for uri in ["mock:middle", "mock:end"] {
        let mock = t.mock(uri);
        mock.expected_bodies_received([HELLO_WORLD]);
        mock.message_matches(0, caught_is(ErrorKind::IllegalArgument));
    }
    t.template.send_body("direct:a", HELLO_WORLD).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
