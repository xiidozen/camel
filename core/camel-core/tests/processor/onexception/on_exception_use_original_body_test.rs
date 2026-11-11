//! Port of `OnExceptionUseOriginalBodyTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

/// Java `MyThrowProcessor`.
fn my_throw_processor(camel_exception: bool) -> impl Fn(&mut Exchange) -> CamelResult<()> {
    move |e: &mut Exchange| {
        assert_eq!("Hello World", e.in_message().body().to_string());
        if camel_exception {
            Err(CamelError::new(ErrorKind::CamelExchange, "I cannot do it"))
        } else {
            Err(CamelError::illegal_argument("Forced"))
        }
    }
}

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        // will not use original exchange
        r.error_handler(
            dead_letter_channel("mock:dead")
                .disable_redelivery()
                .log_stack_trace(false)
                .redelivery_delay(0),
        );
        r.on_exception(
            on_exception(ErrorKind::IllegalArgument)
                .maximum_redeliveries(2)
                .use_original_message()
                .handled(true)
                .to("mock:a"),
        );
        r.add(
            from("direct:a")
                .set_body(body().append(" World"))
                .process_fn(my_throw_processor(false)),
        );
        r.add(
            from("direct:b")
                .set_body(body().append(" World"))
                .process_fn(my_throw_processor(true)),
        );
    })
    .await
}

#[tokio::test]
async fn test_use_original_body() {
    let t = setup().await;
    t.mock("mock:a").expected_bodies_received(["Hello"]);
    t.template.send_body("direct:a", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_do_not_use_original_body() {
    let t = setup().await;
    t.mock("mock:dead")
        .expected_bodies_received(["Hello World"]);
    t.template.send_body("direct:b", "Hello").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
