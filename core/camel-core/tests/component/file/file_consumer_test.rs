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

/// Ports of `FileConsumerAbsolutePathTest`, `FileConsumerAbsolutePathWithRelativeMoveTest`,
/// `FileConsumeSimpleAbsoluteMoveToRelativeTest` and `FileConsumeSimpleRelativeMoveToAbsoluteTest`
/// (the test directories are absolute).
#[tokio::test]
async fn test_absolute_paths_and_moves() {
    // delete
    let dir = TestDirectory::new("FileConsumerAbsolutePathTest");
    let uri = dir.file_uri("?initialDelay=0&delay=100&delete=true");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:report"));
    })
    .await;
    t.mock("mock:report")
        .expected_bodies_received(["Hello Paris"]);
    send(&t, &dir, "paris.txt", "Hello Paris").await;
    t.assert_mock_endpoints_satisfied().await;

    // a relative move expression
    let dir = TestDirectory::new("FileConsumerAbsolutePathWithRelativeMoveTest");
    let uri = format!(
        "file://{}/dir?initialDelay=0&delay=10&move=../done/${{file:onlyname}}",
        dir.path()
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:report"));
    })
    .await;
    let mock = t.mock("mock:report");
    mock.expected_bodies_received(["Hello Paris"]);
    mock.expected_file_exists(dir.test_file("dir/../done/paris.txt"), None);
    send(&t, &dir, "dir/paris.txt", "Hello Paris").await;
    mock.assert_is_satisfied().await;

    // a move to an absolute directory
    let dir = TestDirectory::new("FileConsumeSimpleRelativeMoveToAbsoluteTest");
    let base = format!("{}/.done", dir.path());
    let uri = dir.file_uri(&format!(
        "?recursive=true&move={base}&initialDelay=0&delay=10"
    ));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    for f in [".done/bye.txt", ".done/hello.txt", ".done/goodday.txt"] {
        mock.expected_file_exists(dir.test_file(f), None);
    }
    send(&t, &dir, "bye.txt", "Bye World").await;
    send(&t, &dir, "sub/hello.txt", "Hello World").await;
    send(&t, &dir, "sub/sub2/goodday.txt", "Goodday World").await;
    mock.assert_is_satisfied().await;
}

/// Port of `FileConsumerDirectoryNotMatchedTest`.
#[tokio::test]
async fn test_directory_not_matched() {
    let dir = TestDirectory::new("FileConsumerDirectoryNotMatchedTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&recursive=true&include=.*txt$");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    send(&t, &dir, ".skipme", "This is a dot file").await;
    send(&t, &dir, "index.html", "This is a web file").await;
    send(&t, &dir, "2007/report2007.txt", "2007 report").await;
    send(&t, &dir, "2008/report2008.txt", "2008 report").await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(2);
    mock.assert_is_satisfied().await;
}

/// Port of `FileConvertBodyToUTF8Test`.
#[tokio::test]
async fn test_convert_body_to_utf8() {
    let dir = TestDirectory::new("FileConvertBodyToUTF8Test");
    let body = "Hello Thai Elephant \u{0E08}".as_bytes().to_vec();
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    t.template
        .send_body_and_header(&dir.file_uri(""), body.clone(), FILE_NAME, "utf8.txt")
        .await
        .unwrap();
    let uri = dir.file_uri("?initialDelay=0&delay=10");
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.assert_is_satisfied().await;
    let received = mock.received_exchanges();
    assert_eq!(
        body,
        received[0].message().body().to_string().into_bytes(),
        "Should be same byte data"
    );
}

/// Port of `PollEnrichSimpleMoveTest` (the `${file:..}` functions of the uri are evaluated by
/// the file endpoint).
#[tokio::test]
async fn test_poll_enrich_simple_move() {
    let dir = TestDirectory::new("PollEnrichSimpleMoveTest");
    let uri = dir.file_uri("?move=backup/my-${file:onlyname}");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .poll_enrich(&uri, 5000)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("backup/my-hello.txt"), None);
    send(&t, &dir, "hello.txt", "Hello World").await;
    t.template
        .send_body("direct:start", "Trigger")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumeTemplateTest` and `FileConsumerTemplateTest` (a consumer template).
#[tokio::test]
async fn test_consumer_template() {
    let dir = TestDirectory::new("FileConsumeTemplateTest");
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    let consumer = t.context.create_consumer_template();
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "bye.txt", "Bye World").await;
    let uri = dir.file_uri("?sortBy=file:name&eagerMaxMessagesPerPoll=false");
    let out = consumer.receive(&uri).await.unwrap().expect("a file");
    let out2 = consumer.receive(&uri).await.unwrap().expect("another file");
    assert_eq!(&Value::from("Bye World"), out.message().body());
    assert_eq!(&Value::from("Hello World"), out2.message().body());
    consumer.done_uow(out).await;
    consumer.done_uow(out2).await;
    consumer.stop().await.unwrap();

    let dir = TestDirectory::new("FileConsumerTemplateTest");
    send(&t, &dir, "hello.txt", "Hello World").await;
    assert!(dir.test_file("hello.txt").exists(), "File should exist");
    let body = consumer
        .receive_body_timeout(&dir.file_uri("?delete=true"), 5000)
        .await
        .unwrap();
    assert_eq!(Some(Value::from("Hello World")), body);
    assert!(
        !dir.test_file("hello.txt").exists(),
        "File should be deleted"
    );
}

/// Port of `ConsumerTemplateFileShutdownTest`.
#[tokio::test]
async fn test_consumer_template_file_shutdown() {
    let dir = TestDirectory::new("ConsumerTemplateFileShutdownTest");
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    let consumer = t.context.create_consumer_template();
    send(&t, &dir, "hello.123.txt", "Hello World").await;
    let exchange = consumer
        .receive_timeout(&dir.file_uri("?fileName=hello.123.txt"), 5000)
        .await
        .unwrap()
        .expect("the file");
    assert_eq!(&Value::from("Hello World"), exchange.message().body());
    consumer.done_uow(exchange).await;
    consumer.stop().await.unwrap();
}
