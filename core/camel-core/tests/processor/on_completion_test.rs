//! Ports of `OnCompletionTest`, `OnCompletionGlobalTest`, `OnCompletionOnFailureOnlyTest`,
//! `OnCompletionOnCompleteOnlyTest`, `OnCompletionWhenTest`, `OnCompletionUseOriginalBodyTest`,
//! `OnCompletionModeTest`, `OnCompletionRouteScopeOverrideGlobalScopeTest`,
//! `OnCompletionInvalidConfiguredTest`, `OnCompletionFailAndOkTest`, `OnCompletionMoreGlobalTest`,
//! `OnCompletionMoreGlobalRouteCompletionTest`, `OnCompletionParallelProcessingTest`,
//! `OnCompletionShouldBeLastTest`, `OnCompletionContainsTest`,
//! `OnCompletionAfterChainedSedaRoutesTest`, `OnCompletionBeforeChainedSedaRoutesTest` and
//! `OnCompletionAndInterceptGlobalTest`.

use crate::support::{caught_is, cause_of, ContextTestSupport};
use camel_core::api::exchange_constants::ON_COMPLETION;
use camel_core::api::processor::BoxFuture;
use camel_core::api::spi::synchronization::{SharedSynchronization, Synchronization};
use camel_core::prelude::*;
use std::sync::Arc;

/// Java `OnCompletionTest.MyProcessor`.
fn my_processor(e: &mut Exchange) -> CamelResult<()> {
    if e.message().body().to_string().contains("Kaboom") {
        return Err(CamelError::illegal_argument("Kaboom"));
    }
    e.message_mut().set_body("Bye World");
    Ok(())
}

async fn assert_kaboom(t: &ContextTestSupport) {
    let err = t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .unwrap_err();
    assert_eq!(Some("Kaboom"), cause_of(&err).message());
}

async fn route_scoped(block: fn(OnCompletionDsl) -> OnCompletionDsl) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .on_completion(|c| block(c).to("log:sync").to("mock:sync"))
                .process_fn(my_processor)
                .to("mock:result"),
        );
    })
    .await
}

async fn synchronize_complete(t: &ContextTestSupport) {
    let sync = t.mock("mock:sync");
    sync.expected_bodies_received(["Bye World"]);
    sync.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn synchronize_failure(t: &ContextTestSupport) {
    let sync = t.mock("mock:sync");
    sync.expected_message_count(1);
    sync.expected_property_received(ON_COMPLETION, true);
    sync.message_matches(0, caught_is(ErrorKind::IllegalArgument));
    t.mock("mock:result").expected_message_count(0);
    assert_kaboom(t).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_synchronize_complete() {
    synchronize_complete(&route_scoped(|c| c).await).await;
}

#[tokio::test]
async fn test_synchronize_failure() {
    synchronize_failure(&route_scoped(|c| c).await).await;
}

async fn global() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_completion(|c| c.to("log:global").to("mock:sync"));
        r.add(
            from("direct:start")
                .process_fn(my_processor)
                .to("mock:result"),
        );
    })
    .await
}

// OnCompletionGlobalTest
#[tokio::test]
async fn test_global_synchronize_complete() {
    synchronize_complete(&global().await).await;
}

