//! Port of `BeanRefMethodNotFoundTest`.

use super::{bind, my_foo_bean};
use crate::support::cause_of;
use camel_core::prelude::*;

#[tokio::test]
async fn test_bean_ref_method_not_found() {
    let t = bind(&[("foo", my_foo_bean())]);
    let e = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:a")
                    .route_id("a")
                    .bean_method("foo", "hello")
                    .to("mock:a"),
            );
            r.add(
                from("direct:b")
                    .route_id("b")
                    .bean_method("foo", "bye")
                    .to("mock:b"),
            );
        })
        .await
        .expect_err("Should have thrown exception");
    assert!(e.is(ErrorKind::FailedToCreateRoute), "{e:?}");
    assert_eq!(Some("b"), e.detail());
    let cause = cause_of(&e);
    assert!(cause.is_subtype("MethodNotFoundException"), "{cause:?}");
    assert_eq!(Some("bye"), cause.detail());
}
