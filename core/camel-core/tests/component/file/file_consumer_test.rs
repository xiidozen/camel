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
