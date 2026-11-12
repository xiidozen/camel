//! Port of `InterceptSendToEndpointWithStopTest`, `InterceptSendToEndpointConditionalSkipTest`
//! and the route cases of `InterceptSendToEndpointAfterTest`.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::INTERCEPTED_ENDPOINT;
use camel_core::prelude::*;

async fn with_stop() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept_send_to_endpoint("mock:b", |i| {
            i.choice(|c| {
                c.when(body().is_equal_to("stop"), |w| w.stop())
                    .otherwise(|o| o.to("mock:c"))
            })
        });
        r.add(
            from("direct:start")
                .to("mock:a")
                .to("mock:b")
                .to("mock:result"),
        );
    })
    .await
}

#[tokio::test]
async fn test_intercept_send_to_endpoint_with_stop() {
    let t = with_stop().await;
    t.mock("mock:a").expected_message_count(1);
    for m in ["mock:b", "mock:c", "mock:result"] {
        t.mock(m).expected_message_count(0);
    }
    t.template.send_body("direct:start", "stop").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_intercept_send_to_endpoint_with_no_stop() {
    let t = with_stop().await;
    for m in ["mock:a", "mock:b", "mock:c", "mock:result"] {
        t.mock(m).expected_message_count(1);
    }
    t.mock("mock:b")
        .expected_property_received(INTERCEPTED_ENDPOINT, "mock://b");
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn conditional_skip() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept_send_to_endpoint("mock:skippable", |i| {
            i.skip_send_to_original_endpoint()
                .on_when(body().is_equal_to("skip"))
                .to("mock:detour")
        });
        r.intercept_send_to_endpoint("mock:skippableNoEffect", |i| {
            i.skip_send_to_original_endpoint().choice(|c| {
                c.when(body().is_equal_to("skipNoEffectWhen"), |w| {
                    w.to("mock:noSkipWhen")
                })
                .otherwise(|o| o.to("mock:noSkipOW"))
            })
        });
        r.add(
            from("direct:start")
                .to("mock:a")
                .to("mock:skippable")
                .to("mock:c"),
        );
        r.add(
            from("direct:startNoEffect")
                .to("mock:a")
                .to("mock:skippableNoEffect")
                .to("mock:c"),
        );
    })
    .await
}

#[tokio::test]
async fn test_skip_condition_satisfied() {
    let t = conditional_skip().await;
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:skippable").expected_message_count(0);
    t.mock("mock:detour").expected_message_count(1);
    t.mock("mock:c").expected_message_count(1);
    t.template.send_body("direct:start", "skip").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_skip_condition_not_satisfied() {
    let t = conditional_skip().await;
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:skippable").expected_message_count(1);
    t.mock("mock:detour").expected_message_count(0);
    t.mock("mock:c").expected_message_count(1);
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_skip_condition_no_effect_choice() {
    let t = conditional_skip().await;
    t.mock("mock:a").expected_message_count(2);
    t.mock("mock:skippableNoEffect").expected_message_count(0);
    t.mock("mock:c").expected_message_count(2);
    t.mock("mock:noSkipWhen").expected_message_count(1);
    t.mock("mock:noSkipOW").expected_message_count(1);
    t.template
        .send_body("direct:startNoEffect", "skipNoEffectWhen")
        .await
        .unwrap();
    t.template
        .send_body("direct:startNoEffect", "Hello Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_intercept_endpoint_after_uri() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.intercept_send_to_endpoint("mock:foo", |i| {
            i.to("mock:detour")
                .transform(constant("Bye World"))
                .after_uri("direct:after")
        });
        r.add(
            from("direct:first")
                .to("mock:bar")
                .to("mock:foo")
                .to("mock:result"),
        );
        r.add(from("direct:after").to("mock:after"));
    })
    .await;
    t.mock("mock:bar").expected_bodies_received(["Hello World"]);
    t.mock("mock:detour")
        .expected_bodies_received(["Hello World"]);
    t.mock("mock:foo").expected_bodies_received(["Bye World"]);
    t.mock("mock:result")
        .expected_bodies_received(["Bye World"]);
    t.mock("mock:after").expected_bodies_received(["Bye World"]);
    t.template
        .send_body("direct:first", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
