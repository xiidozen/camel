//! Port of `ValidateSimpleTest` (the predicate text is the Rust form of the simple
//! predicate).

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;

async fn context() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .validate(simple("${body} contains 'Camel'"))
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_send_matching_message() {
    let t = context().await;
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_send_not_matching_message() {
    let t = context().await;
    t.mock("mock:result").expected_message_count(0);
    let e = t
        .template
        .send_body("direct:start", "Bye World")
        .await
        .expect_err("CamelExecutionException expected");
    let cause = cause_of(&e);
    assert!(
        cause.is_subtype("PredicateValidationException"),
        "{cause:?}"
    );
    assert!(cause.is(ErrorKind::Validation));
    assert!(
        cause
            .to_string()
            .contains("Validation failed for Predicate["),
        "{cause}"
    );
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        "Validate[simple{${body} contains 'Camel'} -> []]",
        from("direct:x")
            .validate(simple("${body} contains 'Camel'"))
            .build()
            .steps[0]
            .to_string()
    );
}

/// Ports of `ValidateRegExpTest` and `ValidateSimpleRegExpTest`.
#[tokio::test]
async fn test_validate_reg_exp() {
    for use_simple in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let route = from("direct:start");
            let route = if use_simple {
                route.validate(simple(
                    r"${bodyAs(java.lang.String)} regex '^\d{2}\.\d{2}\.\d{4}$'",
                ))
            } else {
                route.validate(body().regex(r"^\d{2}\.\d{2}\.\d{4}$"))
            };
            r.add(route.to("mock:result"));
        })
        .await;
        let result = t.mock("mock:result");
        result.expected_message_count(1);
        t.template
            .send_body("direct:start", "01.01.2010")
            .await
            .unwrap();
        result.assert_is_satisfied().await;
        result.reset();
        result.expected_message_count(0);
        let e = t
            .template
            .send_body("direct:start", "1.1.2010")
            .await
            .expect_err("CamelExecutionException expected");
        let cause = cause_of(&e);
        assert!(
            cause.is_subtype("PredicateValidationException"),
            "{cause:?}"
        );
        assert!(
            cause
                .to_string()
                .contains("Validation failed for Predicate"),
            "{cause}"
        );
        assert!(
            cause.to_string().contains(r"^\d{2}\.\d{2}\.\d{4}$"),
            "{cause}"
        );
        result.assert_is_satisfied().await;
    }
}
