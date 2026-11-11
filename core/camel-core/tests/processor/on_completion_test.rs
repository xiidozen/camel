//! Ports of `OnCompletionTest`, `OnCompletionGlobalTest`, `OnCompletionOnFailureOnlyTest`,
//! `OnCompletionOnCompleteOnlyTest`, `OnCompletionWhenTest`, `OnCompletionUseOriginalBodyTest`,
//! `OnCompletionModeTest`, `OnCompletionRouteScopeOverrideGlobalScopeTest`,
//! `OnCompletionInvalidConfiguredTest` and `OnCompletionFailAndOkTest`.

use crate::support::{caught_is, cause_of, ContextTestSupport};
use camel_core::api::exchange_constants::ON_COMPLETION;
use camel_core::prelude::*;

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
