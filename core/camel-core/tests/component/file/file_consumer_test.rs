//! Ports of `FileConsumerProducerRouteTest`, `FileConsumerIncludeExtTest`,
//! `FileConsumerExcludeExtTest`, `FileConsumeSimpleRelativeMoveToRelativeTest`,
//! `FileConsumerDeleteAndFailureTest`, `FileConsumerDeleteAndMoveFailedTest` and
//! `FromFileMoveFileIfProcessFailsTest`.

use crate::support::{await_until, ContextTestSupport, TestDirectory};
use camel_core::api::error::ErrorType;
use camel_core::api::exchange_constants::{
    FILE_LAST_MODIFIED, FILE_LENGTH, FILE_NAME, MESSAGE_TIMESTAMP,
};
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

/// Ports of `FileConsumerPreMoveTest`, `FileConsumerPreMoveDeleteTest` and
/// `FileConsumerPreMoveNoopTest` (each sends the same file twice).
#[tokio::test]
async fn test_pre_move() {
    for (option, work, after) in [
        (
            "?preMove=work/work-${file:name}&initialDelay=0&delay=10",
            "work/work-hello.txt",
            None,
        ),
        (
            "?preMove=work&delete=true&idempotent=false&initialDelay=0&delay=10",
            "work/hello.txt",
            Some(false),
        ),
        (
            "?preMove=work&noop=true&idempotent=false&initialDelay=0&delay=10",
            "work/hello.txt",
            Some(true),
        ),
    ] {
        let dir = TestDirectory::new("FileConsumerPreMoveTest");
        let uri = dir.file_uri(option);
        let pre_moved = dir.test_file(work);
        let checked = pre_moved.clone();
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let checked = checked.clone();
            r.add(
                from(&uri)
                    .process_fn(move |_| {
                        assert!(checked.exists(), "Pre move file should exist");
                        Ok(())
                    })
                    .to("mock:result"),
            );
        })
        .await;
        for body in ["Hello World", "Hello Again World"] {
            let mock = t.mock("mock:result");
            mock.reset();
            mock.expected_bodies_received([body]);
            send(&t, &dir, "hello.txt", body).await;
            mock.assert_is_satisfied().await;
            if let Some(exists) = after {
                support_wait(|| pre_moved.exists() == exists).await;
                assert_eq!(exists, pre_moved.exists(), "{option}");
            }
        }
    }
}

async fn support_wait(condition: impl Fn() -> bool) {
    crate::support::await_until(std::time::Duration::from_secs(5), condition).await;
}

/// Port of `FileConsumeMaxMessagesPerPollTest`.
#[tokio::test]
async fn test_max_messages_per_poll() {
    use camel_core::api::exchange_constants::BATCH_SIZE;
    let dir = TestDirectory::new("FileConsumeMaxMessagesPerPollTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&maxMessagesPerPoll=2");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(2);
    mock.message_matches(0, exchange_property(BATCH_SIZE).is_equal_to(2));
    mock.message_matches(1, exchange_property(BATCH_SIZE).is_equal_to(2));
    send(&t, &dir, "bye.txt", "Bye World").await;
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "godday.txt", "Godday World").await;
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumerIdempotentTest`, `FileConsumerIdempotentKeyTest` and
/// `FileConsumerIdempotentKeyNameAndSizeTest`: a file moved back
/// after being consumed is not consumed again.
#[tokio::test]
async fn test_idempotent() {
    for key in [
        "",
        "&idempotentKey=${file:onlyname}",
        "&idempotentKey=${file:onlyname}-${file:size}",
    ] {
        let dir = TestDirectory::new("FileConsumerIdempotentTest");
        let uri = dir.file_uri(&format!(
            "?idempotent=true{key}&move=done/${{file:name}}&initialDelay=0&delay=10"
        ));
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Hello World"]);
        send(&t, &dir, "report.txt", "Hello World").await;
        mock.assert_is_satisfied().await;
        let done = dir.test_file("done/report.txt");
        support_wait(|| done.exists()).await;
        mock.reset();
        mock.expected_message_count(0);
        std::fs::rename(&done, dir.test_file("report.txt")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        mock.assert_is_satisfied().await;
        assert!(dir.test_file("report.txt").exists());
    }
}

/// Port of `FileIdempotentReadSameFileAgainTest`.
#[tokio::test]
async fn test_idempotent_read_same_file_again() {
    let dir = TestDirectory::new("FileIdempotentReadSameFileAgainTest");
    let uri = dir.file_uri(
        "?idempotent=false&move=../done&moveFailed=../error&preMove=working/${date:now:yyyyMMddHHmmssSSS}-${file:name}&readLock=none&initialDelay=0&delay=10",
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(["Hello World", "Foo"]);
    send(&t, &dir, "foo.txt", "Hello World").await;
    send(&t, &dir, "bar.txt", "Foo").await;
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["Bye World"]);
    send(&t, &dir, "foo.txt", "Bye World").await;
    mock.assert_is_satisfied().await;
}

/// Port of `FileConsumeCharsetTest`.
#[tokio::test]
async fn test_consume_charset() {
    let dir = TestDirectory::new("FileConsumeCharsetTest");
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    t.template
        .send_body_and_header(
            &dir.file_uri("?charset=UTF-8"),
            "Hello World \u{4f60}\u{597d}",
            FILE_NAME,
            "report.txt",
        )
        .await
        .unwrap();
    let uri =
        dir.file_uri("?initialDelay=0&delay=10&fileName=report.txt&delete=true&charset=UTF-8");
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World \u{4f60}\u{597d}"]);
    mock.assert_is_satisfied().await;
    let f = dir.test_file("report.txt");
    support_wait(|| !f.exists()).await;
    assert!(!f.exists(), "File should been deleted");
}

/// Ports of `FileConsumeWithDollarInPathTest` and `FileConsumerPathWithAmpersandTest`.
#[tokio::test]
async fn test_special_characters_in_path() {
    for (sub, option) in [
        ("edi$/dev1", "?initialDelay=0&delay=10"),
        ("file&stuff", "?delete=true&initialDelay=0&delay=10"),
    ] {
        let dir = TestDirectory::new("FileConsumeWithDollarInPathTest");
        let uri = format!("{}{option}", dir.file_uri(sub));
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        t.mock("mock:result").expected_message_count(1);
        send(&t, &dir, &format!("{sub}/hello.txt"), "Hello World").await;
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `FileMoveErrorOnExceptionNotHandledTest`.
#[tokio::test]
async fn test_move_error_on_exception_not_handled() {
    let dir = TestDirectory::new("FileMoveErrorOnExceptionNotHandledTest");
    let uri = dir.file_uri("?moveFailed=error&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&uri)
                .on_exception(
                    on_exception(ErrorType::Kind(ErrorKind::IllegalArgument)).to("mock:damn"),
                )
                .to("mock:before")
                .throw_exception(CamelError::illegal_argument("Damn"))
                .to("mock:after"),
        );
    })
    .await;
    t.mock("mock:before").expected_message_count(1);
    t.mock("mock:after").expected_message_count(0);
    let damn = t.mock("mock:damn");
    damn.expected_message_count(1);
    damn.expected_file_exists(dir.test_file("error/hello.txt"), None);
    send(&t, &dir, "hello.txt", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FromFileMoveDoneCustomHeaderTest`.
