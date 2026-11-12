//! Port of `MethodCallRefOrBeanPrefixTest`.

use super::{my_foo_bean, with_beans};
use camel_core::prelude::*;

#[tokio::test]
async fn test_ref_or_bean_prefix() {
    let t = with_beans(&[("foo", my_foo_bean())], |r: &mut Routes| {
        r.add(
            from("direct:a")
                .transform(method_ref("ref:foo"))
                .to("mock:a"),
        );
        r.add(
            from("direct:b")
                .transform(method_ref("bean:foo"))
                .to("mock:b"),
        );
    })
    .await;
    t.mock("mock:a").expected_bodies_received(["Hello A"]);
    t.mock("mock:b").expected_bodies_received(["Hello B"]);
    t.template.send_body("direct:a", "A").await.unwrap();
    t.template.send_body("direct:b", "B").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
