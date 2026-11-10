//! Ports of `RecipientListTest`, `RecipientListWithDelimiterTest`,
//! `RecipientListWithStringDelimitedHeaderTest`, `RecipientListIgnoreInvalidEndpointsTest`
//! and `RecipientListStopOnExceptionTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;

async fn expect_xyz(t: &ContextTestSupport) {
    for m in ["mock:x", "mock:y", "mock:z"] {
        t.mock(m).expected_bodies_received(["answer"]);
    }
}

#[tokio::test]
async fn test_sending_a_message_using_multicast_receives_its_own_exchange() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").recipient_list(header("recipientListHeader")));
    })
    .await;
    expect_xyz(&t).await;
    t.template
        .send_body_and_header(
            "direct:a",
            "answer",
            "recipientListHeader",
            "mock:x,mock:y,mock:z",
        )
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_recipient_list_with_delimiter() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .recipient_list(header("myHeader"))
                .delimiter("#"),
        );
    })
    .await;
    expect_xyz(&t).await;
    t.template
        .send_body_and_header("direct:a", "answer", "myHeader", "mock:x#mock:y#mock:z")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_recipient_list_with_delimiter_disabled() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .recipient_list(header("myHeader"))
                .delimiter("false"),
        );
    })
    .await;
    t.mock("mock:falseDelimiterTest")
        .expected_bodies_received(["answer"]);
    t.template
        .send_body_and_header("direct:a", "answer", "myHeader", "mock:falseDelimiterTest")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

// RecipientListWithStringDelimitedHeaderTest: the uris are trimmed
#[tokio::test]
async fn test_string_delimited_header() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:a").recipient_list(header("myHeader")));
    })
    .await;
    expect_xyz(&t).await;
    t.template
        .send_body_and_header("direct:a", "answer", "myHeader", "mock:x, mock:y, mock:z")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

async fn ignore_invalid_routes() -> ContextTestSupport {
    ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:startA")
                .recipient_list(simple("mock:result,fail:endpoint,direct:a"))
                .ignore_invalid_endpoints(),
        );
        r.add(from("direct:startB").recipient_list(simple("mock:result,fail:endpoint,direct:a")));
        r.add(
            from("direct:a")
                .transform(constant("Hello a"))
                .to("mock:endpointA"),
        );
    })
    .await
}

#[tokio::test]
async fn test_recipient_list_with_ignore_invalid_endpoints_option() {
    let t = ignore_invalid_routes().await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.mock("mock:endpointA")
        .expected_bodies_received(["Hello a"]);
    t.template
        .request_body("direct:startA", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_recipient_list_without_ignore_invalid_endpoints_option() {
    let t = ignore_invalid_routes().await;
    t.mock("mock:result").expected_message_count(0);
    t.mock("mock:endpointA").expected_message_count(0);
    let err = t
        .template
        .request_body("direct:startB", "Hello World")
        .await
        .unwrap_err();
    assert!(
        cause_of(&err).is(ErrorKind::NoSuchEndpoint),
        "Get a wrong cause of the exception: {err:?}"
    );
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_recipient_list_stop_on_exception() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .recipient_list(header("foo"))
                .stop_on_exception()
                .to("mock:result"),
        );
        r.add(from("direct:a").to("mock:a"));
        r.add(
            from("direct:b")
                .to("mock:b")
                .throw_exception(CamelError::illegal_argument("Damn")),
        );
        r.add(from("direct:c").to("mock:c"));
    })
    .await;
    t.mock("mock:result").expected_message_count(0);
    t.mock("mock:a").expected_message_count(1);
    t.mock("mock:b").expected_message_count(1);
    t.mock("mock:c").expected_message_count(0);
    let err = t
        .template
        .send_body_and_header(
            "direct:start",
            "Hello World",
            "foo",
            "direct:a,direct:b,direct:c",
        )
        .await
        .unwrap_err();
    assert!(err.is(ErrorKind::CamelExecution), "{err:?}");
    let cause = cause_of(&err);
    assert!(cause.is(ErrorKind::CamelExchange), "{err:?}");
    let root = cause_of(cause);
    assert!(root.is(ErrorKind::IllegalArgument), "{err:?}");
    assert_eq!(Some("Damn"), root.message());
    t.assert_mock_endpoints_satisfied().await;
}