#[tokio::test]
async fn test_move_done_custom_header() {
    let dir = TestDirectory::new("FromFileMoveDoneCustomHeaderTest");
    let (inbox, outbox) = (
        dir.file_uri("inbox?initialDelay=0&delay=10&move=${header.bar}"),
        dir.file_uri("outbox"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&inbox)
                .set_header("bar", constant("dones/mydone.txt"))
                .transform(constant("Bye World"))
                .to("mock:result")
                .to(&outbox),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("outbox/hello.txt"), None);
    mock.expected_file_exists(dir.test_file("inbox/dones/mydone.txt"), None);
    send(&t, &dir, "inbox/hello.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumerNoopTest` and `FileConsumerIncludeNameTest`.
#[tokio::test]
async fn test_noop_and_include_name() {
    let dir = TestDirectory::new("FileConsumerNoopTest");
    let uri = dir.file_uri("?noop=true&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    t.mock("mock:result").expected_message_count(2);
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "bye.txt", "Bye World").await;
    t.assert_mock_endpoints_satisfied().await;
    assert!(dir.test_file("hello.txt").exists());
    assert!(dir.test_file("bye.txt").exists());

    let dir = TestDirectory::new("FileConsumerIncludeNameTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&include=^report.*txt$");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(["Reports1", "Reports2", "Reports3"]);
    mock.expected_message_count(3);
    for (name, body) in [
        ("hello.xml", "Hello World"),
        ("report1.txt", "Reports1"),
        ("secret.txt", "Bye World"),
        ("report2.txt", "Reports2"),
        ("Report3.txt", "Reports3"),
        ("Secret2.txt", "Secret2"),
    ] {
        send(&t, &dir, name, body).await;
    }
    mock.assert_is_satisfied().await;
}

/// One file route with the options, the files sent before starting it, the expected bodies
/// in any order and the files expected to exist.
async fn consume(
    name: &str,
    option: &str,
    files: &[(&str, &str)],
    bodies: &[&str],
    exist: &[&str],
) {
    let dir = TestDirectory::new(name);
    let uri = dir.file_uri(option);
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .process_fn(kaboom)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(bodies.iter().copied());
    for f in exist {
        mock.expected_file_exists(dir.test_file(f), None);
    }
    for (file, body) in files {
        send(&t, &dir, file, body).await;
    }
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumerAbsolutePathDefaultMoveTest`, `FileConsumerMoveAndMoveFailureTest`,
/// `FileConsumerIncludeAndExcludeNameTest`, `FileConsumeHiddenFilesTest` and
/// `FileConsumeMoveRelativeNameTest`.
#[tokio::test]
async fn test_consume_options() {
    consume(
        "FileConsumerAbsolutePathDefaultMoveTest",
        "?initialDelay=0&delay=10",
        &[("paris.txt", "Hello Paris")],
        &["Hello Paris"],
        &[".camel/paris.txt"],
    )
    .await;
    consume(
        "FileConsumerMoveAndMoveFailureTest",
        "?move=moved&initialDelay=0&delay=10&moveFailed=error/${file:name.noext}-error.txt",
        &[("hello.txt", "Hello World"), ("bye.txt", "Kaboom")],
        &["Hello World"],
        &["moved/hello.txt", "error/bye-error.txt"],
    )
    .await;
    consume(
        "FileConsumerIncludeAndExcludeNameTest",
        "?initialDelay=0&delay=10&include=report.*txt&exclude=hello.*",
        &[
            ("hello.txt", "Hello World"),
            ("report1.xml", "Report 1"),
            ("report2.txt", "Report 2"),
            ("report3.txt", "Report 3"),
            ("Report4.txt", "Report 4"),
            ("Secret.txt", "Secret"),
        ],
        &["Report 2", "Report 3", "Report 4"],
        &[],
    )
    .await;
    consume(
        "FileConsumeHiddenFilesTest",
        "?initialDelay=0&delay=10&delete=true&includeHiddenFiles=true",
        &[
            ("report.txt", "Report 123"),
            (".report.hidden", "Hidden report 123"),
        ],
        &["Report 123", "Hidden report 123"],
        &[],
    )
    .await;
    consume(
        "FileConsumeMoveRelativeNameTest",
        "?initialDelay=0&delay=10&recursive=true&move=.done/${file:name}.old",
        &[
            ("bye.txt", "Bye World"),
            ("sub/hello.txt", "Hello World"),
            ("sub/sub2/goodday.txt", "Goodday World"),
        ],
        &["Bye World", "Hello World", "Goodday World"],
        &[
            ".done/bye.txt.old",
            ".done/sub/hello.txt.old",
            ".done/sub/sub2/goodday.txt.old",
        ],
    )
    .await;
}

/// Port of `FileConsumerBatchTest`.
#[tokio::test]
async fn test_consume_batch() {
    use camel_core::api::exchange_constants::{BATCH_INDEX, BATCH_SIZE};
    let dir = TestDirectory::new("FileConsumerBatchTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
    mock.message_matches(0, exchange_property(BATCH_SIZE).is_equal_to(2));
    mock.message_matches(0, exchange_property(BATCH_INDEX).is_equal_to(0));
    mock.message_matches(1, exchange_property(BATCH_INDEX).is_equal_to(1));
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "bye.txt", "Bye World").await;
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FilePollEnrichTest`.
#[tokio::test]
async fn test_file_poll_enrich() {
    let dir = TestDirectory::new("FilePollEnrichTest");
    let uri = dir.file_uri("?move=done");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("timer:foo?period=1000&delay=100")
                .route_id("foo")
                .log("Trigger timer foo")
                .poll_enrich(&uri, 5000)
                .convert_body_to("String")
                .log("Polled filed ${file:name}")
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_file_exists(dir.test_file("done/hello.txt"), None);
    send(&t, &dir, "hello.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    assert!(
        !dir.test_file("hello.txt").exists(),
        "File should have been moved"
    );
    t.context.stop().await.unwrap();
}

/// Port of `FileConsumerIdempotentOnExceptionHandledTest`: a handled failure moves the file.
#[tokio::test]
async fn test_idempotent_on_exception_handled() {
    let dir = TestDirectory::new("FileConsumerIdempotentOnExceptionHandledTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&idempotent=true");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.on_exception(
            on_exception(ErrorType::Any)
                .handled(true)
                .to("mock:invalid"),
        );
        r.add(
            from(&uri)
                .to("mock:input")
                .throw_exception(CamelError::illegal_argument("Forced")),
        );
    })
    .await;
    let mock = t.mock("mock:invalid");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file(".camel/hello.txt"), None);
    send(&t, &dir, "hello.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    assert!(!dir.test_file("hello.txt").exists());
}

/// Port of `FileConsumerIdempotentKeyNameAndSizeTest.testIdempotentDiffSize`: the same name
/// with another size is consumed again.
#[tokio::test]
async fn test_idempotent_key_name_and_size_diff_size() {
    let dir = TestDirectory::new("FileConsumerIdempotentKeyNameAndSizeTest");
    let uri = dir.file_uri(
        "?idempotent=true&idempotentKey=${file:onlyname}-${file:size}&move=done/${file:name}&initialDelay=0&delay=10",
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    send(&t, &dir, "report.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    let done = dir.test_file("done/report.txt");
    support_wait(|| done.exists()).await;
    mock.reset();
    mock.expected_bodies_received(["Bye World"]);
    send(&t, &dir, "report.txt", "Bye World").await;
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumerExcludeNameTest`, `FileConsumeSimpleAbsoluteMoveToAbsoluteTest` and
/// `FileConsumerAbsolutePathWithAbsoluteMoveTest`.
#[tokio::test]
async fn test_exclude_name_and_absolute_moves() {
    consume(
        "FileConsumerExcludeNameTest",
        "?initialDelay=0&delay=10&exclude=^secret.*|.*xml$",
        &[
            ("hello.xml", "Hello World"),
            ("report1.txt", "Reports1"),
            ("secret.txt", "Bye World"),
            ("report2.txt", "Reports2"),
            ("Report3.txt", "Reports3"),
            ("Secret2.txt", "Secret2"),
        ],
        &["Reports1", "Reports2", "Reports3"],
        &[],
    )
    .await;

    let dir = TestDirectory::new("FileConsumeSimpleAbsoluteMoveToAbsoluteTest");
    let base = dir.path();
    let uri = format!("file://{base}?recursive=true&move={base}/.done&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    for f in ["bye.txt", "hello.txt", "goodday.txt"] {
        mock.expected_file_exists(dir.test_file(&format!(".done/{f}")), None);
    }
    send(&t, &dir, "bye.txt", "Bye World").await;
    send(&t, &dir, "sub/hello.txt", "Hello World").await;
    send(&t, &dir, "sub/sub2/goodday.txt", "Goodday World").await;
    mock.assert_is_satisfied().await;

    let dir = TestDirectory::new("FileConsumerAbsolutePathWithAbsoluteMoveTest");
    let uri = dir.file_uri(&format!(
        "?initialDelay=0&delay=10&move={}/done/${{file:onlyname}}",
        dir.path()
    ));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:report"));
    })
    .await;
    let mock = t.mock("mock:report");
    mock.expected_bodies_received(["Hello Paris"]);
    mock.expected_file_exists(dir.test_file("done/paris.txt"), None);
    send(&t, &dir, "paris.txt", "Hello Paris").await;
    mock.assert_is_satisfied().await;
}

/// Port of `FileConsumeFileAsDirectoryTest` (with an absolute file).
#[tokio::test]
async fn test_consume_file_as_directory() {
    let dir = TestDirectory::new("FileConsumeFileAsDirectoryTest");
    dir.write("as-directory.json", "{}");
    let file = dir
        .test_file("as-directory.json")
        .to_string_lossy()
        .into_owned();
    let uri = format!("file:{file}?initialDelay=0&delay=10&noop=true");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).to("mock:result"));
        })
        .await
        .unwrap();
    let e = t.context.start().await.unwrap_err();
    let msg = format!("{e} {}", crate::support::cause_of(&e));
    assert!(
        msg.contains(&format!("{file} is a file, not a directory")),
        "{msg}"
    );
    assert!(
        msg.contains(&format!("file:{}?fileName=as-directory.json", dir.path())),
        "{msg}"
    );
}

/// Port of `FileConsumerInterceptEmptyFileTest`.
#[tokio::test]
async fn test_intercept_empty_file() {
    let dir = TestDirectory::new("FileConsumerInterceptEmptyFileTest");
    let uri = dir.file_uri("?initialDelay=10&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.intercept_from(None, |i| {
            i.on_when(simple("${file:length} == 0"))
                .to("mock:skip")
                .stop()
        });
        r.add(
            from(&uri)
                .convert_body_to("String")
                .to("log:test")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
    t.mock("mock:skip").expected_message_count(2);
    send(&t, &dir, "hello.xml", "Hello World").await;
    send(&t, &dir, "empty1.txt", "").await;
    send(&t, &dir, "secret.txt", "Bye World").await;
    send(&t, &dir, "empty2.txt", "").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `PollSimpleMoveTest`.
#[tokio::test]
async fn test_poll_simple_move() {
    let dir = TestDirectory::new("PollSimpleMoveTest");
    let uri = dir.file_uri("?move=backup/my-${file:onlyname}");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .poll_timeout(&uri, 5000)
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

/// Ports of `FileConsumerCommitRenameAbsolutePathStrategyTest` and
/// `FileRenameFileOnCommitIssueTest`.
#[tokio::test]
async fn test_commit_rename() {
    let dir = TestDirectory::new("FileConsumerCommitRenameAbsolutePathStrategyTest");
    let uri = dir.file_uri(&format!(
        "reports?move={}/done/${{file:name}}&initialDelay=0&delay=10",
        dir.path()
    ));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:report"));
    })
    .await;
    let mock = t.mock("mock:report");
    mock.expected_bodies_received(["Hello Paris"]);
    mock.expected_file_exists(dir.test_file("done/paris.txt"), Some("Hello Paris"));
    send(&t, &dir, "reports/paris.txt", "Hello Paris").await;
    mock.assert_is_satisfied().await;

    let dir = TestDirectory::new("FileRenameFileOnCommitIssueTest");
    let uri = dir.file_uri("renameissue?noop=false&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&uri)
                .set_property("PartitionID", simple("${file:name}"))
                .convert_body_to("String")
                .to_with_pattern(ExchangePattern::InOut, "direct:source")
                .log("The exchange's IN body as String is ${body}")
                .to("mock:result"),
        );
        r.add(from("direct:source").transform(body().prepend("Hello ")));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("renameissue/.camel/hello.txt"), None);
    send(&t, &dir, "renameissue/hello.txt", "World").await;
    mock.assert_is_satisfied().await;
}

/// Port of `FileConsumerIdleMessageTest`.
#[tokio::test]
async fn test_idle_message() {
    let dir = TestDirectory::new("FileConsumerIdleMessageTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&sendEmptyMessageWhenIdle=true");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_minimum_message_count(2);
    mock.assert_is_satisfied().await;
    let exchanges = mock.received_exchanges();
    assert!(exchanges[0].message().body().is_null());
    assert!(exchanges[1].message().body().is_null());
}

/// Ports of `FileConsumerPreMoveLastModifiedTest` and `FileConsumerMessageTimestampTest`.
#[tokio::test]
async fn test_pre_move_last_modified_and_message_timestamp() {
    let dir = TestDirectory::new("FileConsumerPreMoveLastModifiedTest");
    let (a, b) = (
        dir.file_uri(
            "a?preMove=work/work-${file:name}&initialDelay=0&delay=10&keepLastModified=true",
        ),
        dir.file_uri("b?noop=true&initialDelay=0&delay=10"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&a)
                .process_fn(|e| {
                    let long = |k| e.message().header(k).and_then(|v| v.as_i64()).unwrap_or(0);
                    assert!(long(FILE_LAST_MODIFIED) > 0);
                    assert!(long(FILE_LENGTH) > 0);
                    Ok(())
                })
                .log("Got file ${file:name} modified=${file:modified}")
                .to("mock:result"),
        );
        r.add(from(&b).convert_body_to("String").to("mock:timestamp"));
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    let mock = t.mock("mock:timestamp");
    mock.expected_message_count(1);
    mock.message_matches(0, header(MESSAGE_TIMESTAMP).is_greater_than(0));
    send(&t, &dir, "a/hello.txt", "Hello World").await;
    send(&t, &dir, "b/hello.txt", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileMoveAndMoveFailedIssueTest`.
#[tokio::test]
async fn test_move_and_move_failed_issue() {
    for (name, content, count, moved) in [
        (
            "hello.txt",
            "Hello World",
            1,
            "input.bak/somedate/hello.txt",
        ),
        ("bomb.txt", "Kaboom", 0, "input.err/somedate/bomb.txt"),
    ] {
        let dir = TestDirectory::new("FileMoveAndMoveFailedIssueTest");
        let uri = format!(
            "file://{}/input?move=${{file:parent}}.bak/somedate/${{file:onlyname}}&moveFailed=${{file:parent}}.err/somedate/${{file:onlyname}}&initialDelay=0&delay=10",
            dir.path()
        );
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .convert_body_to("String")
                    .filter(body().contains("Kaboom"), |s| {
                        s.throw_exception(CamelError::illegal_argument("Forced"))
                    })
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_message_count(count);
        send(&t, &dir, &format!("input/{name}"), content).await;
        let f = dir.test_file(moved);
        support_wait(|| f.exists()).await;
        assert!(f.exists(), "{moved}");
        mock.assert_is_satisfied().await;
    }
}

/// Port of `FileProducerNoForcedWritesTest`.
#[tokio::test]
async fn test_no_forced_writes() {
    let dir = TestDirectory::new("FileProducerNoForcedWritesTest");
    let (from_uri, a, b) = (
        dir.file_uri("?initialDelay=0&delay=10&noop=true"),
        dir.file_uri("?fileName=output.txt&forceWrites=false"),
        dir.file_uri("?fileName=output2.txt&charset=iso-8859-1&forceWrites=false"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&from_uri)
                .multicast(|s| s.to(&a).to(&b))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    send(&t, &dir, "hello.txt", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(Some("Hello World".to_string()), dir.read("output.txt"));
    assert_eq!(Some("Hello World".to_string()), dir.read("output2.txt"));
}

/// Ports of `FileConsumeFilesAndDeleteTest`, `FileConsumeHiddenDirsTest` and
/// `FileConsumerSkipDotFilesTest`.
#[tokio::test]
async fn test_delete_hidden_dirs_and_dot_files() {
    consume(
        "FileConsumeFilesAndDeleteTest",
        "?initialDelay=0&delay=2000&fileName=report.txt&delete=true",
        &[
            ("report2.txt", "Bye World"),
            ("report.txt", "Hello World"),
            ("2008/report2008.txt", "2008 Report"),
        ],
        &["Hello World"],
        &[],
    )
    .await;
    consume(
        "FileConsumeHiddenDirsTest",
        "?initialDelay=0&delay=10&delete=true&includeHiddenDirs=true&recursive=true",
        &[
            (".hidden/report1.txt", "Report 123"),
            ("obvious/report2.txt", "Report 456"),
        ],
        &["Report 123", "Report 456"],
        &[],
    )
    .await;
    consume(
        "FileConsumerSkipDotFilesTest",
        "?initialDelay=0&delay=10",
        &[
            (".skipme", "This is a dot file"),
            ("hello.txt", "Hello World"),
        ],
        &["Hello World"],
        &[],
    )
    .await;
}

/// Ports of `FromFileDoNotDeleteFileIfProcessFailsTest` and
/// `FromFileDoNotMoveFileIfProcessFailsTest`.
#[tokio::test]
async fn test_do_not_delete_or_move_if_process_fails() {
    for option in [
        "?initialDelay=0&delay=10&delete=true",
        "?initialDelay=0&delay=10&move=done",
    ] {
        let dir = TestDirectory::new("FromFileDoNotDeleteFileIfProcessFailsTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.on_exception(
                on_exception(ErrorType::Kind(ErrorKind::IllegalArgument)).to("mock:error"),
            );
            r.add(
                from(&uri).process_fn(|_| Err(CamelError::illegal_argument("Forced by unittest"))),
            );
        })
        .await;
        let mock = t.mock("mock:error");
        mock.expected_minimum_message_count(1);
        send(
            &t,
            &dir,
            "hello.txt",
            "Hello World this file will NOT be deleted",
        )
        .await;
        mock.assert_is_satisfied().await;
        t.context.stop().await.unwrap();
        assert!(dir.test_file("hello.txt").exists(), "{option}");
    }
}

/// Port of `FileAbsolutePathIssueTest`.
#[tokio::test]
async fn test_absolute_path_issue() {
    let dir = TestDirectory::new("FileAbsolutePathIssueTest");
    let (start, done) = (
        format!("{}/issue", dir.path()),
        format!("{}/done", dir.path()),
    );
    let uri = format!("file:{start}?initialDelay=0&delay=10&move={done}/${{file:name}}");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("done/hello.txt"), None);
    t.template
        .send_body_and_header(
            &format!("file:{start}"),
            "Hello World",
            FILE_NAME,
            "hello.txt",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileConsumerIdempotentKeyHeaderMutationTest`: a route changing the file name
/// header does not change the idempotent key released on failure.
#[tokio::test]
async fn test_idempotent_key_header_mutation() {
    let dir = TestDirectory::new("FileConsumerIdempotentKeyHeaderMutationTest");
    let uri = dir.file_uri(
        "?idempotent=true&idempotentKey=${file:onlyname}-${file:size}&initialDelay=0&delay=10",
    );
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let attempts = attempts.clone();
        r.add(
            from(&uri)
                .set_header(FILE_NAME, constant("output.txt"))
                .process_fn(move |_| {
                    if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        return Err(CamelError::illegal_argument(
                            "Simulated failure on first attempt",
                        ));
                    }
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    send(&t, &dir, "report.txt", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FilePollEnrichNoWaitTest`.
#[tokio::test]
async fn test_file_poll_enrich_no_wait() {
    let dir = TestDirectory::new("FilePollEnrichNoWaitTest");
    dir.write("hello.txt", "Hello World");
    let uri = dir.file_uri("?initialDelay=0&delay=10&move=done");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("timer:foo?delay=0&period=10")
                .route_id("foo")
                .log("Trigger timer foo")
                .poll_enrich(&uri, 0)
                .convert_body_to("String")
                .filter(body().is_null(), |s| s.stop())
                .log("Polled filed ${file:name}")
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_file_exists(dir.test_file("done/hello.txt"), None);
    mock.set_result_wait_time(2000);
    mock.assert_is_satisfied().await;
    assert!(
        !dir.test_file("hello.txt").exists(),
        "File should have been moved"
    );
    t.context.stop().await.unwrap();
}

/// Port of `FileConsumerPreMoveIssueTest`.
#[tokio::test]
async fn test_pre_move_issue() {
    let dir = TestDirectory::new("FileConsumerPreMoveIssueTest");
    let uri = dir
        .file_uri("?preMove=before/${file:name.noext}-moved.${file:ext}&initialDelay=0&delay=10");
    let moved = dir.test_file("before/hello-moved.txt");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let moved = moved.clone();
        r.add(
            from(&uri)
                .process_fn(move |_| {
                    assert!(moved.exists(), "Pre move file should exist");
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result").expected_message_count(1);
    send(&t, &dir, "hello.txt", "Hello World").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileRecursiveNoopTest`.
#[tokio::test]
async fn test_recursive_noop() {
    let dir = TestDirectory::new("FileRecursiveNoopTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&recursive=true&noop=true");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    for (name, body) in [
        ("a.txt", "a"),
        ("b.txt", "b"),
        ("foo/a.txt", "a2"),
        ("bar/c.txt", "c"),
        ("bar/b.txt", "b2"),
    ] {
        send(&t, &dir, name, body).await;
    }
    t.context.start_route("foo").await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received_in_any_order(["a", "b", "a2", "c", "b2"]);
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["c2"]);
    send(&t, &dir, "c.txt", "c2").await;
    mock.assert_is_satisfied().await;
}

/// Port of `FileLanguageCBRTest`.
#[tokio::test]
async fn test_file_language_cbr() {
    for (name, body, target) in [
        ("hello.txt", "Hello World", "mock:txt"),
        ("bye.dat", "Bye World", "mock:dat"),
        ("hi.foo", "Hi World", "mock:other"),
    ] {
        let dir = TestDirectory::new("FileLanguageCBRTest");
        let uri = dir.file_uri("?delete=true&initialDelay=0&delay=10");
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").choice(|c| {
                c.when(simple("${file:ext} == 'txt'"), |w| w.to("mock:txt"))
                    .when(simple("${file:ext} == 'dat'"), |w| w.to("mock:dat"))
                    .otherwise(|o| o.to("mock:other"))
            }));
        })
        .await;
        for m in ["mock:txt", "mock:dat", "mock:other"] {
            t.mock(m).expected_message_count(i32::from(m == target));
        }
        send(&t, &dir, name, body).await;
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `FromFilePollThirdTimeOkTest`.
#[tokio::test]
async fn test_poll_third_time_ok() {
    let dir = TestDirectory::new("FromFilePollThirdTimeOkTest");
    let uri = dir.file_uri("?delete=true&initialDelay=0&delay=10");
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (c, file) = (counter.clone(), dir.test_file("hello.txt"));
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            let (c, file) = (c.clone(), file.clone());
            r.add(
                from(&uri)
                    .auto_startup(false)
                    .route_id("FromFilePollThirdTimeOkTest")
                    .process_fn(move |_| {
                        if c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 < 3 {
                            assert!(file.exists());
                            return Err(CamelError::illegal_argument("Forced by unittest"));
                        }
                        Ok(())
                    })
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let body = "Hello World this file will be deleted";
    send(&t, &dir, "hello.txt", body).await;
    t.context
        .start_route("FromFilePollThirdTimeOkTest")
        .await
        .unwrap();
    t.mock("mock:result").expected_bodies_received([body]);
    t.assert_mock_endpoints_satisfied().await;
    let f = dir.test_file("hello.txt");
    support_wait(|| !f.exists()).await;
    assert_eq!(3, counter.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!f.exists());
}

/// Port of `FileAbsoluteAndRelativeConsumerTest`.
#[tokio::test]
async fn test_absolute_and_relative_consumer() {
    use camel_core::api::exchange_constants::FILE_NAME_ONLY;
    let dir = TestDirectory::new("FileAbsoluteAndRelativeConsumerTest");
    let (relative, absolute) = (
        dir.file_uri("filerelative?initialDelay=0&delay=10&recursive=true"),
        format!(
            "file://{}/fileabsolute?initialDelay=0&delay=10&recursive=true",
            dir.path()
        ),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&relative)
                .convert_body_to("String")
                .to("mock:relative"),
        );
        r.add(
            from(&absolute)
                .convert_body_to("String")
                .to("mock:absolute"),
        );
    })
    .await;
    for kind in ["relative", "absolute"] {
        let mock = t.mock(&format!("mock:{kind}"));
        mock.expected_message_count(1);
        mock.message_matches(0, header(FILE_NAME).is_equal_to("test/hello.txt"));
        mock.message_matches(0, header(FILE_NAME_ONLY).is_equal_to("hello.txt"));
        send(
            &t,
            &dir,
            &format!("file{kind}/test/hello.txt"),
            "Hello World",
        )
        .await;
    }
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileConsumePollEnrichFileTest`.
#[tokio::test]
async fn test_consume_poll_enrich_file() {
    let dir = TestDirectory::new("FileConsumePollEnrichFileTest");
    let (enrich, data) = (
        dir.file_uri("enrich?initialDelay=0&delay=10&move=.done"),
        dir.file_uri("enrichdata?initialDelay=0&delay=10&move=.done"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&enrich)
                .to("mock:start")
                .poll_enrich(&data, 5000)
                .to("mock:result"),
        );
    })
    .await;
    let start = t.mock("mock:start");
    start.expected_bodies_received(["Start"]);
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Big file"]);
    mock.expected_file_exists(dir.test_file("enrich/.done/AAA.fin"), None);
    mock.expected_file_exists(dir.test_file("enrichdata/.done/AAA.dat"), None);
    send(&t, &dir, "enrich/AAA.fin", "Start").await;
    support_wait(|| start.received_counter() >= 1).await;
    send(&t, &dir, "enrichdata/AAA.dat", "Big file").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Ports of `FileConsumerMoveFailureTest` and `FileConsumerMoveFailureOnCompletionTest`.
#[tokio::test]
async fn test_move_failure() {
    for on_completion in [false, true] {
        let dir = TestDirectory::new("FileConsumerMoveFailureTest");
        let uri =
            dir.file_uri("?initialDelay=0&delay=10&moveFailed=error/${file:name.noext}-error.txt");
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let mut route = from(&uri);
            if on_completion {
                route = route.on_completion(|c| c.on_failure_only().to("mock:failed"));
            }
            r.add(
                route
                    .process_fn(kaboom)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Hello World"]);
        mock.expected_file_exists(dir.test_file(".camel/hello.txt"), Some("Hello World"));
        mock.expected_file_exists(dir.test_file("error/bye-error.txt"), Some("Kaboom"));
        if on_completion {
            t.mock("mock:failed").expected_message_count(1);
        }
        send(&t, &dir, "hello.txt", "Hello World").await;
        send(&t, &dir, "bye.txt", "Kaboom").await;
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// A file is consumed only once its done file exists; the files expected to exist (`true`)
/// or not afterwards.
async fn done_file(name: &str, option: &str, done: &str, after: &[(&str, bool)]) {
    let dir = TestDirectory::new(name);
    let uri = dir.file_uri(option);
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(0);
    send(&t, &dir, "hello.txt", "Hello World").await;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["Hello World"]);
    send(&t, &dir, done, "").await;
    mock.assert_is_satisfied().await;
    for (file, exists) in after {
        let f = dir.test_file(file);
        support_wait(|| f.exists() == *exists).await;
        assert_eq!(*exists, f.exists(), "{name}: {file}");
    }
}

/// Ports of `FilerConsumerDoneFileNameTest`, `FilerConsumerDoneFileNameDeleteTest`,
/// `FilerConsumerDoneFileNamePrefixTest`, `FilerConsumerDoneFileNameSimplePrefixTest`,
/// `FilerConsumerDoneFileNameSuffixTest` and `FilerConsumerDoneFileNoopTest`.
#[tokio::test]
async fn test_done_file_name() {
    done_file(
        "FilerConsumerDoneFileNameTest",
        "?doneFileName=done&initialDelay=0&delay=10",
        "done",
        &[("done", false), (".camel/hello.txt", true)],
    )
    .await;
    done_file(
        "FilerConsumerDoneFileNameDeleteTest",
        "?doneFileName=done&delete=true&initialDelay=0&delay=10",
        "done",
        &[("done", false), ("hello.txt", false)],
    )
    .await;
    done_file(
        "FilerConsumerDoneFileNamePrefixTest",
        "?doneFileName=done-${file:name}&initialDelay=0&delay=10",
        "done-hello.txt",
        &[("done-hello.txt", false)],
    )
    .await;
    done_file(
        "FilerConsumerDoneFileNameSimplePrefixTest",
        "?doneFileName=done-$simple{file:name}&initialDelay=0&delay=10",
        "done-hello.txt",
        &[("done-hello.txt", false)],
    )
    .await;
    done_file(
        "FilerConsumerDoneFileNameSuffixTest",
        "?doneFileName=${file:name}.ready&initialDelay=0&delay=10",
        "hello.txt.ready",
        &[("hello.txt.ready", false)],
    )
    .await;
    done_file(
        "FilerConsumerDoneFileNoopTest",
        "?initialDelay=0&delay=10&doneFileName=done&noop=true",
        "done",
        &[("done", true), ("hello.txt", true)],
    )
    .await;
}

/// Port of `FileConsumeDoneFileIssueTest`: a static done file is deleted with the last file
/// of the batch, a dynamic one with its file.
#[tokio::test]
async fn test_consume_done_file_issue() {
    for (option, files, done) in [
        (
            "?doneFileName=foo.done&initialDelay=0&delay=10",
            vec![
                ("foo-a.txt", "A"),
                ("foo-b.txt", "B"),
                ("foo-c.txt", "C"),
                ("foo-d.txt", "D"),
                ("foo-e.txt", "E"),
            ],
            vec!["foo.done"],
        ),
        (
            "?doneFileName=${file:name}.done&initialDelay=0&delay=10",
            vec![("a.txt", "A"), ("b.txt", "B"), ("c.txt", "C")],
            vec!["a.txt.done", "b.txt.done", "c.txt.done"],
        ),
        (
            "?doneFileName=${file:name}.done&initialDelay=0&delay=10",
            vec![("$a$.txt", "A"), ("$b.txt", "B"), ("c$.txt", "C")],
            vec!["$a$.txt.done", "$b.txt.done", "c$.txt.done"],
        ),
    ] {
        let dir = TestDirectory::new("FileConsumeDoneFileIssueTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::new();
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(
                    from(&uri)
                        .route_id("foo")
                        .auto_startup(false)
                        .convert_body_to("String")
                        .to("mock:result"),
                );
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        for (name, body) in &files {
            send(&t, &dir, name, body).await;
        }
        for d in &done {
            send(&t, &dir, d, "done").await;
            assert!(dir.test_file(d).exists(), "Done file should exists");
        }
        let mock = t.mock("mock:result");
        mock.expected_bodies_received_in_any_order(files.iter().map(|(_, b)| *b));
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
        for d in &done {
            let f = dir.test_file(d);
            support_wait(|| !f.exists()).await;
            assert!(!f.exists(), "Done file should be deleted: {d}");
        }
    }
}

/// Ports of `FileConsumeDynamicDoneFileNameWithTwoDotsTest` and
/// `FileConsumeSimpleDynamicDoneFileNameWithTwoDotsTest`.
#[tokio::test]
async fn test_dynamic_done_file_name_with_two_dots() {
    for token in ["${file:name.noext}", "$simple{file:name.noext}"] {
        let dir = TestDirectory::new("FileConsumeDynamicDoneFileNameWithTwoDotsTest");
        let uri = dir.file_uri(&format!("?doneFileName={token}.done&initialDelay=0"));
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received_in_any_order(["input-body"]);
        send(&t, &dir, "test.twodot.txt", "input-body").await;
        send(&t, &dir, "test.twodot.done", "done-body").await;
        mock.assert_is_satisfied().await;
        for f in ["test.twodot.txt", "test.twodot.done"] {
            let f = dir.test_file(f);
            support_wait(|| !f.exists()).await;
            assert!(!f.exists(), "{}", f.display());
        }
    }
}

/// Port of `FilerConsumerDualDoneFileNameTest` (the producer writes the done files).
#[tokio::test]
async fn test_dual_done_file_name() {
    for missing in [false, true] {
        let dir = TestDirectory::new("FilerConsumerDualDoneFileNameTest");
        let uri = dir.file_uri("?doneFileName=${file:name}.ready&initialDelay=0&delay=10");
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        let producer = dir.file_uri("?doneFileName=${file:name}.ready");
        t.template
            .send_body_and_header(&producer, "Hello World", FILE_NAME, "hello.txt")
            .await
            .unwrap();
        let bye = if missing { dir.file_uri("") } else { producer };
        t.template
            .send_body_and_header(&bye, "Bye World", FILE_NAME, "bye.txt")
            .await
            .unwrap();
        if missing {
            mock.expected_bodies_received(["Hello World"]);
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        } else {
            mock.expected_bodies_received_in_any_order(["Hello World", "Bye World"]);
        }
        mock.assert_is_satisfied().await;
    }
}

/// Port of `FilerConsumerMoveFailedDoneFileNameTest`.
#[tokio::test]
async fn test_move_failed_done_file_name() {
    let dir = TestDirectory::new("FilerConsumerMoveFailedDoneFileNameTest");
    let uri = dir.file_uri("?doneFileName=done&initialDelay=0&delay=10&moveFailed=failed");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&uri)
                .to("mock:input")
                .throw_exception(CamelError::illegal_argument("Forced")),
        );
    })
    .await;
    let mock = t.mock("mock:input");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("failed/hello.txt"), None);
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "done", "").await;
    mock.assert_is_satisfied().await;
    let done = dir.test_file("done");
    support_wait(|| !done.exists()).await;
    assert!(!done.exists());
}

/// Port of `FilerConsumerPreMoveDoneFileNameTest`.
#[tokio::test]
async fn test_pre_move_done_file_name() {
    done_file(
        "FilerConsumerPreMoveDoneFileNameTest",
        "?preMove=work/work-${file:name}&doneFileName=ready&initialDelay=0&delay=10",
        "ready",
        &[("ready", false)],
    )
    .await;
}

/// Port of `FilerConsumerRetryDoneFileNameTest`: a failure keeps the done file for the retry.
#[tokio::test]
async fn test_retry_done_file_name() {
    let dir = TestDirectory::new("FilerConsumerRetryDoneFileNameTest");
    let uri = dir.file_uri("?doneFileName=done&initialDelay=0&delay=10");
    let done = dir.test_file("done");
    let index = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let (done, index) = (done.clone(), index.clone());
        r.add(from(&uri).to("mock:input").process_fn(move |_| {
            if index.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                assert!(done.exists());
                return Err(CamelError::illegal_argument("Forced"));
            }
            Ok(())
        }));
    })
    .await;
    let mock = t.mock("mock:input");
    mock.expected_message_count(2);
    mock.expected_file_exists(dir.test_file(".camel/hello.txt"), None);
    send(&t, &dir, "hello.txt", "Hello World").await;
    send(&t, &dir, "done", "").await;
    mock.assert_is_satisfied().await;
    let done = dir.test_file("done");
    support_wait(|| !done.exists()).await;
    assert!(!done.exists());
}

/// Ports of `FilerConsumerShouldSkipDoneFileTest`, `FilerConsumerShouldSkipDoneFilePrefixTest`
/// and `FilerConsumerShouldSkipDoneFileSuffixTest`: the done file alone is not consumed.
#[tokio::test]
async fn test_should_skip_done_file() {
    for (option, done) in [
        ("?doneFileName=done&initialDelay=0&delay=10", "done"),
        (
            "?doneFileName=done-${file:name}&initialDelay=0&delay=10",
            "done-hello.txt",
        ),
        (
            "?doneFileName=${file:name}.ready&initialDelay=0&delay=50",
            "hello.txt.ready",
        ),
    ] {
        let dir = TestDirectory::new("FilerConsumerShouldSkipDoneFileTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_message_count(0);
        send(&t, &dir, done, "").await;
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        mock.assert_is_satisfied().await;
        mock.reset();
        assert!(dir.test_file(done).exists());
        mock.expected_bodies_received(["Hello World"]);
        send(&t, &dir, "hello.txt", "Hello World").await;
        mock.assert_is_satisfied().await;
        let f = dir.test_file(done);
        support_wait(|| !f.exists()).await;
        assert!(!f.exists(), "{option}");
    }
}

/// The bodies in the order of the route consuming the files with the options.
async fn sorted(name: &str, option: &str, files: &[(&str, &str)], expected: &[&str]) {
    let dir = TestDirectory::new(name);
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    for (file, body) in files {
        send(&t, &dir, file, body).await;
    }
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(expected.iter().copied());
    let uri = dir.file_uri(option);
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileSortByExpressionTest`, `FileSortByIgnoreCaseExpressionTest`,
/// `FileSortByNestedExpressionTest` and `FileConsumeMultipleDirectoriesTest` (the order).
#[tokio::test]
async fn test_sort_by() {
    let cities = [
        ("paris.dat", "Hello Paris"),
        ("london.txt", "Hello London"),
        ("copenhagen.xml", "Hello Copenhagen"),
    ];
    sorted(
        "FileSortByExpressionTest",
        "?initialDelay=0&delay=10&sortBy=file:ext",
        &cities,
        &["Hello Paris", "Hello London", "Hello Copenhagen"],
    )
    .await;
    sorted(
        "FileSortByExpressionTest",
        "?initialDelay=0&delay=10&sortBy=reverse:file:ext",
        &cities,
        &["Hello Copenhagen", "Hello London", "Hello Paris"],
    )
    .await;

    let reports = [
        ("report-3.dat", "Hello Paris"),
        ("REPORT-2.txt", "Hello London"),
        ("Report-1.xml", "Hello Copenhagen"),
    ];
    sorted(
        "FileSortByIgnoreCaseExpressionTest",
        "?sortBy=file:name&initialDelay=0&delay=10",
        &reports,
        &["Hello London", "Hello Copenhagen", "Hello Paris"],
    )
    .await;
    sorted(
        "FileSortByIgnoreCaseExpressionTest",
        "?initialDelay=0&delay=10&sortBy=ignoreCase:file:name",
        &reports,
        &["Hello Copenhagen", "Hello London", "Hello Paris"],
    )
    .await;
    sorted(
        "FileSortByIgnoreCaseExpressionTest",
        "?initialDelay=0&delay=10&sortBy=reverse:ignoreCase:file:name",
        &reports,
        &["Hello Paris", "Hello London", "Hello Copenhagen"],
    )
    .await;

    let nested = [
        ("paris.txt", "Hello Paris"),
        ("london.txt", "Hello London"),
        ("copenhagen.xml", "Hello Copenhagen"),
        ("dublin.txt", "Hello Dublin"),
    ];
    sorted(
        "FileSortByNestedExpressionTest",
        "?initialDelay=0&delay=10&sortBy=file:ext;file:name",
        &nested,
        &[
            "Hello Dublin",
            "Hello London",
            "Hello Paris",
            "Hello Copenhagen",
        ],
    )
    .await;
    sorted(
        "FileSortByNestedExpressionTest",
        "?initialDelay=0&delay=10&sortBy=file:ext;reverse:file:name",
        &nested,
        &[
            "Hello Paris",
            "Hello London",
            "Hello Dublin",
            "Hello Copenhagen",
        ],
    )
    .await;

    sorted(
        "FileConsumeMultipleDirectoriesTest",
        "?initialDelay=0&delay=10&recursive=true&delete=true&sortBy=file:path",
        &[
            ("bye.txt", "Bye World"),
            ("sub/hello.txt", "Hello World"),
            ("sub/sub2/godday.txt", "Godday World"),
        ],
        &["Bye World", "Hello World", "Godday World"],
    )
    .await;
}

/// Port of `FileConsumeNotEagerMaxMessagesPerPollTest`.
#[tokio::test]
async fn test_not_eager_max_messages_per_poll() {
    use camel_core::api::exchange_constants::BATCH_SIZE;
    let dir = TestDirectory::new("FileConsumeNotEagerMaxMessagesPerPollTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&maxMessagesPerPoll=2&eagerMaxMessagesPerPoll=false&sortBy=file:name");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["AAA", "BBB"]);
    send(&t, &dir, "ccc.txt", "CCC").await;
    send(&t, &dir, "aaa.txt", "AAA").await;
    send(&t, &dir, "bbb.txt", "BBB").await;
    t.context.start_route("foo").await.unwrap();
    mock.expected_property_received(BATCH_SIZE, 2);
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_bodies_received(["CCC"]);
    mock.expected_property_received(BATCH_SIZE, 1);
    mock.assert_is_satisfied().await;
}

/// Port of `FileConsumerRelativeFileNameTest`.
#[tokio::test]
async fn test_relative_file_name() {
    let dir = TestDirectory::new("FileConsumerRelativeFileNameTest");
    let uri =
        dir.file_uri("filename-consumer?initialDelay=0&delay=10&recursive=true&sortBy=file:name");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_message_count(2);
    mock.all_messages_match(header(FILE_NAME).is_not_null());
    send(
        &t,
        &dir,
        "filename-consumer/filename-consumer-hello.txt",
        "Hello World",
    )
    .await;
    send(
        &t,
        &dir,
        "filename-consumer/filename-consumer-bye.txt",
        "Bye World",
    )
    .await;
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
    let names: Vec<String> = mock
        .received_exchanges()
        .iter()
        .map(|e| {
            e.message()
                .header(FILE_NAME)
                .map(|v| v.to_string())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        vec!["filename-consumer-bye.txt", "filename-consumer-hello.txt"],
        names
    );
}

/// Port of `FileConsumerFilterDirectoryTest`.
#[tokio::test]
async fn test_filter_directory() {
    for with_regular in [false, true] {
        let dir = TestDirectory::new("FileConsumerFilterDirectoryTest");
        let uri = dir.file_uri("?initialDelay=0&delay=10&recursive=true&filterDirectory=${header.CamelFileNameOnly.length()} > 4");
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        send(&t, &dir, "foo/skipme.txt", "This is a file to be filtered").await;
        if with_regular {
            mock.expected_bodies_received(["Hello World"]);
            send(&t, &dir, "barbar/hello.txt", "Hello World").await;
        } else {
            mock.expected_message_count(0);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        mock.assert_is_satisfied().await;
    }
}

/// Port of `FileConsumerFilterFileTest`.
#[tokio::test]
async fn test_filter_file() {
    let routes = |dir: &TestDirectory| {
        let (a, b) = (
            dir.file_uri("?initialDelay=0&delay=10&filterFile=${bodyAs(String)} contains 'World'"),
            dir.file_uri("?initialDelay=0&delay=10&filterFile=${file:modified} < ${date:now-2s}"),
        );
        move |r: &mut Routes| {
            r.add(from(&a).convert_body_to("String").to("mock:result"));
            r.add(from(&b).convert_body_to("String").to("mock:result2"));
        }
    };
    let dir = TestDirectory::new("FileConsumerFilterFileTest");
    let t = ContextTestSupport::with_routes(routes(&dir)).await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    send(&t, &dir, "skipme.txt", "This is a file to be filtered").await;
    send(&t, &dir, "hello.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    // the older than 2 seconds file
    let dir = TestDirectory::new("FileConsumerFilterFileTest");
    let t = ContextTestSupport::with_routes(routes(&dir)).await;
    let mock = t.mock("mock:result2");
    mock.expected_bodies_received(["Something else"]);
    mock.set_result_wait_time(5000);
    send(&t, &dir, "hello2.txt", "Something else").await;
    mock.assert_is_satisfied().await;
}

/// Port of `FileMarkerFileRecursiveDeleteOldLockFilesTest`.
#[tokio::test]
async fn test_marker_file_recursive_old_lock_files() {
    use camel_core::api::exchange_constants::FILE_NAME_ONLY;
    let dir = TestDirectory::new("FileMarkerFileRecursiveDeleteOldLockFilesTest");
    let uri = dir.file_uri("?initialDelay=0&delay=10&recursive=true&sortBy=file:name");
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from(&uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .convert_body_to("String")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Bye World", "Hi World"]);
    mock.message_matches(0, header(FILE_NAME_ONLY).is_equal_to("bye.txt"));
    mock.message_matches(1, header(FILE_NAME_ONLY).is_equal_to("hi.txt"));
    send(&t, &dir, "hello.txt.camelLock", "locked").await;
    send(&t, &dir, "bye.txt", "Bye World").await;
    send(&t, &dir, "foo/gooday.txt.camelLock", "locked").await;
    send(&t, &dir, "foo/hi.txt", "Hi World").await;
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileConsumerIdempotentKeyChangedIssueTest` and
/// `FileConsumerIdempotentKeyChangedIssue2Test`: a changed file has another idempotent key.
#[tokio::test]
async fn test_idempotent_key_changed() {
    for option in [
        "?noop=true&readLock=changed&initialDelay=0&delay=10&readLockCheckInterval=100&idempotentKey=${file:onlyname}-${file:size}-${date:file:yyyyMMddHHmmss}",
        "?noop=true&initialDelay=0&delay=100&idempotentKey=${file:name}-${file:size}-${file:modified}",
    ] {
        let dir = TestDirectory::new("FileConsumerIdempotentKeyChangedIssueTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::new();
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from(&uri).route_id("foo").auto_startup(false).convert_body_to("String").to("log:file").to("mock:file"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:file");
        mock.expected_bodies_received(["Hello World"]);
        send(&t, &dir, "hello.txt", "Hello World").await;
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
        mock.reset();
        mock.expected_bodies_received(["Hello World Again"]);
        send(&t, &dir, "hello.txt", "Hello World Again").await;
        mock.assert_is_satisfied().await;
    }
}

/// Port of `NewFileConsumeTest`: a consumer created from the endpoint with a processor.
#[tokio::test]
async fn test_new_file_consumer() {
    let dir = TestDirectory::new("NewFileConsumeTest");
    dir.write("hello.txt", "Hello World");
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    let endpoint = t
        .context
        .endpoint(&dir.file_uri("?initialDelay=0&delay=10"))
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let processor: camel_core::api::processor::SharedProcessor = std::sync::Arc::new(
        camel_core::api::processor::processor_fn(move |e: &mut Exchange| {
            let _ = tx.send(e.message().body().to_string());
            Ok(())
        }),
    );
    let consumer = endpoint.create_consumer(processor).unwrap();
    assert!(dir.test_file("hello.txt").exists());
    consumer.start().await.unwrap();
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .unwrap();
    assert_eq!(Some("Hello World".to_string()), body);
    consumer.stop().await.unwrap();
}

/// Ports of `FileConsumeRunLoggingLevelTest`, `FileConsumeBackoffMultiplierTest` and
/// `FileToFileNioLowBufferTest` (options without effect in the port).
#[tokio::test]
async fn test_accepted_options() {
    consume(
        "FileConsumeRunLoggingLevelTest",
        "?runLoggingLevel=INFO&initialDelay=0&delay=10",
        &[("hello.txt", "Hello World")],
        &["Hello World"],
        &[],
    )
    .await;
    consume(
        "FileConsumeBackoffMultiplierTest",
        "?initialDelay=0&delay=10&delete=true&backoffMultiplier=4&backoffIdleThreshold=3",
        &[("report.txt", "Hello World")],
        &["Hello World"],
        &[],
    )
    .await;

    let dir = TestDirectory::new("FileToFileNioLowBufferTest");
    let (from_uri, out) = (
        dir.file_uri("in?initialDelay=0&delay=10"),
        dir.file_uri("out?bufferSize=4"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from(&from_uri)
                .convert_body_to("String")
                .to(&out)
                .to("mock:result"),
        );
    })
    .await;
    let body = "1234567890123456789012345678901234567890";
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("out/hello.txt"), Some(body));
    send(&t, &dir, "in/hello.txt", body).await;
    mock.assert_is_satisfied().await;
}

/// Port of `FilePollingConsumerTest`: a stopped polling consumer leaves the files.
#[tokio::test]
async fn test_polling_consumer() {
    let dir = TestDirectory::new("FilePollingConsumerTest");
    let t = ContextTestSupport::new();
    t.context.start().await.unwrap();
    send(&t, &dir, "hello.txt", "Hello World").await;
    let consumer = t
        .context
        .endpoint(&dir.file_uri(""))
        .unwrap()
        .create_polling_consumer()
        .unwrap();
    consumer.start().await.unwrap();
    let exchange = consumer
        .receive_timeout(std::time::Duration::from_millis(5000))
        .await
        .unwrap()
        .expect("an exchange");
    assert_eq!("Hello World", exchange.message().body().to_string());
    send(&t, &dir, "bye.txt", "Bye World").await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        dir.test_file("bye.txt").exists(),
        "File should exist bye.txt"
    );
    consumer.stop().await.unwrap();
}

/// Port of `FileRecursiveDepthTest`.
#[tokio::test]
async fn test_recursive_depth() {
    let files = [
        ("a.txt", "a"),
        ("b.txt", "b"),
        ("foo/a2.txt", "a2"),
        ("foo/bar/a3.txt", "a3"),
        ("bar/b2.txt", "b2"),
        ("bar/foo/b3.txt", "b3"),
    ];
    for (option, expected) in [
        (
            "?initialDelay=0&delay=10&recursive=true&minDepth=2&maxDepth=2",
            vec!["a2", "b2"],
        ),
        (
            "?initialDelay=0&delay=10&recursive=true&minDepth=2&maxDepth=99",
            vec!["a2", "b2", "a3", "b3"],
        ),
        (
            "?initialDelay=0&delay=10&recursive=true&minDepth=1&maxDepth=1",
            vec!["a", "b"],
        ),
    ] {
        consume("FileRecursiveDepthTest", option, &files, &expected, &[]).await;
    }
}

/// Port of `PollEnrichHeaderPropagationTest`: the headers, properties and variables of the
/// exchange are kept by a pollEnrich without strategy.
#[tokio::test]
async fn test_poll_enrich_header_propagation() {
    let dir = TestDirectory::new("PollEnrichHeaderPropagationTest");
    let uri = dir.file_uri("?fileName=file.txt");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header("foo", constant("bar"))
                .set_header("customHeader", constant("customValue"))
                .set_property("customProp", constant("cheese"))
                .set_variable("myVar", constant("beer"))
                .poll_enrich(&uri, 3000)
                .convert_body_to("String")
                .to("log:result?showAll=true")
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello Camel from file"]);
    mock.expected_header_received("foo", "bar");
    mock.expected_header_received("customHeader", "customValue");
    mock.expected_property_received("customProp", "cheese");
    mock.expected_variable_received("myVar", "beer");
    send(&t, &dir, "file.txt", "Hello Camel from file").await;
    t.template
        .send_body("direct:start", "trigger")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileMoveWithInMessageTest` and `FileMoveWithOutMessageTest`: two pollEnrich move
/// their files, the route replacing the in (or setting the out) message.
#[tokio::test]
async fn test_move_with_in_and_out_message() {
    for out in [false, true] {
        move_with_message(out).await;
    }
}

async fn move_with_message(out: bool) {
    let dir = TestDirectory::new("FileMoveWithInMessageTest");
    let uri = dir.file_uri("?move=archive");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("seda:triggerIn")
                .poll_enrich(&uri, -1)
                .poll_enrich(&uri, -1)
                .process_fn(move |e| {
                    // a new in message with the body and headers
                    let mut msg = camel_core::api::message::Message::default();
                    msg.set_body(e.message().body().clone());
                    for (k, v) in e.message().headers().iter() {
                        msg.set_header(k, v.clone());
                    }
                    if out {
                        e.set_out(Some(msg));
                    } else {
                        *e.in_message_mut() = msg;
                    }
                    Ok(())
                }),
        );
    })
    .await;
    send(&t, &dir, "hello1.txt", "Hello World1").await;
    send(&t, &dir, "hello2.txt", "Hello World2").await;
    t.template.send_body("seda:triggerIn", "").await.unwrap();
    for f in ["archive/hello1.txt", "archive/hello2.txt"] {
        let f = dir.test_file(f);
        await_until(std::time::Duration::from_secs(10), || f.exists()).await;
    }
}

/// Port of `FileConsumeCurrentDirTest` (a file of the working directory).
#[tokio::test]
async fn test_consume_current_dir() {
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    for (dir, body) in [(".", "Report 123"), ("./", "Report 456")] {
        let name = format!(
            "current-dir-{}-{}.txt",
            std::process::id(),
            body.len() + dir.len()
        );
        let _file = Remove(std::path::PathBuf::from(&name));
        std::fs::write(&name, body).unwrap();
        let uri = format!("file:{dir}?initialDelay=0&delay=10&noop=true&include={name}");
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received([body]);
        mock.assert_is_satisfied().await;
        t.context.stop().await.unwrap();
    }
}

/// Port of `FileConsumerCommitRenameStrategyTest`: the move replaces an existing file.
#[tokio::test]
async fn test_commit_rename_strategy() {
    for existing in [false, true] {
        let dir = TestDirectory::new("FileConsumerCommitRenameStrategyTest");
        if existing {
            dir.write("done/paris.txt", "I was there once in Paris");
        }
        let uri = dir.file_uri("reports?move=../done/${file:name}&initialDelay=0&delay=10");
        let t = ContextTestSupport::new();
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(
                    from(&uri)
                        .route_id("foo")
                        .auto_startup(false)
                        .convert_body_to("String")
                        .to("mock:report"),
                );
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:report");
        mock.expected_bodies_received(["Hello Paris"]);
        mock.expected_file_exists(dir.test_file("done/paris.txt"), Some("Hello Paris"));
        send(&t, &dir, "reports/paris.txt", "Hello Paris").await;
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
    }
}

/// Port of `FileConsumePollEnrichFileUsingProcessorTest`: a processor receives the data file
/// of the consumed file with a consumer template.
#[tokio::test]
async fn test_poll_enrich_file_using_processor() {
    let dir = TestDirectory::new("FileConsumePollEnrichFileUsingProcessorTest");
    let t = ContextTestSupport::new();
    let consumer = std::sync::Arc::new(t.context.create_consumer_template());
    let producer = std::sync::Arc::new(t.context.create_producer_template());
    let (enrich, data_dir) = (
        dir.file_uri("enrich?initialDelay=0&delay=10&move=.done"),
        dir.file_uri("enrichdata"),
    );
    t.context
        .add_routes(move |r: &mut Routes| {
            let (consumer, producer, data_dir) =
                (consumer.clone(), producer.clone(), data_dir.clone());
            let lookup: camel_core::api::processor::SharedProcessor = std::sync::Arc::new(
                camel_core::api::processor::async_processor_fn(move |e: &mut Exchange| {
                    let (consumer, producer, data_dir) =
                        (consumer.clone(), producer.clone(), data_dir.clone());
                    Box::pin(async move {
                        let name = e
                            .message()
                            .header(camel_core::api::exchange_constants::FILE_NAME_ONLY)
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        let name = format!("{}.dat", camel_core::util::file_util::strip_ext(&name));
                        let uri = format!(
                            "{data_dir}?initialDelay=0&delay=10&move=.done&fileName={name}"
                        );
                        match consumer.receive_timeout(&uri, 5000).await? {
                            Some(data) => {
                                producer.send("direct:data", data).await?;
                                Ok(())
                            }
                            None => Err(CamelError::illegal_state(format!(
                                "Cannot find the data file {name}"
                            ))),
                        }
                    })
                }),
            );
            r.add(from(&enrich).process(lookup).to("mock:start"));
            r.add(from("direct:data").to("mock:result"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:start").expected_bodies_received(["Start"]);
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Big file"]);
    mock.expected_file_exists(dir.test_file("enrich/.done/AAA.fin"), None);
    mock.expected_file_exists(dir.test_file("enrichdata/.done/AAA.dat"), None);
    mock.expected_file_exists(dir.test_file("enrichdata/BBB.dat"), None);
    send(&t, &dir, "enrichdata/AAA.dat", "Big file").await;
    send(&t, &dir, "enrichdata/BBB.dat", "Other Big file").await;
    send(&t, &dir, "enrich/AAA.fin", "Start").await;
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileExclusiveReadNoneStrategyTest`: without read lock a file being written is
/// read at once.
#[tokio::test]
async fn test_exclusive_read_none_strategy() {
    let dir = TestDirectory::new("FileExclusiveReadNoneStrategyTest");
    std::fs::create_dir_all(dir.test_file("slowfile")).unwrap();
    let (uri, slow) = (
        dir.file_uri(
            "slowfile?noop=true&initialDelay=0&delay=10&readLock=none&readLockTimeout=500",
        ),
        dir.test_file("slowfile/hello.txt"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let slow = slow.clone();
        let writer: camel_core::api::processor::SharedProcessor = std::sync::Arc::new(
            camel_core::api::processor::async_processor_fn(move |_: &mut Exchange| {
                let slow = slow.clone();
                Box::pin(async move {
                    use std::io::Write;
                    let mut f = std::fs::File::create(&slow).unwrap();
                    f.write_all(b"Hello World").unwrap();
                    for i in 0..3 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        f.write_all(format!("Line #{i}").as_bytes()).unwrap();
                    }
                    f.write_all(b"Bye World").unwrap();
                    Ok(())
                })
            }),
        );
        r.add(from("seda:start").process(writer));
        r.add(from(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    t.template
        .send_body("seda:start", "Create the slow file")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    let body = mock.received_exchanges()[0].message().body().to_string();
    assert!(
        !body.ends_with("Bye World"),
        "Should not wait and read the entire file: {body}"
    );
}

/// Port of `FileSplitInSplitTest`: parallel nested splits appending to the same files.
#[tokio::test]
async fn test_split_in_split() {
    let dir = TestDirectory::new("FileSplitInSplitTest");
    let (from_uri, out) = (
        dir.file_uri("?initialDelay=0&delay=10"),
        dir.file_uri("outbox?fileExist=Append&fileName=result${exchangeProperty.split}.txt"),
    );
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            let (out, out2) = (out.clone(), out.clone());
            r.add(
                from(&from_uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .split(body().tokenize(","), move |s| {
                        s.set_property("split", simple("${exchangeProperty.CamelSplitIndex}"))
                            .split(body().tokenize("\n"), move |s| {
                                s.set_body(body().append(":Status=OK").append("\n"))
                                    .to(&out)
                            })
                            .parallel_processing()
                            .streaming()
                            .set_body(simple("${exchangeProperty.split} complete"))
                            .to(&out2)
                    })
                    .parallel_processing()
                    .streaming()
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mut body = String::new();
    for i in 0..3 {
        body.push_str(&format!("Block1 Line {i}\n"));
    }
    body.push(',');
    for i in 10..13 {
        body.push_str(&format!("Block2 Line {i}\n"));
    }
    send(&t, &dir, "input.txt", &body).await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
    for f in ["outbox/result0.txt", "outbox/result1.txt"] {
        let txt = dir.read(f).unwrap();
        assert_eq!(4, txt.lines().count(), "{f}: {txt}");
    }
}

/// Port of `FileConsumerFileExpressionTest`: a fileName option with a bean and a date.
#[tokio::test]
async fn test_file_expression() {
    let dir = TestDirectory::new("FileConsumerFileExpressionTest");
    let t = ContextTestSupport::new();
    let counter = camel_bean::MethodBean::new("MyGuidGenerator")
        .method("next", |_| Ok(Value::from("123")))
        .into_value();
    t.context
        .registry()
        .unwrap()
        .bind("counter", counter)
        .unwrap();
    t.context.start().await.unwrap();
    for (name, body) in [
        ("bean/122.txt", "Hello World"),
        ("bean/123.txt", "Goodday World"),
        ("bean/124.txt", "Bye World"),
    ] {
        send(&t, &dir, name, body).await;
    }
    // the name of today (Java sends the expression as the file name header)
    let mut e = Exchange::new(t.context.clone());
    let today = t
        .context
        .resolve_language("simple")
        .unwrap()
        .create_expression("date/myfile-${date:now:yyyyMMdd}.txt")
        .unwrap()
        .evaluate_mut(&mut e)
        .unwrap()
        .to_string();
    for (name, body) in [
        ("date/myfile-20081128.txt".to_string(), "Bye World"),
        ("date/myfile-20081129.txt".to_string(), "Hello World"),
        (today, "Goodday World"),
    ] {
        send(&t, &dir, &name, body).await;
    }
    let (bean, date) = (
        dir.file_uri("bean?initialDelay=0&delay=10&fileName=${bean:counter.next}.txt&delete=true"),
        dir.file_uri("date?initialDelay=0&delay=10&fileName=myfile-${date:now:yyyyMMdd}.txt"),
    );
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&bean).convert_body_to("String").to("mock:result"));
            r.add(from(&date).convert_body_to("String").to("mock:date"));
        })
        .await
        .unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["Goodday World"]);
    t.mock("mock:date")
        .expected_bodies_received(["Goodday World"]);
    t.assert_mock_endpoints_satisfied().await;
}

/// Port of `FileConsumerIdempotentRefTest`: the idempotent repository of the registry.
#[tokio::test]
async fn test_idempotent_ref() {
    use camel_core::api::spi::idempotent_repository::{
        IdempotentRepository, SharedIdempotentRepository,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    struct MyRepository {
        invoked: AtomicBool,
        expected: String,
    }
    impl IdempotentRepository for MyRepository {
        fn add(&self, key: &str) -> bool {
            assert_eq!(self.expected, key);
            !self.invoked.swap(true, Ordering::SeqCst)
        }
        fn contains(&self, _: &str) -> bool {
            self.invoked.load(Ordering::SeqCst)
        }
        fn remove(&self, _: &str) -> bool {
            true
        }
        fn clear(&self) {}
    }
    let dir = TestDirectory::new("FileConsumerIdempotentRefTest");
    let repository = std::sync::Arc::new(MyRepository {
        invoked: AtomicBool::new(false),
        expected: dir.test_file("report.txt").to_string_lossy().into_owned(),
    });
    let shared: SharedIdempotentRepository = repository.clone();
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind(
            "myRepo",
            camel_core::support::idempotent::repository_value(shared),
        )
        .unwrap();
    let uri = dir.file_uri("?idempotent=true&idempotentRepository=#myRepo&move=done/${file:name}&initialDelay=0&delay=10");
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    send(&t, &dir, "report.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    let done = dir.test_file("done/report.txt");
    support_wait(|| done.exists()).await;
    mock.reset();
    mock.expected_message_count(0);
    std::fs::rename(&done, dir.test_file("report.txt")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    mock.assert_is_satisfied().await;
    assert!(
        repository.invoked.load(Ordering::SeqCst),
        "MyIdempotentRepository should have been invoked"
    );
}

/// Port of `FileConsumerAutoCreateDirectoryTest`.
#[tokio::test]
async fn test_auto_create_directory() {
    let t = ContextTestSupport::new();
    let noop: camel_core::api::processor::SharedProcessor = std::sync::Arc::new(
        camel_core::api::processor::processor_fn(|_: &mut Exchange| Ok(())),
    );
    for (path, absolute, exists) in [
        ("foo", false, true),
        ("foo", true, true),
        ("foo?autoCreate=false", false, false),
        ("foo.bar?autoCreate=true", false, true),
    ] {
        let dir = TestDirectory::new("FileConsumerAutoCreateDirectoryTest");
        let uri = if absolute {
            format!("file://{}/{path}", dir.path())
        } else {
            dir.file_uri(path)
        };
        let consumer = t
            .context
            .endpoint(&uri)
            .unwrap()
            .create_consumer(noop.clone())
            .unwrap();
        consumer.start().await.unwrap();
        consumer.stop().await.unwrap();
        let created = dir.test_file(path.split('?').next().unwrap());
        assert_eq!(exists, created.is_dir(), "{uri}");
    }
    let dir = TestDirectory::new("FileConsumerAutoCreateDirectoryTest");
    let consumer = t
        .context
        .endpoint(&dir.file_uri("foo?autoCreate=false&startingDirectoryMustExist=true"))
        .unwrap()
        .create_consumer(noop)
        .unwrap();
    let e = consumer.start().await.unwrap_err();
    assert!(
        e.to_string().contains("Starting directory does not exist"),
        "{e}"
    );
    assert!(
        !dir.test_file("foo").exists(),
        "Directory should NOT be created"
    );
}

/// Port of `FileConsumerFailureHandledTest`: a handled failure deletes the file, a failure not
/// handled keeps it.
#[tokio::test]
async fn test_failure_handled() {
    for (city, mock_uri, expected, deleted) in [
        ("Paris", "mock:valid", "Hello Paris", true),
        ("London", "mock:invalid", "London", true),
        ("Dublin", "mock:beer", "Dublin", false),
        ("Madrid", "mock:error", "Madrid", true),
    ] {
        let dir = TestDirectory::new("FileConsumerFailureHandledTest");
        let uri = dir.file_uri("?initialDelay=0&delay=10&delete=true");
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.error_handler(
                dead_letter_channel("mock:error")
                    .maximum_redeliveries(2)
                    .redelivery_delay(0)
                    .log_stack_trace(false),
            );
            r.on_exception(
                on_exception(ErrorType::Kind(ErrorKind::Validation))
                    .on_when(simple("${exception.message} contains 'beer'"))
                    .handled(false)
                    .to("mock:beer"),
            );
            r.on_exception(
                on_exception(ErrorType::Kind(ErrorKind::Validation))
                    .handled(true)
                    .to("mock:invalid"),
            );
            r.add(
                from(&uri)
                    .process_fn(|e| {
                        let body = e.message().body().to_string();
                        match body.as_str() {
                            "London" => {
                                Err(CamelError::validation(e, "Forced exception by unit test"))
                            }
                            "Madrid" => Err(CamelError::runtime("Madrid is not a supported city")),
                            "Dublin" => Err(CamelError::validation(e, "Dublin have good beer")),
                            _ => {
                                e.message_mut().set_body(format!("Hello {body}"));
                                Ok(())
                            }
                        }
                    })
                    .to("mock:valid"),
            );
        })
        .await;
        let mock = t.mock(mock_uri);
        mock.expected_bodies_received([expected]);
        send(&t, &dir, &format!("{}.txt", city.to_lowercase()), city).await;
        mock.assert_is_satisfied().await;
        let file = dir.test_file(&format!("{}.txt", city.to_lowercase()));
        support_wait(|| file.exists() != deleted).await;
        assert_eq!(deleted, !file.exists(), "{city}");
        t.context.stop().await.unwrap();
        assert!(!dir
            .test_file(&format!("{}.txt.camelLock", city.to_lowercase()))
            .exists());
    }
}

/// Port of `FileConsumeAlterFileNameHeaderIssueTest`: the route changing or removing the file
/// headers does not change the file deleted or moved.
#[tokio::test]
async fn test_alter_file_name_header() {
    for (option, remove, done) in [
        ("?initialDelay=0&delay=10&delete=true", true, None),
        ("?initialDelay=0&delay=10&delete=true", false, None),
        ("?initialDelay=0&delay=10", true, Some(".camel/hello.txt")),
        ("?initialDelay=0&delay=10", false, Some(".camel/hello.txt")),
    ] {
        let dir = TestDirectory::new("FileConsumeAlterFileNameHeaderIssueTest");
        let uri = dir.file_uri(option);
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let route = from(&uri);
            let route = if remove {
                route.remove_headers("*")
            } else {
                route.set_header(FILE_NAME, constant("bye.txt"))
            };
            r.add(route.to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Hello World"]);
        if !remove {
            mock.expected_header_received(FILE_NAME, "bye.txt");
        }
        send(&t, &dir, "hello.txt", "Hello World").await;
        mock.assert_is_satisfied().await;
        if remove {
            assert!(
                mock.received_exchanges()[0].message().headers().is_empty(),
                "Headers should have been removed"
            );
        }
        let gone = dir.test_file("hello.txt");
        support_wait(|| !gone.exists()).await;
        assert!(!gone.exists(), "{option}");
        if let Some(done) = done {
            let f = dir.test_file(done);
            support_wait(|| f.exists()).await;
            assert!(f.exists(), "File should been moved");
        }
    }
}

/// Port of `FileConsumerIdempotentConfirmTest`: the key is confirmed when the file is done.
#[tokio::test]
async fn test_idempotent_confirm() {
    use camel_core::api::spi::idempotent_repository::{
        IdempotentRepository, SharedIdempotentRepository,
    };
    use camel_core::support::idempotent::MemoryIdempotentRepository;
    #[derive(Default)]
    struct MyRepo {
        inner: MemoryIdempotentRepository,
        key: std::sync::Mutex<Option<String>>,
    }
    impl IdempotentRepository for MyRepo {
        fn add(&self, key: &str) -> bool {
            self.inner.add(key)
        }
        fn contains(&self, key: &str) -> bool {
            self.inner.contains(key)
        }
        fn remove(&self, key: &str) -> bool {
            self.inner.remove(key)
        }
        fn confirm(&self, key: &str) -> bool {
            *self.key.lock().unwrap() = Some(key.to_string());
            self.inner.confirm(key)
        }
        fn clear(&self) {
            self.inner.clear()
        }
    }
    let dir = TestDirectory::new("FileConsumerIdempotentConfirmTest");
    let repo = std::sync::Arc::new(MyRepo::default());
    let shared: SharedIdempotentRepository = repo.clone();
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind(
            "myRepo",
            camel_core::support::idempotent::repository_value(shared),
        )
        .unwrap();
    let uri = dir.file_uri("?idempotent=true&move=done/${file:name}&initialDelay=0&delay=10&idempotentRepository=#myRepo");
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    assert!(repo.key.lock().unwrap().is_none());
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    send(&t, &dir, "report.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    let done = dir.test_file("done/report.txt");
    support_wait(|| done.exists()).await;
    mock.reset();
    mock.expected_message_count(0);
    std::fs::rename(&done, dir.test_file("report.txt")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    mock.assert_is_satisfied().await;
    support_wait(|| repo.key.lock().unwrap().is_some()).await;
}

/// Port of `FileConsumerMoveExpressionTest` (not the case configuring the endpoint object).
#[tokio::test]
async fn test_move_expression() {
    let bean = || {
        camel_bean::MethodBean::new("MyGuidGenerator")
            .method("guid", |_| Ok(Value::from("123")))
            .into_value()
    };
    // ${id} is the message id
    for (move_to, prefix, suffix) in [
        ("${id}.bak", "", ".bak"),
        (
            "backup-${id}-${file:name.noext}.bak",
            "backup-",
            "-report.bak",
        ),
    ] {
        let dir = TestDirectory::new("FileConsumerMoveExpressionTest");
        let uri = dir.file_uri(&format!(
            "?initialDelay=0&delay=10&exclude=.*bak&move={move_to}"
        ));
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Hello World"]);
        send(&t, &dir, "report.txt", "Hello World").await;
        mock.assert_is_satisfied().await;
        let e = &mock.received_exchanges()[0];
        let id = e
            .message()
            .message_id()
            .unwrap_or(e.exchange_id())
            .to_string();
        let moved = dir.test_file(&format!("{prefix}{id}{suffix}"));
        support_wait(|| moved.exists()).await;
        assert!(
            moved.exists(),
            "File should have been renamed: {}",
            moved.display()
        );
    }
    for (path, move_to, expected) in [
        (
            "",
            "backup/${bean:myguidgenerator.guid}.txt",
            "backup/123.txt",
        ),
        (
            "test",
            "../backup/${file:name}.bak",
            "backup/report.txt.bak",
        ),
    ] {
        let dir = TestDirectory::new("FileConsumerMoveExpressionTest");
        let t = ContextTestSupport::new();
        t.context
            .registry()
            .unwrap()
            .bind("myguidgenerator", bean())
            .unwrap();
        let uri = dir.file_uri(&format!(
            "{path}?initialDelay=0&delay=10&exclude=.*bak&move={move_to}"
        ));
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from(&uri).convert_body_to("String").to("mock:result"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Bye Big World"]);
        mock.expected_file_exists(dir.test_file(expected), Some("Bye Big World"));
        let name = if path.is_empty() {
            "report.txt".to_string()
        } else {
            format!("{path}/report.txt")
        };
        send(&t, &dir, &name, "Bye Big World").await;
        mock.assert_is_satisfied().await;
    }
}

/// The `AntPathMatcherGenericFileFilterTest` cases of the antInclude, antExclude and
/// antFilterCaseSensitive options.
#[tokio::test]
async fn test_ant_filter() {
    let files = |names: &[&str]| -> Vec<(String, String)> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| (format!("x/y/z/{n}"), format!("Hello World {}", i + 1)))
            .collect()
    };
    for (option, names, expected) in [
        (
            "antInclude=**/*.txt&antFilterCaseSensitive=true",
            files(&["report.txt", "b.TXT"]),
            vec!["Hello World 1"],
        ),
        (
            "antExclude=**/*.bak",
            files(&["report.bak", "report.txt", "b.BAK", "b.TXT"]),
            vec!["Hello World 2", "Hello World 3", "Hello World 4"],
        ),
        (
            "antInclude=**/*.pdf,**/*.txt&antExclude=**/a*,**/b*",
            files(&[
                "a.pdf", "m.pdf", "b.txt", "m.txt", "b.bak", "m.bak", "ay.PDF", "my.Pdf", "by.TXT",
                "my.TxT", "by.BAK", "my.BaK",
            ]),
            vec!["Hello World 2", "Hello World 4"],
        ),
        (
            "antInclude=**/*.txt&antFilterCaseSensitive=false",
            files(&["report.TXT"]),
            vec!["Hello World 1"],
        ),
        (
            "antExclude=**/*.bak&antFilterCaseSensitive=false",
            files(&["report.bak", "report.txt", "b.BAK", "b.TXT"]),
            vec!["Hello World 2", "Hello World 4"],
        ),
    ] {
        let files: Vec<(&str, &str)> = names
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_str()))
            .collect();
        consume(
            "AntPathMatcherGenericFileFilterTest",
            &format!("?initialDelay=0&delay=10&recursive=true&{option}"),
            &files,
            &expected,
            &[],
        )
        .await;
    }
}

/// A context with the beans in the registry.
fn with_file_beans(beans: Vec<(&str, Value)>) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    let registry = t.context.registry().unwrap();
    for (name, bean) in beans {
        registry.bind(name, bean).unwrap();
    }
    t
}

/// Ports of `FileConsumerFileFilterTest` and `FileConsumerFileFilterOptimizedTest` (a filter
/// bean on the name).
#[tokio::test]
async fn test_file_filter_bean() {
    for with_regular in [false, true] {
        let dir = TestDirectory::new("FileConsumerFileFilterTest");
        let filter: camel_file::SharedFileFilter =
            std::sync::Arc::new(|f: &camel_file::GenericFile| {
                f.directory || !f.relative_file_path.starts_with("skip")
            });
        let t = with_file_beans(vec![("myFilter", camel_file::filter_value(filter))]);
        let uri = dir.file_uri("?initialDelay=0&delay=10&filter=#myFilter");
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from(&uri).convert_body_to("String").to("mock:result"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:result");
        send(&t, &dir, "skipme.txt", "This is a file to be filtered").await;
        if with_regular {
            mock.expected_bodies_received(["Hello World"]);
            send(&t, &dir, "hello.txt", "Hello World").await;
        } else {
            mock.expected_message_count(0);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        mock.assert_is_satisfied().await;
    }
}

/// Port of `FileConsumerDirectoryFilterTest`: the filter is asked for the directories too.
#[tokio::test]
async fn test_directory_filter_bean() {
    let dir = TestDirectory::new("FileConsumerDirectoryFilterTest");
    let names = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::new()));
    let seen = names.clone();
    let filter: camel_file::SharedFileFilter =
        std::sync::Arc::new(move |f: &camel_file::GenericFile| {
            seen.lock().unwrap().insert(f.relative_file_path.clone());
            !(f.directory && f.relative_file_path.starts_with("skip"))
        });
    let t = with_file_beans(vec![("myFilter", camel_file::filter_value(filter))]);
    let uri = dir.file_uri("?recursive=true&filter=#myFilter&initialDelay=0&delay=10");
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    send(
        &t,
        &dir,
        "skipDir/skipme.txt",
        "This is a file to be filtered",
    )
    .await;
    send(
        &t,
        &dir,
        "skipDir2/skipme.txt",
        "This is a file to be filtered",
    )
    .await;
    send(&t, &dir, "okDir/hello.txt", "Hello World").await;
    mock.assert_is_satisfied().await;
    let names: Vec<String> = names.lock().unwrap().iter().cloned().collect();
    assert_eq!(
        vec!["okDir", "okDir/hello.txt", "skipDir", "skipDir2"],
        names
    );
}

/// Port of `FileSorterRefTest`.
#[tokio::test]
async fn test_sorter_bean() {
    let dir = TestDirectory::new("FileSorterRefTest");
    let sorter: camel_file::SharedFileSorter =
        std::sync::Arc::new(|a: &camel_file::GenericFile, b: &camel_file::GenericFile| {
            a.relative_file_path
                .to_lowercase()
                .cmp(&b.relative_file_path.to_lowercase())
        });
    let t = with_file_beans(vec![("mySorter", camel_file::sorter_value(sorter))]);
    t.context.start().await.unwrap();
    for (name, body) in [
        ("paris.txt", "Hello Paris"),
        ("london.txt", "Hello London"),
        ("copenhagen.txt", "Hello Copenhagen"),
    ] {
        send(&t, &dir, name, body).await;
    }
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello Copenhagen", "Hello London", "Hello Paris"]);
    let uri = dir.file_uri("?initialDelay=0&delay=10&sorter=#mySorter");
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(from(&uri).convert_body_to("String").to("mock:result"));
        })
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// The `AntPathMatcherGenericFileFilterTest` cases with filter beans.
#[tokio::test]
async fn test_ant_filter_beans() {
    let files = [
        ("x/y/z/a.txt", "Hello World 1"),
        ("x/y/z/b.txt", "Hello World 2"),
        ("x/y/z/c.txt", "Hello World 3"),
        ("x/y/z/Cy.txt", "Hello World 4"),
    ];
    for (bean, expected) in [
        (
            camel_file::AntPathMatcherGenericFileFilter::including(&["**/c*"]),
            vec!["Hello World 3"],
        ),
        (
            camel_file::AntPathMatcherGenericFileFilter {
                case_insensitive: true,
                ..camel_file::AntPathMatcherGenericFileFilter::including(&["**/c*"])
            },
            vec!["Hello World 3", "Hello World 4"],
        ),
    ] {
        let dir = TestDirectory::new("AntPathMatcherGenericFileFilterTest");
        let filter: camel_file::SharedFileFilter = std::sync::Arc::new(bean);
        let t = with_file_beans(vec![("filter", camel_file::filter_value(filter))]);
        t.context.start().await.unwrap();
        for (name, body) in files {
            send(&t, &dir, name, body).await;
        }
        let mock = t.mock("mock:result");
        mock.expected_bodies_received_in_any_order(expected);
        let uri = dir.file_uri("?initialDelay=0&delay=10&recursive=true&antInclude=**/*.txt&antExclude=**/a*&filter=#filter");
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from(&uri).convert_body_to("String").to("mock:result"));
            })
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }
}

