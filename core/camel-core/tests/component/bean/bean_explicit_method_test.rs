//! Port of `BeanExplicitMethodTest`.

use super::{my_other_foo_bean, with_beans};
use camel_core::prelude::*;

async fn context() -> crate::support::ContextTestSupport {
    with_beans(&[("foo", my_other_foo_bean())], |r: &mut Routes| {
        r.add(
            from("direct:start")
                .bean_method("foo", "echo")
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_string_echo() {
    let t = context().await;
    t.mock("mock:result")
        .expected_bodies_received(["WorldWorld"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_integer_echo() {
    let t = context().await;
    t.mock("mock:result").expected_bodies_received([25]);
    t.template.send_body("direct:start", 5).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
