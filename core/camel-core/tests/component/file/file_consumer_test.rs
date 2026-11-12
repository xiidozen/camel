//! Ports of `FileConsumerProducerRouteTest`, `FileConsumerIncludeExtTest`,
//! `FileConsumerExcludeExtTest`, `FileConsumeSimpleRelativeMoveToRelativeTest`,
//! `FileConsumerDeleteAndFailureTest`, `FileConsumerDeleteAndMoveFailedTest` and
//! `FromFileMoveFileIfProcessFailsTest`.

use crate::support::{ContextTestSupport, TestDirectory};
use camel_core::api::error::ErrorType;
use camel_core::api::exchange_constants::FILE_NAME;
use camel_core::prelude::*;

async fn send(t: &ContextTestSupport, dir: &TestDirectory, name: &str, body: &str) {
    t.template
        .send_body_and_header(&dir.file_uri(""), body, FILE_NAME, name)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_file_consumer_producer_route() {
    let dir = TestDirectory::new("FileConsumerProducerRouteTest");
    let (a, b, b_in) = (
        dir.file_uri("a?initialDelay=0&delay=10"),
        dir.file_uri("b"),
        dir.file_uri("b?initialDelay=0&delay=10"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&a).to(&b));
        r.add(from(&b_in).to("mock:result"));
    })
    .await;
    let result = t.mock("mock:result");
    result.expected_message_count(2);
    for (name, body) in [("hello.txt", "Hello World"), ("bye.txt", "Bye World")] {
        t.template
            .send_body_and_header(&dir.file_uri("a"), body, FILE_NAME, name)
            .await
            .unwrap();
    }
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_include_and_exclude_ext() {
    for (option, expected) in [
        (
            "?initialDelay=250&delay=10&includeExt=txt,dat",
            vec!["Hello World", "123", "Bye World"],
        ),
        (
            "?initialDelay=0&delay=10&excludeExt=XML,dat",
            vec!["Hello World", "Bye World"],
        ),
    ] {
        let dir = TestDirectory::new("FileConsumerIncludeExtTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).to("mock:txt"));
        })
        .await;
        t.mock("mock:txt")
            .expected_bodies_received_in_any_order(expected);
        for (name, body) in [
            ("hello.txt", "Hello World"),
            ("customer.xml", "<customer>123</customer>"),
            ("order.DAT", "123"),
            ("book.Xml", "<book>Camel Rocks</book>"),
            ("bye.txt", "Bye World"),
        ] {
            send(&t, &dir, name, body).await;
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_relative_move_to_relative() {
    let dir = TestDirectory::new("FileConsumeSimpleRelativeMoveToRelativeTest");
    let uri = dir.file_uri("?recursive=true&move=.done&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    for f in [
        ".done/bye.txt",
        "sub/.done/hello.txt",
        "sub/sub2/.done/goodday.txt",
    ] {
        mock.expected_file_exists(dir.test_file(f), None);
    }
    send(&t, &dir, "bye.txt", "Bye World").await;
    send(&t, &dir, "sub/hello.txt", "Hello World").await;
    send(&t, &dir, "sub/sub2/goodday.txt", "Goodday World").await;
    mock.assert_is_satisfied().await;
}

fn kaboom(e: &mut Exchange) -> CamelResult<()> {
    if matches!(e.message().body(), Value::Text(t) if t.starts_with("Kaboom")) {
        return Err(CamelError::illegal_argument("Forced"));
    }
    Ok(())
}

#[tokio::test]
async fn test_delete_and_failure() {
    for move_failed in [false, true] {
        let dir = TestDirectory::new("FileConsumerDeleteAndFailureTest");
        let (error, from_uri) = (
            dir.file_uri("error"),
            dir.file_uri(if move_failed {
                "?delete=true&moveFailed=error&initialDelay=0&delay=10"
            } else {
                "?delete=true&initialDelay=0&delay=10"
            }),
        );
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            if !move_failed {
                r.on_exception(
                    on_exception(ErrorType::Kind(ErrorKind::IllegalArgument))
                        .handled(true)
                        .use_original_message()
                        .to(&error),
                );
            }
            r.add(
                from(&from_uri)
                    .set_body(simple("${body} IS processed!"))
                    .process_fn(kaboom)
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Hello World IS processed!"]);
        mock.expected_file_exists(dir.test_file("error/bye.txt"), None);
        send(&t, &dir, "hello.txt", "Hello World").await;
        send(&t, &dir, "bye.txt", "Kaboom").await;
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_move_file_if_process_fails() {
    let dir = TestDirectory::new("FromFileMoveFileIfProcessFailsTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&moveFailed=error");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&uri)
                .convert_body_to("String")
                .to("mock:foo")
                .process_fn(|_| Err(CamelError::illegal_argument("Forced by unittest"))),
        );
    })
    .await;
    let mock = t.mock("mock:foo");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_file_exists(dir.test_file("error/hello.txt"), Some("Hello World"));
    send(&t, &dir, "hello.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumeNoopIdempotentEnabledTest`, `FileConsumeNoopIdempotentAutoTest` and
/// `FileConsumeNoopIdempotentDisabledTest`.
#[tokio::test]
async fn test_noop_idempotent() {
    for (option, minimum) in [
        ("?noop=true&idempotent=true&initialDelay=0&delay=10", None),
        ("?initialDelay=0&delay=10&noop=true", None),
        (
            "?initialDelay=0&delay=10&noop=true&idempotent=false",
            Some(2),
        ),
    ] {
        let dir = TestDirectory::new("FileConsumeNoopIdempotentTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        match minimum {
            Some(n) => mock.expected_minimum_message_count(n),
            None => mock.expected_message_count(1),
        }
        send(&t, &dir, "hello.txt", "Hello World").await;
        if minimum.is_none() {
            // more polls do not consume the file again
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        mock.assert_is_satisfied().await;
        assert!(dir.test_file("hello.txt").exists());
    }
}

/// Port of `FileConsumerFileNameFilterTest`.
#[tokio::test]
async fn test_file_name_filter() {
    let dir = TestDirectory::new("FileConsumerFileNameFilterTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&fileName=${file:onlyname.noext}.txt");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .to("mock:txt"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:txt")
        .expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "customer.xml", "<customer>123</customer>").await;
    send(&t, &dir, "book.xml", "<book>Camel Rocks</book>").await;
    send(&t, &dir, "bye.txt", "Bye World").await;
    t.context.start_route("foo").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileConsumeSingleDirectoryOnlyTest`.
#[tokio::test]
async fn test_single_directory_only() {
    let dir = TestDirectory::new("FileConsumeSingleDirectoryOnlyTest");
    let uri = dir.file_uri("?recursive=false&delete=true&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
    send(&t, &dir, "2008/report2008.txt", "2008 Report").await;
    send(&t, &dir, "report.txt", "Hello World").await;
    send(&t, &dir, "report2.txt", "Bye World").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileConsumerConsumedFileNameTest`.
#[tokio::test]
async fn test_consumed_file_name() {
    use camel_core::api::exchange_constants::FILE_NAME_CONSUMED;
    let dir = TestDirectory::new("FileConsumerConsumedFileNameTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.message_matches(0, header(FILE_NAME).is_equal_to("hello.123.txt"));
    mock.message_matches(0, header(FILE_NAME_CONSUMED).is_equal_to("hello.123.txt"));
    send(&t, &dir, "hello.123.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
}
