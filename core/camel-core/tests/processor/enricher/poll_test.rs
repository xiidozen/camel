//! Ports of `PollTest` and `PollDynamicFileNameTest` (the `poll` EIP).

use crate::support::{ContextTestSupport, TestDirectory};
use camel_core::api::exchange_constants::FILE_NAME;
use camel_core::prelude::*;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_poll() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:enricher-test-1")
                .poll("seda:foo1")
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-2")
                .poll_timeout("seda:foo2", 1000)
                .to("mock:mock"),
        );
        r.add(
            from("direct:enricher-test-3")
                .poll_timeout("seda:foo3", -1)
                .to("mock:mock"),
        );
    })
    .await;
    let mock = t.mock("mock:mock");

    // testPoll
    t.template.send_body("seda:foo1", "blah").await.unwrap();
    mock.expected_bodies_received(["blah"]);
    t.template
        .send_body("direct:enricher-test-1", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;

    // testPollWithTimeout: no data the first time
    mock.reset();
    mock.expected_bodies_received(["blah"]);
    t.template
        .send_body("direct:enricher-test-2", "test")
        .await
        .unwrap();
    mock.assert_is_not_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["blah"]);
    t.template.send_body("seda:foo2", "blah").await.unwrap();
    t.template
        .send_body("direct:enricher-test-2", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;

    // testPollNoTimeout: waits for the message sent later
    mock.reset();
    mock.expected_bodies_received(["blah"]);
    let template = t.context.create_producer_template();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        template.send_body("seda:foo3", "blah").await.unwrap();
    });
    let watch = Instant::now();
    t.template
        .send_body("direct:enricher-test-3", "test")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    let delta = watch.elapsed().as_millis();
    assert!(delta > 150, "Should take approx 0.25 sec: was {delta}");
}

async fn dynamic_file_name() -> (TestDirectory, ContextTestSupport) {
    let dir = TestDirectory::new("PollDynamicFileNameTest");
    let uri = dir.file_uri("?noop=true&fileName=${header.target}");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .poll_timeout(&uri, 500)
                .to("mock:result"),
        );
    })
    .await;
    (dir, t)
}

#[tokio::test]
async fn test_poll_dynamic_file_name_one() {
    let (dir, t) = dynamic_file_name().await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(2);
    mock.message_matches(0, body().is_equal_to("Hello World"));
    mock.message_matches(1, body().is_null());
    let file = dir.file_uri("");
    t.template
        .send_body_and_header(&file, "Hello World", FILE_NAME, "myfile.txt")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Foo", "target", "myfile.txt")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Bar", "target", "unknown.txt")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_poll_dynamic_file_name_two() {
    let (dir, t) = dynamic_file_name().await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
    let file = dir.file_uri("");
    t.template
        .send_body_and_header(&file, "Hello World", FILE_NAME, "myfile.txt")
        .await
        .unwrap();
    t.template
        .send_body_and_header(&file, "Bye World", FILE_NAME, "myfile2.txt")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Foo", "target", "myfile.txt")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Bar", "target", "myfile2.txt")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}
