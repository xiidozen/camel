//! Port of `OnExceptionFromChoiceUseOriginalBodyTest`. The Java route calls the bean
//! `MyServiceBean.validateOrder` from the `when` predicate; here the predicate does the same
//! (the bean component is not ported yet).

use crate::support::ContextTestSupport;
use camel_core::api::processor::predicate_fn;
use camel_core::prelude::*;
use std::sync::Arc;

const TECH: &str = "org.apache.camel.processor.onexception.MyTechnicalException";
const FUNC: &str = "org.apache.camel.processor.onexception.MyFunctionalException";

/// Java `MyServiceBean.validateOrder(payload).equals("James")`.
fn validate_order_is_james() -> Arc<dyn Predicate> {
    Arc::new(predicate_fn(|e: &Exchange| {
        let payload = e.in_message().body().to_string();
        if payload.contains("Tech") {
            return Err(
                CamelError::new(ErrorKind::General, "Damm a tech problem").with_subtype(TECH)
            );
        } else if payload.contains("Func") {
            return Err(
                CamelError::new(ErrorKind::General, "Damm a func problem").with_subtype(FUNC)
            );
        }
        Ok(true)
    }))
}

async fn setup() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:error"));
        r.on_exception(
            on_exception(TECH)
                .use_original_message()
                .maximum_redeliveries(0)
                .handled(true)
                .to("mock:tech"),
        );
        r.on_exception(
            on_exception(FUNC)
                .use_original_message()
                .maximum_redeliveries(0)
                .handled(true)
                .to("mock:func"),
        );
        r.add(
            from("direct:tech")
                .set_body(constant(
                    "<order><type>myType</type><user>Tech</user></order>",
                ))
                .to("direct:route"),
        );
        r.add(
            from("direct:func")
                .set_body(constant(
                    "<order><type>myType</type><user>Func</user></order>",
                ))
                .to("direct:route"),
        );
        r.add(from("direct:route").choice(|c| {
            c.when(validate_order_is_james(), |s| s.to("mock:when"))
                .otherwise(|s| s.to("mock:otherwise"))
        }));
    })
    .await
}

#[tokio::test]
async fn test_no_error_when() {
    let t = setup().await;
    for uri in ["mock:error", "mock:func", "mock:tech", "mock:otherwise"] {
        t.mock(uri).expected_message_count(0);
    }
    t.mock("mock:when").expected_message_count(1);
    t.template
        .send_body(
            "direct:route",
            "<order><type>myType</type><user>James</user></order>",
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_functional_error() {
    let t = setup().await;
    for uri in ["mock:error", "mock:tech", "mock:when", "mock:otherwise"] {
        t.mock(uri).expected_message_count(0);
    }
    t.mock("mock:func").expected_bodies_received(["Func"]);
    t.template.send_body("direct:func", "Func").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_technical_error() {
    let t = setup().await;
    for uri in ["mock:error", "mock:func", "mock:when", "mock:otherwise"] {
        t.mock(uri).expected_message_count(0);
    }
    t.mock("mock:tech").expected_bodies_received(["Tech"]);
    t.template.send_body("direct:tech", "Tech").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
