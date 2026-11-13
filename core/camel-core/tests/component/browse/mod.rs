//! Ports of the browse component tests: `BrowseTest`, `BrowseFilterTest`, `BrowseLimitTest`,
//! `BrowseRouteTest`, `BrowseRouteConsumeTest` and `BrowseEndpointTest`.

use crate::support::ContextTestSupport;
use camel_browse::{filter_value, BrowseEndpoint};
use camel_core::prelude::*;
use std::sync::Arc;

/// The bodies kept by the browse endpoint.
fn bodies(t: &ContextTestSupport, uri: &str) -> Vec<String> {
    let e = t.context.endpoint(uri).unwrap();
    let b = e
        .as_any()
        .downcast_ref::<BrowseEndpoint>()
        .expect("a browse endpoint");
    b.exchanges()
        .iter()
        .map(|e| e.message().body().to_string())
        .collect()
}

/// The number of endpoints of the context.
fn endpoint_count(t: &ContextTestSupport) -> usize {
    t.context.endpoints().len()
}

/// `BrowseTest`: both endpoints keep the messages.
#[tokio::test]
async fn test_list_endpoints() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("browse:foo").to("browse:bar"));
    })
    .await;
    t.template.send_body("browse:foo", "one").await.unwrap();
    t.template.send_body("browse:foo", "two").await.unwrap();
    assert_eq!(2, endpoint_count(&t), "number of endpoints");
    for uri in ["browse:foo", "browse:bar"] {
        assert_eq!(vec!["one", "two"], bodies(&t, uri), "{uri}");
    }
}

/// `BrowseFilterTest`: only the accepted messages are kept.
#[tokio::test]
async fn test_filter() {
    let t = ContextTestSupport::new();
    let even: camel_core::api::processor::SharedPredicate =
        Arc::new(camel_core::api::processor::predicate_fn(|e: &Exchange| {
            let b = e.message().body().to_string();
            Ok(b == "two" || b == "four")
        }));
    t.context
        .registry()
        .unwrap()
        .bind("evenFilter", filter_value(even))
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("browse:foo").to("browse:bar?filter=#evenFilter"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    for b in ["one", "two", "three", "four", "five"] {
        t.template.send_body("browse:foo", b).await.unwrap();
    }
    assert_eq!(2, endpoint_count(&t), "number of endpoints");
    assert_eq!(5, bodies(&t, "browse:foo").len());
    assert_eq!(
        vec!["two", "four"],
        bodies(&t, "browse:bar?filter=#evenFilter")
    );
}

/// `BrowseLimitTest`: the oldest messages beyond the limit are dropped.
#[tokio::test]
async fn test_limit() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("browse:foo?browseLimit=1").to("browse:bar?browseLimit=5"));
    })
    .await;
    for b in ["one", "two", "three", "four", "five"] {
        t.template
            .send_body("browse:foo?browseLimit=1", b)
            .await
            .unwrap();
    }
    assert_eq!(2, endpoint_count(&t), "number of endpoints");
    assert_eq!(vec!["five"], bodies(&t, "browse:foo?browseLimit=1"));
    assert_eq!(5, bodies(&t, "browse:bar?browseLimit=5").len());
}

/// `BrowseRouteTest` and `BrowseRouteConsumeTest`: a browse endpoint in a route, with and
/// without a consumer.
#[tokio::test]
async fn test_browse_route() {
    for consume in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from("direct:start").to("browse:foo").to("mock:result"));
            if consume {
                r.add(from("browse:foo").to("log:foo").to("mock:foo"));
            }
        })
        .await;
        let mock = t.mock(if consume { "mock:foo" } else { "mock:result" });
        mock.expected_bodies_received(["Hello World", "Bye World"]);
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        t.template
            .send_body("direct:start", "Bye World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
        let kept: Vec<String> = bodies(&t, "browse:foo");
        assert_eq!(vec!["Hello World", "Bye World"], kept);
    }
}

/// `BrowseEndpointTest` (the endpoint created by uri; Java also creates it with constructors).
#[tokio::test]
async fn test_browse_endpoint() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").to("browse://foo"));
    })
    .await;
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!(1, bodies(&t, "browse://foo").len());
}