/// Ports of `FileMarkerFileRecursiveFilterDeleteOldLockFilesTest` and
/// `FileMarkerFileRecursiveFilterDeleteOldLockFilesIncludeTest`: the filtered files and their
/// lock files are left.
#[tokio::test]
async fn test_marker_file_recursive_filter_old_lock_files() {
    use camel_core::api::exchange_constants::FILE_NAME_ONLY;
    for (option, hi_name) in [
        ("&filter=#myFilter", "hi.txt"),
        ("&include=.*(hello.txt|bye.txt|gooday.txt)$", "gooday.txt"),
    ] {
        let dir = TestDirectory::new("FileMarkerFileRecursiveFilterDeleteOldLockFilesTest");
        let filter: camel_file::SharedFileFilter =
            std::sync::Arc::new(|f: &camel_file::GenericFile| {
                !(f.directory && f.relative_file_path == "bar")
            });
        let t = with_file_beans(vec![("myFilter", camel_file::filter_value(filter))]);
        let uri = dir.file_uri(&format!(
            "?initialDelay=0&delay=10&recursive=true&sortBy=file:name{option}"
        ));
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(
                    from(&uri)
                        .route_id("foo")
                        .auto_startup(false)
                        .convert_body_to("String")
                        .to("mock:result"),
                );
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:result");
        mock.expected_bodies_received(["Bye World", "Hi World"]);
        mock.message_matches(0, header(FILE_NAME_ONLY).is_equal_to("bye.txt"));
        mock.message_matches(1, header(FILE_NAME_ONLY).is_equal_to(hi_name));
        mock.expected_file_exists(dir.test_file("bar/davs.txt"), None);
        mock.expected_file_exists(dir.test_file("bar/davs.txt.camelLock"), None);
        for (name, body) in [
            ("hello.txt.camelLock", "locked"),
            ("bye.txt", "Bye World"),
            ("foo/gooday.txt.camelLock", "locked"),
            (&format!("foo/{hi_name}") as &str, "Hi World"),
            ("bar/davs.txt.camelLock", "locked"),
            ("bar/davs.txt", "Davs World"),
        ] {
            send(&t, &dir, name, body).await;
        }
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
    }
}