#[tokio::test]
async fn test_global_synchronize_failure() {
    let t = global().await;
    let sync = t.mock("mock:sync");
    sync.expected_message_count(1);
    sync.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:result").expected_message_count(0);
    assert_kaboom(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionOnFailureOnlyTest
#[tokio::test]
async fn test_on_failure_only_synchronize_complete() {
    let t = route_scoped(|c| c.on_failure_only()).await;
    t.mock("mock:sync").expected_message_count(0);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_on_failure_only_synchronize_failure() {
    synchronize_failure(&route_scoped(|c| c.on_failure_only()).await).await;
}

// OnCompletionOnCompleteOnlyTest
#[tokio::test]
async fn test_on_complete_only_synchronize_complete() {
    synchronize_complete(&route_scoped(|c| c.on_complete_only()).await).await;
}

#[tokio::test]
async fn test_on_complete_only_synchronize_failure() {
    let t = route_scoped(|c| c.on_complete_only()).await;
    t.mock("mock:sync").expected_message_count(0);
    t.mock("mock:result").expected_message_count(0);
    assert_kaboom(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionWhenTest
async fn when() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .on_completion(|c| {
                    c.on_when(body().contains("Hello"))
                        .to("log:sync")
                        .to("mock:sync")
                })
                .to("log:original")
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_synchronize_complete_when() {
    let t = when().await;
    let sync = t.mock("mock:sync");
    sync.expected_bodies_received(["Hello World"]);
    sync.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_synchronize_complete_no_when() {
    let t = when().await;
    t.mock("mock:sync").expected_message_count(0);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Bye World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionUseOriginalBodyTest
#[tokio::test]
async fn test_on_completion_use_original_body() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_completion(|c| {
            c.use_original_message()
                .parallel_processing()
                .to("mock:before")
                .delay(constant(1000))
                .set_body(simple("OnComplete:${body}"))
                .to("mock:after")
        });
        r.add(
            from("direct:start")
                .process_fn(my_processor)
                .to("mock:result"),
        );
    })
    .await;
    let before = t.mock("mock:before");
    before.expected_bodies_received(["Hello World"]);
    before.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:after")
        .expected_bodies_received(["OnComplete:Hello World"]);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    let out = t
        .template
        .request_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert_eq!("Bye World", out.to_string());
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionModeTest
async fn mode() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:foo")
                .on_completion(|c| {
                    c.mode_before_consumer()
                        .transform(body().prepend("I was here "))
                        .to("mock:after")
                })
                .to("mock:input")
                .transform(body().prepend("Hello "))
                .to("log:foo"),
        );
        r.add(
            from("seda:bar")
                .on_completion(|c| {
                    c.mode_after_consumer()
                        .parallel_processing()
                        .transform(body().prepend("I was here "))
                        .to("mock:after")
                })
                .to("mock:input")
                .transform(body().prepend("Hello "))
                .to("log:bar"),
        );
    })
    .await
}

#[tokio::test]
async fn test_on_completion_scope_before() {
    let t = mode().await;
    t.mock("mock:input").expected_bodies_received(["Camel"]);
    t.mock("mock:after")
        .expected_bodies_received(["I was here Hello Camel"]);
    let out = t.template.request_body("seda:foo", "Camel").await.unwrap();
    assert_eq!("I was here Hello Camel", out.to_string());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_on_completion_scope_after() {
    let t = mode().await;
    t.mock("mock:input").expected_bodies_received(["World"]);
    t.mock("mock:after")
        .expected_bodies_received(["I was here Hello World"]);
    let out = t.template.request_body("seda:bar", "World").await.unwrap();
    assert_eq!("Hello World", out.to_string());
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionRouteScopeOverrideGlobalScopeTest
async fn override_global() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.on_completion(|c| c.to("log:global").to("mock:global"));
        r.add(
            from("direct:start")
                .on_completion(|c| c.to("log:route").to("mock:sync"))
                .process_fn(my_processor)
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_route_scope_override_global_synchronize_complete() {
    let t = override_global().await;
    t.mock("mock:global").expected_message_count(0);
    synchronize_complete(&t).await;
}

#[tokio::test]
async fn test_route_scope_override_global_synchronize_failure() {
    let t = override_global().await;
    t.mock("mock:global").expected_message_count(0);
    let sync = t.mock("mock:sync");
    sync.expected_message_count(1);
    sync.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:result").expected_message_count(0);
    assert_kaboom(&t).await;
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionInvalidConfiguredTest
// Java validates while building (the node has no steps yet: `onCompletion[[]]`); the Rust DSL
// is infallible, so the reifier validates and the node shows its steps.
#[tokio::test]
async fn test_invalid_configured() {
    let ctx = camel_core::default_context();
    let err = ctx
        .add_routes(|r: &mut Routes| {
            r.on_completion(|c| c.on_failure_only().on_complete_only().to("mock:foo"));
            r.add(from("direct:start").to("mock:result"));
        })
        .await
        .unwrap_err();
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::IllegalArgument), "{err:?}");
    assert_eq!(
        Some("Both onCompleteOnly and onFailureOnly cannot be true. Only one of them can be true. On node: onCompletion[[to[mock:foo]]]"),
        cause.message()
    );
}

// OnCompletionFailAndOkTest
async fn fail_and_ok() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .on_completion(|c| c.on_complete_only().to("log:ok").to("mock:ok"))
                .on_completion(|c| c.on_failure_only().to("log:fail").to("mock:fail"))
                .process_fn(my_processor)
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_ok() {
    let t = fail_and_ok().await;
    t.mock("mock:ok").expected_message_count(1);
    t.mock("mock:fail").expected_message_count(0);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_fail() {
    let t = fail_and_ok().await;
    t.mock("mock:ok").expected_message_count(0);
    t.mock("mock:fail").expected_message_count(1);
    t.mock("mock:result").expected_message_count(0);
    assert!(t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .is_err());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_ok_and_fail() {
    let t = fail_and_ok().await;
    t.mock("mock:ok").expected_bodies_received(["Bye World"]);
    t.mock("mock:fail").expected_bodies_received(["Kaboom"]);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    assert!(t
        .template
        .send_body("direct:start", "Kaboom")
        .await
        .is_err());
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionMoreGlobalTest
fn more_global(r: &mut Routes) {
    r.on_completion(|c| c.to("log:global").to("mock:sync"));
    r.on_completion(|c| c.to("log:global").to("mock:two"));
    r.on_completion(|c| c.on_complete_only().to("log:global").to("mock:complete"));
    r.on_completion(|c| c.on_failure_only().to("log:global").to("mock:failure"));
    r.add(
        from("direct:start")
            .process_fn(my_processor)
            .to("mock:result"),
    );
}

async fn more_global_complete(t: &ContextTestSupport) {
    t.mock("mock:complete")
        .expected_bodies_received(["Bye World"]);
    t.mock("mock:failure").expected_message_count(0);
    t.mock("mock:two").expected_bodies_received(["Bye World"]);
    let sync = t.mock("mock:sync");
    sync.expected_bodies_received(["Bye World"]);
    sync.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn more_global_failure(t: &ContextTestSupport) {
    t.mock("mock:complete").expected_message_count(0);
    t.mock("mock:failure").expected_message_count(1);
    t.mock("mock:two").expected_message_count(1);
    let sync = t.mock("mock:sync");
    sync.expected_message_count(1);
    sync.expected_property_received(ON_COMPLETION, true);
    t.mock("mock:result").expected_message_count(0);
    assert_kaboom(t).await;
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_more_global_synchronize_complete() {
    more_global_complete(&ContextTestSupport::with_routes(more_global).await).await;
}

#[tokio::test]
async fn test_more_global_synchronize_failure() {
    more_global_failure(&ContextTestSupport::with_routes(more_global).await).await;
}

// OnCompletionMoreGlobalRouteCompletionTest
fn more_global_route_completion(r: &mut Routes) {
    more_global(r);
    // these route completions should override the global
    r.add(
        from("direct:other")
            .on_completion(|c| c.on_complete_only().to("mock:routeComplete"))
            .on_completion(|c| c.on_failure_only().to("mock:routeFailure"))
            .on_completion(|c| c.to("mock:routeAll"))
            .process_fn(my_processor)
            .to("mock:other"),
    );
}

#[tokio::test]
async fn test_more_global_route_completion_synchronize_complete() {
    more_global_complete(&ContextTestSupport::with_routes(more_global_route_completion).await)
        .await;
}

#[tokio::test]
async fn test_more_global_route_completion_synchronize_failure() {
    more_global_failure(&ContextTestSupport::with_routes(more_global_route_completion).await).await;
}

#[tokio::test]
async fn test_more_global_route_completion_synchronize_other_complete() {
    let t = ContextTestSupport::with_routes(more_global_route_completion).await;
    for uri in ["mock:complete", "mock:failure", "mock:two", "mock:sync"] {
        t.mock(uri).expected_message_count(0);
    }
    t.mock("mock:routeComplete")
        .expected_bodies_received(["Bye World"]);
    t.mock("mock:routeFailure").expected_message_count(0);
    t.mock("mock:routeAll")
        .expected_bodies_received(["Bye World"]);
    t.mock("mock:other").expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:other", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_more_global_route_completion_synchronize_other_failure() {
    let t = ContextTestSupport::with_routes(more_global_route_completion).await;
    // the global onCompletions should not be invoked
    for uri in ["mock:complete", "mock:failure", "mock:two", "mock:sync"] {
        t.mock(uri).expected_message_count(0);
    }
    // the onCompletions of the route for any outcome and for failure should be invoked
    t.mock("mock:routeComplete").expected_message_count(0);
    t.mock("mock:routeFailure").expected_message_count(1);
    t.mock("mock:routeAll").expected_message_count(1);
    t.mock("mock:other").expected_message_count(0);
    let err = t
        .template
        .send_body("direct:other", "Kaboom")
        .await
        .unwrap_err();
    assert_eq!(Some("Kaboom"), cause_of(&err).message());
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionParallelProcessingTest
#[tokio::test]
async fn test_on_completion_parallel() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("seda:bar")
                .on_completion(|c| {
                    c.parallel_processing()
                        .transform(body().prepend("I was here "))
                        .to("mock:after")
                })
                .to("mock:input")
                .transform(body().prepend("Hello "))
                .to("log:bar"),
        );
    })
    .await;
    t.mock("mock:input").expected_bodies_received(["World"]);
    t.mock("mock:after")
        .expected_bodies_received(["I was here Hello World"]);
    let out = t.template.request_body("seda:bar", "World").await.unwrap();
    assert_eq!("Hello World", out.to_string());
    t.assert_mock_endpoints_satisfied().await;
}

/// Java `SynchronizationAdapter` whose `onDone` sends the body to the endpoint.
struct SendOnDone {
    context: Arc<DefaultCamelContext>,
    uri: &'static str,
    body: &'static str,
}

impl SendOnDone {
    fn shared(context: &Arc<DefaultCamelContext>, body: &'static str) -> SharedSynchronization {
        Arc::new(SendOnDone {
            context: context.clone(),
            uri: "mock:sync",
            body,
        })
    }

    fn on_done(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let template = self.context.create_producer_template();
            template.send_body(self.uri, self.body).await.unwrap();
        })
    }
}

impl Synchronization for SendOnDone {
    fn on_complete<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        self.on_done()
    }

    fn on_failure<'a>(&'a self, _exchange: &'a mut Exchange) -> BoxFuture<'a, ()> {
        self.on_done()
    }
}

/// A context with a global onCompletion to `mock:sync` and a route whose processor adds the
/// synchronizations.
async fn with_adapters(
    add: impl Fn(&Arc<DefaultCamelContext>, &mut Exchange) + Send + Sync + 'static,
) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    let context = t.context.clone();
    let add = Arc::new(add);
    t.context
        .add_routes(move |r: &mut Routes| {
            r.on_completion(|c| c.to("mock:sync"));
            let (context, add) = (context.clone(), add.clone());
            r.add(
                from("direct:start")
                    .process_fn(move |e| {
                        add(&context, e);
                        Ok(())
                    })
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t
}

// OnCompletionShouldBeLastTest
#[tokio::test]
async fn test_on_completion_should_be_last() {
    let t = with_adapters(|context, e| {
        for body in ["A", "B", "C"] {
            e.add_on_completion(SendOnDone::shared(context, body));
        }
    })
    .await;
    t.mock("mock:sync")
        .expected_bodies_received(["C", "B", "A", "Hello World"]);
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionContainsTest
#[tokio::test]
async fn test_on_completion_contains() {
    let t = with_adapters(|context, e| {
        let adapter = SendOnDone::shared(context, "A");
        e.add_on_completion(adapter.clone());
        // should not add the adapter again as we already have it
        if !e.contains_on_completion(&adapter) {
            e.add_on_completion(adapter);
        }
        let adapter = SendOnDone::shared(context, "B");
        e.add_on_completion(adapter.clone());
        // now add the B again as we want to test that this also works
        if e.contains_on_completion(&adapter) {
            e.add_on_completion(adapter);
        }
        e.add_on_completion(SendOnDone::shared(context, "C"));
    })
    .await;
    t.mock("mock:sync")
        .expected_bodies_received(["C", "B", "B", "A", "Hello World"]);
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionAfterChainedSedaRoutesTest and OnCompletionBeforeChainedSedaRoutesTest
fn chained(r: &mut Routes, before_consumer: bool) {
    let completion = move |name: &'static str| {
        move |c: OnCompletionDsl| {
            let c = if before_consumer {
                c.mode_before_consumer()
            } else {
                c
            };
            c.log(&format!("{name} - done"))
                .process_fn(move |e| {
                    e.message_mut().set_body(format!("completion:{name}"));
                    Ok(())
                })
                .to("mock:completion")
        }
    };
    r.add(from("direct:a").on_completion(completion("a")).to("seda:b"));
    r.add(
        from("seda:b")
            .on_completion(completion("b"))
            .delay(constant(100))
            .to("seda:c"),
    );
    r.add(
        from("seda:c")
            .on_completion(completion("c"))
            .delay(constant(100))
            .to("seda:d"),
    );
    r.add(
        from("seda:d")
            .on_completion(completion("d"))
            .delay(constant(100))
            .to("mock:completion"),
    );
}

#[tokio::test]
async fn test_on_completion_after_chained_seda_routes() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| chained(r, false)).await;
    let body = "<body>";
    let mock = t.mock("mock:completion");
    mock.expected_message_count(5);
    mock.expected_bodies_received([
        body,
        "completion:d",
        "completion:c",
        "completion:b",
        "completion:a",
    ]);
    t.template.send_body("direct:a", body).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_on_completion_before_chained_seda_routes() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| chained(r, true)).await;
    let body = "<body>";
    let mock = t.mock("mock:completion");
    mock.expected_message_count(5);
    mock.expected_bodies_received_in_any_order([
        "completion:a",
        "completion:b",
        "completion:c",
        body,
        "completion:d",
    ]);
    t.template.send_body("direct:a", body).await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// OnCompletionAndInterceptGlobalTest
#[tokio::test]
async fn test_on_completion_and_intercept_global() {
    for kaboom in [false, true] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.intercept(|i| i.to("mock:intercept"));
            r.on_completion(|c| c.to("log:global").to("mock:sync"));
            r.add(
                from("direct:start")
                    .process_fn(my_processor)
                    .to("mock:result"),
            );
        })
        .await;
        t.mock("mock:intercept").expected_minimum_message_count(4);
        let sync = t.mock("mock:sync");
        sync.expected_property_received(ON_COMPLETION, true);
        if kaboom {
            sync.expected_message_count(1);
            t.mock("mock:result").expected_message_count(0);
            assert_kaboom(&t).await;
        } else {
            sync.expected_bodies_received(["Bye World"]);
            t.mock("mock:result")
                .expected_bodies_received(["Bye World"]);
            t.template
                .send_body("direct:start", "Hello World")
                .await
                .unwrap();
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}
