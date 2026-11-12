//! Port of `PropertiesRouteIdTest` (the id of the route; the ids of the steps are not in the
//! Rust DSL yet).

use super::{start, with_location};
use camel_core::prelude::*;

#[tokio::test]
async fn test_properties_route_id() {
    let t = with_location();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .route_id("{{cool.name}}")
                .to("mock:result"),
        );
    })
    .await;
    assert_eq!(1, t.context.routes().len());
    assert!(
        t.context.route("Camel").is_some(),
        "Route with name Camel should exist"
    );
}
