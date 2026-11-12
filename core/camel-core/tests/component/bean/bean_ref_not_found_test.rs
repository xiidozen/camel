//! Port of `BeanRefNotFoundTest`: the routes are created when added in Rust (Java: when the
//! context starts).

use super::{bind, my_foo_bean};
use crate::support::cause_of;
use camel_core::prelude::*;

#[tokio::test]
async fn test_bean_ref_not_found() {
    let t = bind(&[("foo", my_foo_bean())]);
    let e = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:a").route_id("a").bean("foo").to("mock:a"));
            r.add(from("direct:b").route_id("b").bean("bar").to("mock:b"));
        })
        .await
        .expect_err("Should have thrown exception");
    assert!(e.is(ErrorKind::FailedToCreateRoute), "{e:?}");
    assert_eq!(Some("b"), e.detail());
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::NoSuchBean), "{cause:?}");
    assert_eq!(Some("bar"), cause.detail());
}
