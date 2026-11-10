//! Port of `ConvertBodyTest` (the charset cases wait for charset support, the custom
//! converter case for type converters).

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;

async fn routes() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .convert_body_to("Integer")
                .to("mock:result"),
        );
        r.add(
            from("direct:optional")
                .convert_body_to_optional("Integer")
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_convert_to_integer() {
    let t = routes().await;
    t.mock("mock:result").expected_bodies_received([11]);
    t.template.send_body("direct:start", "11").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_convert_to_integer_not_mandatory() {
    let t = routes().await;
    let err = t
        .template
        .send_body("direct:start", "xxx")
        .await
        .unwrap_err();
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::InvalidPayload), "{err:?}");
    assert!(cause_of(cause).is(ErrorKind::TypeConversion), "{err:?}");
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    result.message_matches(0, body().is_null());
    t.template
        .send_body("direct:optional", "xxx")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_convert_null_body() {
    let t = routes().await;
    let result = t.mock("mock:result");
    result.expected_message_count(1);
    result.message_matches(0, body().is_null());
    t.template
        .send_body("direct:start", Value::Null)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// the Java test converts to java.util.Date, which has no converter from a text; an unknown
// type fails when the route is created, as a Java class that cannot be loaded
#[tokio::test]
async fn test_convert_to_unknown_type() {
    let ctx = camel_core::default_context();
    let r = ctx
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:invalid")
                    .convert_body_to("com.foo.Unknown")
                    .to("mock:result"),
            );
        })
        .await;
    assert!(r.is_err());
}
