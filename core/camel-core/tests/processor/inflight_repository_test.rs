//! Ports of `impl.DefaultInflightRepositoryTest`, `impl.InflightRepositoryRouteTest`,
//! `impl.InflightRepositoryBrowseTest`, `impl.InflightRepositoryBrowseFromRouteTest` and
//! `processor.validator.InflightRepositoryWithFailedValidationTest` (without the validator).

use crate::support::ContextTestSupport;
use camel_core::api::spi::inflight::InflightRepository;
use camel_core::prelude::*;
use std::sync::Arc;

#[tokio::test]
async fn test_default_inflight_repository() {
    let t = ContextTestSupport::new();
    let repo = InflightRepository::default();
    assert_eq!(0, repo.size());
    let e1 = Exchange::new(t.context.clone());
    repo.add(&e1);
    assert_eq!(1, repo.size());
    let e2 = Exchange::new(t.context.clone());
    repo.add(&e2);
    assert_eq!(2, repo.size());
    repo.remove(&e2);
    assert_eq!(1, repo.size());
    repo.remove(&e1);
    assert_eq!(0, repo.size());
}

#[tokio::test]
async fn test_inflight_route() {
    let t = ContextTestSupport::new();
    let repo = t.context.inflight_repository().unwrap();
    let seen = repo.clone();
    t.context
        .add_routes(move |r: &mut Routes| {
            let seen = seen.clone();
            r.add(
                from("direct:start")
                    .route_id("foo")
                    .process_fn(move |_| {
                        assert_eq!(1, seen.size());
                        assert_eq!(1, seen.route_size("foo"));
                        Ok(())
                    })
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    assert_eq!(0, repo.size());
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!(0, repo.size());
    assert_eq!(0, repo.route_size("foo"));
}

/// Checks the browsed exchange in the route `at`, created by the route `foo`.
fn checker(
    repo: Arc<InflightRepository>,
    from_route: Option<&'static str>,
    at: &'static str,
) -> impl Fn(&mut Exchange) -> CamelResult<()> {
    move |e: &mut Exchange| {
        let list = repo.browse(from_route, -1, false);
        assert_eq!(1, list.len());
        assert_eq!(e.exchange_id(), list[0].exchange_id);
        assert_eq!(Some("foo"), list[0].from_route_id.as_deref());
        assert_eq!(Some(at), list[0].at_route_id.as_deref());
        Ok(())
    }
}

#[tokio::test]
async fn test_inflight_browse() {
    for from_route in [false, true] {
        let t = ContextTestSupport::new();
        let repo = t.context.inflight_repository().unwrap();
        repo.set_inflight_browse_enabled(true);
        let r2 = repo.clone();
        t.context
            .add_routes(move |r: &mut Routes| {
                if from_route {
                    r.add(
                        from("direct:start")
                            .route_id("foo")
                            .to("mock:a")
                            .to("direct:bar")
                            .to("mock:result"),
                    );
                    r.add(
                        from("direct:bar")
                            .route_id("bar")
                            .to("mock:b")
                            .process_fn(checker(r2.clone(), Some("foo"), "bar")),
                    );
                } else {
                    r.add(
                        from("direct:start")
                            .route_id("foo")
                            .to("mock:a")
                            .process_fn(checker(r2.clone(), None, "foo"))
                            .to("mock:result"),
                    );
                }
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        assert!(repo.browse(None, -1, false).is_empty());
        t.template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap();
        assert!(repo.browse(None, -1, false).is_empty());
    }
}

#[tokio::test]
async fn test_inflight_with_failure() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .route_id("first")
                .to("direct:validation"),
        );
        r.add(
            from("direct:validation")
                .route_id("second")
                .validate(body().contains("valid"))
                .to("mock:result"),
        );
    })
    .await;
    let repo = t.context.inflight_repository().unwrap();
    assert_eq!(0, repo.size());
    assert!(t.template.send_body("direct:start", "FAIL").await.is_err());
    assert_eq!(0, repo.route_size("first"));
    assert_eq!(0, repo.route_size("second"));
    assert_eq!(0, repo.size());
}
