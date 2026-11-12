//! Ports of `FileProducerFilenameConstantTest`, `NewFileProduceTest`,
//! `FileProducerFileExistIgnoreTest`, `FileProducerFileExistOverrideTest`,
//! `FileProducerFileExistTryRenameTest`, `FileProduceAppendCharsTest` and
//! `FileHeaderFileNameProducedTest`.

use crate::support::{ContextTestSupport, TestDirectory};
use camel_core::api::exchange_constants::{FILE_NAME, FILE_NAME_PRODUCED, OVERRULE_FILE_NAME};
use camel_core::prelude::*;

#[tokio::test]
async fn test_file_producer_filename_constant_and_header() {
    let dir = TestDirectory::new("FileProducerFilenameConstantTest");
    let t = ContextTestSupport::new();
    t.template
        .send_body(&dir.file_uri("?fileName=header.txt"), "Hello World")
        .await
        .unwrap();
    assert_eq!(Some("Hello World".to_string()), dir.read("header.txt"));
    t.template
        .send_body_and_header(&dir.file_uri(""), "Bye World", FILE_NAME, "bye.txt")
        .await
        .unwrap();
    assert_eq!(Some("Bye World".to_string()), dir.read("bye.txt"));
}

/// The file exists: Ignore, Override and TryRename (consumed by a noop route started after).
#[tokio::test]
async fn test_file_exist() {
    for (option, expected) in [
        ("?fileExist=Ignore", "Hello World"),
        ("?fileExist=Override", "Bye World"),
        ("?fileExist=TryRename&tempPrefix=tmp", "Bye World"),
    ] {
        let dir = TestDirectory::new("FileProducerFileExistTest");
        let uri = dir.file_uri("?noop=true&initialDelay=0&delay=10");
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
        mock.expected_bodies_received([expected]);
        mock.expected_file_exists(dir.test_file("hello.txt"), Some(expected));
        t.template
            .send_body_and_header(&dir.file_uri(""), "Hello World", FILE_NAME, "hello.txt")
            .await
            .unwrap();
        t.template
            .send_body_and_header(&dir.file_uri(option), "Bye World", FILE_NAME, "hello.txt")
            .await
            .unwrap();
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
        t.context.stop().await.unwrap();
    }
}

#[tokio::test]
async fn test_append_chars() {
    let dir = TestDirectory::new("FileProduceAppendCharsTest");
    let uri = dir.file_uri("?fileExist=Append&appendChars=\\n");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header(FILE_NAME, constant("hello.txt"))
                .to(&uri)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(3);
    mock.expected_file_exists(
        dir.test_file("hello.txt"),
        Some("Hello\nWorld\nHow are you?\n"),
    );
    for body in ["Hello", "World", "How are you?"] {
        t.template.send_body("direct:start", body).await.unwrap();
    }
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_header_file_name_produced() {
    let dir = TestDirectory::new("FileHeaderFileNameProducedTest");
    let uri = dir.file_uri("target?fileExist=Override");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:in").to(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.message_matches(0, header(FILE_NAME_PRODUCED).contains("target"));
    t.template
        .request_body_and_header(
            "direct:in",
            "Hello World",
            FILE_NAME,
            "FileHeaderFileNameProducedTest.txt",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    assert_eq!(
        Some("Hello World".to_string()),
        dir.read("target/FileHeaderFileNameProducedTest.txt")
    );
}

/// Port of `FileProducerFileExistAppendNoFileBeforeTest`.
#[tokio::test]
async fn test_append_no_file_before() {
    let dir = TestDirectory::new("FileProducerFileExistAppendNoFileBeforeTest");
    let uri = dir.file_uri("?noop=true&initialDelay=100&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Bye World"]);
    mock.expected_file_exists(dir.test_file("hello.txt"), Some("Bye World"));
    t.template
        .send_body_and_header(
            &dir.file_uri("?fileExist=Append"),
            "Bye World",
            FILE_NAME,
            "hello.txt",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileProduceGeneratedFileNameTest`.
#[tokio::test]
async fn test_generated_file_name() {
    let dir = TestDirectory::new("FileProduceGeneratedFileNameTest");
    let uri = dir.file_uri("");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:a").to(&uri));
    })
    .await;
    let mut exchange = t
        .template
        .create_exchange("direct:a", ExchangePattern::InOnly)
        .unwrap();
    exchange.message_mut().set_body("Hello World");
    let id = camel_file::generated_file_name(&exchange);
    t.template.send("direct:a", exchange).await.unwrap();
    assert!(
        dir.test_file(&id).exists(),
        "The generated file should exists: {id}"
    );
}

/// Port of `FilerProducerAbsoluteTest`.
#[tokio::test]
async fn test_produce_absolute_file() {
    let dir = TestDirectory::new("FilerProducerAbsoluteTest");
    let path = format!("{}/absolute", dir.path());
    let uri = format!("file://{path}");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:report").to(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(format!("{path}/hello.txt"), None);
    t.template
        .send_body_and_header("direct:report", "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileProducerFileExistFailTest`.
#[tokio::test]
async fn test_file_exist_fail() {
    let dir = TestDirectory::new("FileProducerFileExistFailTest");
    let uri = dir.file_uri("?noop=true&initialDelay=0&delay=10");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from(&uri).convert_body_to("String").to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_file_exists(dir.test_file("hello.txt"), Some("Hello World"));
    t.template
        .send_body_and_header(&dir.file_uri(""), "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap();
    let e = t
        .template
        .send_body_and_header(
            &dir.file_uri("?fileExist=Fail"),
            "Bye World",
            FILE_NAME,
            "hello.txt",
        )
        .await
        .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert!(cause.is(ErrorKind::GenericFileOperationFailed), "{e}");
    assert_eq!(
        Some(
            format!(
                "File already exist: {}/hello.txt. Cannot write new file.",
                dir.path()
            )
            .as_str()
        ),
        cause.message()
    );
    mock.assert_is_satisfied().await;
}

/// Ports of `FileProducerFileExistAppendTest` (not the case appending `File` bodies) and
/// `FileProducerFileExistOverrideNoFileBeforeTest`.
#[tokio::test]
async fn test_file_exist_append_and_override_no_file() {
    for (first, option, expected) in [
        (
            Some("Hello World\n"),
            "?fileExist=Append",
            "Hello World\nBye World",
        ),
        (None, "?fileExist=Override", "Bye World"),
    ] {
        let dir = TestDirectory::new("FileProducerFileExistAppendTest");
        let uri = dir.file_uri("?noop=true&initialDelay=0&delay=10");
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
        if let Some(body) = first {
            t.template
                .send_body_and_header(&dir.file_uri(""), body, FILE_NAME, "hello.txt")
                .await
                .unwrap();
        }
        let mock = t.mock("mock:result");
        mock.expected_bodies_received([expected]);
        mock.expected_file_exists(dir.test_file("hello.txt"), Some(expected));
        t.template
            .send_body_and_header(&dir.file_uri(option), "Bye World", FILE_NAME, "hello.txt")
            .await
            .unwrap();
        t.context.start_route("foo").await.unwrap();
        mock.assert_is_satisfied().await;
    }
}

async fn put(
    t: &ContextTestSupport,
    dir: &TestDirectory,
    option: &str,
    body: &str,
    name: &str,
) -> CamelResult<()> {
    t.template
        .send_body_and_header(&dir.file_uri(option), body, FILE_NAME, name)
        .await
}

/// Port of `FileProducerMoveExistingTest` (the date based cases check one moved file).
#[tokio::test]
async fn test_move_existing() {
    let t = ContextTestSupport::new();
    let renamed = "?fileExist=Move&moveExisting=${file:parent}/renamed-${file:onlyname}";
    // no existing file
    let dir = TestDirectory::new("FileProducerMoveExistingTest");
    put(&t, &dir, renamed, "Hello World", "hello.txt")
        .await
        .unwrap();
    assert!(dir.test_file("hello.txt").exists());
    assert!(!dir.test_file("renamed-hello.txt").exists());
    // existing file, with or without temp file name, to a file or a directory
    for (option, moved) in [
        (renamed.to_string(), "renamed-hello.txt"),
        (
            format!("?tempFileName=${{file:onlyname}}.temp&{}", &renamed[1..]),
            "renamed-hello.txt",
        ),
        (
            "?tempFileName=${file:onlyname}.temp&fileExist=Move&moveExisting=renamed".to_string(),
            "renamed/hello.txt",
        ),
        (
            "?fileExist=Move&moveExisting=backup".to_string(),
            "backup/hello.txt",
        ),
        (
            "?tempFileName=${file:onlyname}.temp&fileExist=Move&moveExisting=archive/".to_string(),
            "archive/hello.txt",
        ),
    ] {
        let dir = TestDirectory::new("FileProducerMoveExistingTest");
        put(&t, &dir, &option, "Hello World", "hello.txt")
            .await
            .unwrap();
        put(&t, &dir, &option, "Bye World", "hello.txt")
            .await
            .unwrap();
        assert_eq!(
            Some("Bye World".to_string()),
            dir.read("hello.txt"),
            "{option}"
        );
        assert_eq!(Some("Hello World".to_string()), dir.read(moved), "{option}");
    }
    // a dynamic name: one moved file
    let dir = TestDirectory::new("FileProducerMoveExistingTest");
    let option = "?tempFileName=${file:onlyname}.temp&fileExist=Move&moveExisting=${file:onlyname}.${date:now:yyyyMMddHHmmssSSS}";
    put(&t, &dir, option, "Hello World1", "howdy.txt")
        .await
        .unwrap();
    put(&t, &dir, option, "Hello World2", "howdy.txt")
        .await
        .unwrap();
    let moved: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("howdy.txt."))
        .collect();
    assert_eq!(1, moved.len(), "{moved:?}");
    assert_eq!(Some("Hello World1".to_string()), dir.read(&moved[0]));
    assert_eq!(Some("Hello World2".to_string()), dir.read("howdy.txt"));
    // the moved name exists: eagerDeleteTargetFile replaces it, else fails
    for eager in [true, false] {
        let dir = TestDirectory::new("FileProducerMoveExistingTest");
        let option = format!("{renamed}&eagerDeleteTargetFile={eager}");
        put(&t, &dir, "", "Old file", "renamed-hello.txt")
            .await
            .unwrap();
        put(&t, &dir, &option, "Hello World", "hello.txt")
            .await
            .unwrap();
        let r = put(&t, &dir, &option, "Bye World", "hello.txt").await;
        if eager {
            r.unwrap();
            assert_eq!(Some("Bye World".to_string()), dir.read("hello.txt"));
            assert_eq!(
                Some("Hello World".to_string()),
                dir.read("renamed-hello.txt")
            );
        } else {
            let e = r.unwrap_err();
            let cause = crate::support::cause_of(&e);
            assert!(cause.is(ErrorKind::GenericFileOperationFailed), "{e}");
            assert!(cause
                .message()
                .unwrap_or_default()
                .starts_with("Cannot move existing file"));
            assert_eq!(Some("Hello World".to_string()), dir.read("hello.txt"));
            assert_eq!(Some("Old file".to_string()), dir.read("renamed-hello.txt"));
        }
    }
}

/// The route cases of `FileProduceTempFileNameTest`.
#[tokio::test]
async fn test_temp_file_name() {
    let dir = TestDirectory::new("FileProduceTempFileNameTest");
    let (a, b, c) = (
        dir.file_uri("tempandrename?tempFileName=inprogress-${file:name.noext}.tmp"),
        dir.file_uri("tempandrename?tempFileName=../work/${file:name.noext}.tmp"),
        dir.file_uri("tempandrename?tempFileName=work/${file:name.noext}.tmp"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:a").to(&a));
        r.add(from("direct:b").to(&b));
        r.add(from("direct:c").to(&c));
    })
    .await;
    t.template
        .send_body_and_header("direct:a", "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap();
    assert!(dir.test_file("tempandrename/hello.txt").exists());
    assert!(!dir.test_file("tempandrename/inprogress-hello.tmp").exists());
    t.template
        .send_body_and_header("direct:b", "Hello World", FILE_NAME, "hello2.txt")
        .await
        .unwrap();
    assert!(dir.test_file("work").is_dir());
    t.template
        .send_body_and_header("direct:c", "Hello World", FILE_NAME, "hello3.txt")
        .await
        .unwrap();
    assert!(dir.test_file("tempandrename/work").is_dir());
}

/// Ports of `FileMEPInOutTest` and `FileProduceAppendCharsEmptyMessageTest`.
#[tokio::test]
async fn test_in_out_and_append_chars_empty_message() {
    let dir = TestDirectory::new("FileMEPInOutTest");
    let (a, b) = (
        dir.file_uri("?fileExist=Override"),
        dir.file_uri("?fileExist=Append&appendChars=@@@"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:in").to(&a).to("mock:result"));
        r.add(
            from("direct:start")
                .set_header(FILE_NAME, constant("hello.txt"))
                .to(&b)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["Hello World"]);
    mock.expected_file_exists(dir.test_file("FileMEPInOutTest.txt"), Some("Hello World"));
    t.template
        .request_body_and_header(
            "direct:in",
            "Hello World",
            FILE_NAME,
            "FileMEPInOutTest.txt",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("hello.txt"), Some("@@@"));
    t.template.send_body("direct:start", "").await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Ports of `FileProduceAppendTest` and `FileProduceAppendCharsSingleMessageTest`.
#[tokio::test]
async fn test_append_text_and_append_chars_single_message() {
    let dir = TestDirectory::new("FileProduceAppendTest");
    let (a, b) = (
        dir.file_uri("?fileExist=Append"),
        dir.file_uri("?fileExist=Append&appendChars=@@@"),
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header(FILE_NAME, constant("hello.txt"))
                .to(&a)
                .to("mock:result"),
        );
        r.add(
            from("direct:chars")
                .set_header(FILE_NAME, constant("chars.txt"))
                .to(&b)
                .to("mock:result"),
        );
    })
    .await;
    put(&t, &dir, "", "Hello", "hello.txt").await.unwrap();
    put(&t, &dir, "", " World", "world.txt").await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("hello.txt"), Some("Hello World"));
    t.template
        .send_body("direct:start", " World")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_message_count(1);
    mock.expected_file_exists(dir.test_file("chars.txt"), Some("Hello@@@"));
    t.template.send_body("direct:chars", "Hello").await.unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileProducerToDMoveExistingTest`.
#[tokio::test]
async fn test_to_d_move_existing() {
    let dir = TestDirectory::new("FileProducerToDMoveExistingTest");
    let uri = dir.file_uri(&format!(
        "${{header.myDir}}?fileExist=Move&moveExisting={}/out/old-${{file:onlyname}}",
        dir.path()
    ));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:start").to_d(&uri).to("mock:result"));
    })
    .await;
    t.mock("mock:result").expected_message_count(2);
    for body in ["Hello World", "Bye World"] {
        t.template
            .send_body_and_headers(
                "direct:start",
                body,
                vec![
                    ("myDir".into(), "out".into()),
                    (FILE_NAME.into(), "hello.txt".into()),
                ],
            )
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
    assert!(dir.test_file("out/old-hello.txt").exists());
    assert!(dir.test_file("out/hello.txt").exists());
}

/// Port of `FileProduceOverruleOnlyOnceTest`.
#[tokio::test]
async fn test_overrule_only_once() {
    let dir = TestDirectory::new("FileProduceOverruleOnlyOnceTest");
    let (write, again) = (dir.file_uri("write"), dir.file_uri("again"));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:start").to(&write).to(&again).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_header_received(FILE_NAME, "hello.txt");
    mock.message_matches(0, header(OVERRULE_FILE_NAME).is_null());
    mock.expected_file_exists(dir.test_file("write/ruled.txt"), Some("Hello World"));
    mock.expected_file_exists(dir.test_file("again/hello.txt"), Some("Hello World"));
    t.template
        .send_body_and_headers(
            "direct:start",
            "Hello World",
            vec![
                (FILE_NAME.into(), "hello.txt".into()),
                (OVERRULE_FILE_NAME.into(), "ruled.txt".into()),
            ],
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FilerProducerFileNamesTest`.
#[tokio::test]
async fn test_producer_file_names() {
    let dir = TestDirectory::new("FilerProducerFileNamesTest");
    let (reports, root) = (dir.file_uri("reports"), dir.file_uri(""));
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:report").to(&reports));
        r.add(
            from("direct:report2")
                .set_header(FILE_NAME, constant("report-super.txt"))
                .to(&root),
        );
    })
    .await;
    let mut exchange = t
        .template
        .create_exchange("direct:report", ExchangePattern::InOnly)
        .unwrap();
    exchange.message_mut().set_body("This is a good report");
    let id = camel_file::generated_file_name(&exchange);
    t.template.send("direct:report", exchange).await.unwrap();
    assert!(dir.test_file(&format!("reports/{id}")).exists());
    t.template
        .send_body("direct:report2", "This is super good report")
        .await
        .unwrap();
    assert!(dir.test_file("report-super.txt").exists());
}

/// Port of `FileProducerJailStartingDirectoryTest`.
#[tokio::test]
async fn test_jail_starting_directory() {
    let dir = TestDirectory::new("FileProducerJailStartingDirectoryTest");
    let outbox = dir.file_uri("outbox");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header(FILE_NAME, simple("../${file:name}"))
                .to(&outbox)
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(0);
    let e = t
        .template
        .send_body_and_header("direct:start", "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
    assert!(
        cause
            .message()
            .unwrap_or_default()
            .contains("as the filename is jailed to the starting directory"),
        "{e}"
    );
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_message_count(1);
    t.template
        .send_body_and_header("direct:start", "Bye World", FILE_NAME, "outbox/bye.txt")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileEagerDeleteTargetFileTest`.
#[tokio::test]
async fn test_eager_delete_target_file() {
    let t = ContextTestSupport::new();
    for eager in [
        "&eagerDeleteTargetFile=true",
        "&eagerDeleteTargetFile=false",
        "",
    ] {
        let dir = TestDirectory::new("FileEagerDeleteTargetFileTest");
        put(&t, &dir, "", "Hello World", "world.txt").await.unwrap();
        put(
            &t,
            &dir,
            &format!("?tempFileName=inprogress-${{file:name}}{eager}"),
            "Bye World",
            "world.txt",
        )
        .await
        .unwrap();
        assert_eq!(
            Some("Bye World".to_string()),
            dir.read("world.txt"),
            "{eager}"
        );
    }
}

/// Port of `FileProduceOverruleExpressionTest`.
#[tokio::test]
async fn test_overrule_expression() {
    let dir = TestDirectory::new("FileProduceOverruleExpressionTest");
    let uri = dir.file_uri("?fileName=copy-of-${file:name}");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:start").to(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(1);
    mock.expected_header_received(FILE_NAME, "hello.txt");
    mock.expected_file_exists(dir.test_file("copy-of-hello.txt"), Some("Hello World"));
    t.template
        .send_body_and_header("direct:start", "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    mock.reset();
    mock.expected_message_count(1);
    mock.expected_header_received(FILE_NAME, "hello.txt");
    mock.message_matches(0, header(OVERRULE_FILE_NAME).is_null());
    mock.expected_file_exists(dir.test_file("copy-of-overruled.txt"), Some("Hello World"));
    t.template
        .send_body_and_headers(
            "direct:start",
            "Hello World",
            vec![
                (FILE_NAME.into(), "hello.txt".into()),
                (OVERRULE_FILE_NAME.into(), "overruled.txt".into()),
            ],
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileProducerAllowNullBodyFileAlreadyExistsTest`.
#[tokio::test]
async fn test_allow_null_body_file_already_exists() {
    for (option, expected) in [("Append", "Hello world"), ("Override", "")] {
        let dir = TestDirectory::new("FileProducerAllowNullBodyFileAlreadyExistsTest");
        let uri = dir.file_uri(&format!("?allowNullBody=true&fileExist={option}"));
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .set_header(FILE_NAME, constant("hello.txt"))
                    .to(&uri)
                    .to("mock:result"),
            );
        })
        .await;
        put(&t, &dir, "", "Hello world", "hello.txt").await.unwrap();
        let mock = t.mock("mock:result");
        mock.expected_message_count(1);
        mock.expected_file_exists(dir.test_file("hello.txt"), Some(expected));
        t.template
            .send_body("direct:start", Value::Null)
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }
}

/// Port of `DirectoryCreateIssueTest`.
#[tokio::test]
async fn test_directory_create_issue() {
    let dir = TestDirectory::new("DirectoryCreateIssueTest");
    let uri = dir.file_uri("a/b/c/d/e/f/g/h/?fileExist=Override&noop=true");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        let mut all = from("seda:testFileCreatedAsDir");
        for i in 0..10 {
            r.add(
                from(&format!("direct:file{i}"))
                    .set_header(FILE_NAME, constant(format!("file{i}.txt")))
                    .to(&uri)
                    .to("mock:result"),
            );
            all = all.to(&format!("direct:file{i}"));
        }
        r.add(all);
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_message_count(10);
    t.template
        .send_body("seda:testFileCreatedAsDir", "Contents of test file")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    for i in 0..10 {
        assert!(dir
            .test_file(&format!("a/b/c/d/e/f/g/h/file{i}.txt"))
            .is_file());
    }
}

/// Ports of `FilerProducerDoneFileNameTest` and `FilerProducerDoneFileNameRouteTest`.
#[tokio::test]
async fn test_producer_done_file_name() {
    let t = ContextTestSupport::new();
    for (option, done) in [
        ("?doneFileName=xdone", "xdone"),
        ("?doneFileName=done-${file:name}", "done-hello.foo.txt"),
        ("?doneFileName=${file:name}.done", "hello.foo.txt.done"),
        ("?doneFileName=${file:name.noext}.done", "hello.foo.done"),
    ] {
        let dir = TestDirectory::new("FilerProducerDoneFileNameTest");
        put(&t, &dir, option, "Hello World", "hello.foo.txt")
            .await
            .unwrap();
        assert!(dir.test_file("hello.foo.txt").exists());
        assert!(dir.test_file(done).exists(), "{option}");
    }
    // invalid and empty done file names
    let dir = TestDirectory::new("FilerProducerDoneFileNameTest");
    let e = put(
        &t,
        &dir,
        "?doneFileName=${file:parent}/foo",
        "Hello World",
        "hello.txt",
    )
    .await
    .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert!(cause.is(ErrorKind::ExpressionIllegalSyntax), "{e}");
    assert!(
        cause
            .message()
            .unwrap_or_default()
            .ends_with("Cannot resolve reminder: ${file:parent}/foo"),
        "{e}"
    );
    let e = put(&t, &dir, "?doneFileName=", "Hello World", "hello.txt")
        .await
        .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
    assert!(
        cause
            .message()
            .unwrap_or_default()
            .starts_with("doneFileName must be specified and not empty"),
        "{e}"
    );
    // a placeholder in the uri, sent directly and from a route
    let dir = TestDirectory::new("FilerProducerDoneFileNameTest");
    t.context
        .properties()
        .add_initial_property("myDir", dir.path());
    t.template
        .send_body_and_header(
            "file:{{myDir}}?doneFileName=done-${file:name}",
            "Hello World",
            FILE_NAME,
            "hello.txt",
        )
        .await
        .unwrap();
    assert!(dir.test_file("hello.txt").exists());
    assert!(dir.test_file("done-hello.txt").exists());
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .to("file:{{myDir}}?doneFileName=done-${file:name}")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result").expected_message_count(1);
    t.template
        .send_body_and_header("direct:start", "Hello World", FILE_NAME, "route.txt")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert!(dir.test_file("route.txt").exists());
    assert!(dir.test_file("done-route.txt").exists());
}

/// Ports of `BeanToFileTest` and `ToFileRouteTest` (a bean method and a processor bean before
/// the file producer).
#[tokio::test]
async fn test_bean_and_processor_to_file() {
    use camel_bean::{processor_value, MethodBean};
    let dir = TestDirectory::new("BeanToFileTest");
    let t = ContextTestSupport::new();
    let registry = t.context.registry().unwrap();
    let my_bean = MethodBean::new("MyBean")
        .method("doSomething", |inv| {
            let input = inv.parameter_as::<String>(0)?.unwrap_or_default();
            Ok(Value::from(format!("Bye {input}")))
        })
        .into_value();
    registry.bind("myBean", my_bean).unwrap();
    let report = processor_value(std::sync::Arc::new(
        camel_core::api::processor::processor_fn(|e: &mut Exchange| {
            let body = e.message().body().to_string();
            e.message_mut().set_body(body);
            e.message_mut().set_header(FILE_NAME, "report.txt");
            Ok(())
        }),
    ));
    registry.bind("processReport", report).unwrap();
    let (a, b) = (dir.file_uri("?fileExist=Override"), dir.file_uri(""));
    t.context
        .add_routes(move |r: &mut Routes| {
            r.add(
                from("direct:in")
                    .to("bean:myBean")
                    .set_header(FILE_NAME, constant("bean.txt"))
                    .to(&a)
                    .to("mock:result"),
            );
            r.add(
                from("direct:reports")
                    .process_ref("processReport")
                    .to(&b)
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_message_count(2);
    mock.expected_file_exists(dir.test_file("bean.txt"), Some("Bye World"));
    mock.expected_file_exists(dir.test_file("report.txt"), None);
    t.template.send_body("direct:in", "World").await.unwrap();
    t.template
        .send_body("direct:reports", "This is a great report")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileProduceOverruleTest`.
#[tokio::test]
async fn test_produce_overrule() {
    let dir = TestDirectory::new("FileProduceOverruleTest");
    let uri = dir.file_uri("");
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:start").to(&uri).to("mock:result"));
    })
    .await;
    let mock = t.mock("mock:result");
    // no overrule
    mock.expected_header_received(FILE_NAME, "hello.txt");
    mock.expected_file_exists(dir.test_file("hello.txt"), Some("Hello World"));
    t.template
        .send_body_and_header("direct:start", "Hello World", FILE_NAME, "hello.txt")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    // only the overrule
    mock.reset();
    mock.expected_message_count(1);
    mock.message_matches(0, header(FILE_NAME).is_null());
    mock.expected_file_exists(dir.test_file("overrule.txt"), Some("Hello World"));
    t.template
        .send_body_and_header(
            "direct:start",
            "Hello World",
            OVERRULE_FILE_NAME,
            "overrule.txt",
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    // both
    mock.reset();
    mock.expected_header_received(FILE_NAME, "hello.txt");
    mock.message_matches(0, header(OVERRULE_FILE_NAME).is_null());
    mock.expected_file_exists(dir.test_file("ruled.txt"), Some("Hello World"));
    t.template
        .send_body_and_headers(
            "direct:start",
            "Hello World",
            vec![
                (FILE_NAME.into(), "hello.txt".into()),
                (OVERRULE_FILE_NAME.into(), "ruled.txt".into()),
            ],
        )
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

/// Port of `FileProducerAllowNullBodyTest`.
#[tokio::test]
async fn test_allow_null_body() {
    let dir = TestDirectory::new("FileProducerAllowNullBodyTest");
    let t = ContextTestSupport::new();
    let allow = dir.file_uri("?allowNullBody=true&fileName=allowNullBody.txt");
    t.template.send_body(&allow, Value::Null).await.unwrap();
    let path = dir.test_file("allowNullBody.txt");
    assert_eq!(0, std::fs::metadata(&path).unwrap().len());
    // a null body truncates the file
    t.template.send_body(&allow, "Hello").await.unwrap();
    assert_ne!(0, std::fs::metadata(&path).unwrap().len());
    t.template.send_body(&allow, Value::Null).await.unwrap();
    assert_eq!(0, std::fs::metadata(&path).unwrap().len());
    // not allowed
    std::fs::remove_file(&path).unwrap();
    let e = t
        .template
        .send_body(&dir.file_uri("?fileName=allowNullBody.txt"), Value::Null)
        .await
        .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert!(cause.is(ErrorKind::GenericFileOperationFailed), "{e}");
    assert!(
        cause
            .message()
            .unwrap_or_default()
            .ends_with("allowNullBody.txt"),
        "{e}"
    );
    assert!(
        !path.exists(),
        "allowNullBody set to false with null body should not create a new file"
    );
}

/// Port of `FileProducerTempFileExistsIssueTest`.
#[tokio::test]
async fn test_temp_file_exists_issue() {
    let t = ContextTestSupport::new();
    let dir = TestDirectory::new("FileProducerTempFileExistsIssueTest");
    for option in [
        "?fileExist=Append&tempPrefix=foo",
        "?fileExist=Append&tempFileName=foo",
    ] {
        let e = match t
            .context
            .endpoint(&dir.file_uri(option))
            .unwrap()
            .create_producer()
        {
            Err(e) => e,
            Ok(_) => panic!("{option} should fail"),
        };
        assert!(e.is(ErrorKind::IllegalArgument), "{e}");
        assert_eq!(
            Some("You cannot set both fileExist=Append and tempPrefix/tempFileName options"),
            e.message()
        );
    }
    for (option, other, expected) in [
        ("?tempPrefix=foo", false, "Bye World"),
        ("?tempPrefix=foo", true, "Bye World"),
        ("?tempPrefix=foo&fileExist=Override", false, "Bye World"),
        ("?tempPrefix=foo&fileExist=Ignore", false, "Hello World"),
    ] {
        let dir = TestDirectory::new("FileProducerTempFileExistsIssueTest");
        put(&t, &dir, "", "Hello World", "hello.txt").await.unwrap();
        if other {
            put(&t, &dir, "", "Hello World", "foohello.txt")
                .await
                .unwrap();
        }
        put(&t, &dir, option, "Bye World", "hello.txt")
            .await
            .unwrap();
        assert_eq!(
            Some(expected.to_string()),
            dir.read("hello.txt"),
            "{option}"
        );
    }
    let dir = TestDirectory::new("FileProducerTempFileExistsIssueTest");
    put(&t, &dir, "", "Hello World", "hello.txt").await.unwrap();
    let e = put(
        &t,
        &dir,
        "?tempPrefix=foo&fileExist=Fail",
        "Bye World",
        "hello.txt",
    )
    .await
    .unwrap_err();
    let cause = crate::support::cause_of(&e);
    assert!(cause.is(ErrorKind::GenericFileOperationFailed), "{e}");
    assert!(
        cause
            .message()
            .unwrap_or_default()
            .starts_with("File already exist"),
        "{e}"
    );
    assert_eq!(Some("Hello World".to_string()), dir.read("hello.txt"));
}

/// Port of `FromFileMulticastToFilesTest`.
#[tokio::test]
async fn test_from_file_multicast_to_files() {
    for parallel in [false, true] {
        let dir = TestDirectory::new("FromFileMulticastToFilesTest");
        let (from_uri, header_uri, footer_uri) = (
            dir.file_uri("?initialDelay=0&delay=10"),
            dir.file_uri("out/?fileName=header.txt"),
            dir.file_uri("out/?fileName=footer.txt"),
        );
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            let route = from(&from_uri).multicast(|s| {
                s.pipeline(|p| {
                    p.transform(body().prepend("HEADER:"))
                        .to(&header_uri)
                        .to("mock:header")
                })
                .pipeline(|p| {
                    p.transform(body().prepend("FOOTER:"))
                        .to(&footer_uri)
                        .to("mock:footer")
                })
            });
            let route = if parallel {
                route.parallel_processing()
            } else {
                route
            };
            r.add(route.to("mock:end"));
        })
        .await;
        let header = t.mock("mock:header");
        header.expected_bodies_received(["HEADER:foo"]);
        header.expected_file_exists(dir.test_file("out/header.txt"), None);
        let footer = t.mock("mock:footer");
        footer.expected_bodies_received(["FOOTER:foo"]);
        footer.expected_file_exists(dir.test_file("out/footer.txt"), None);
        let end = t.mock("mock:end");
        end.expected_message_count(1);
        end.expected_file_exists(dir.test_file(".camel/foo.txt"), None);
        put(&t, &dir, "", "foo", "foo.txt").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `FileURLDecodingTest`: the fileName option is URL decoded unless RAW.
#[tokio::test]
async fn test_url_decoding() {
    for (encoded, expected) in [
        ("data.txt", "data.txt"),
        ("data .txt", "data .txt"),
        ("data%20.txt", "data .txt"),
        ("RAW(data%2B.txt)", "data%2B.txt"),
        ("RAW(data+.txt)", "data+.txt"),
        ("RAW(data%20.txt)", "data%20.txt"),
        ("RAW(data%%.txt)", "data%%.txt"),
    ] {
        let dir = TestDirectory::new("FileURLDecodingTest");
        let uri = dir.file_uri(&format!("?fileName={encoded}"));
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(from("direct:start").to(&uri));
        })
        .await;
        let result = t
            .template
            .request_body("direct:start", "Kermit")
            .await
            .unwrap();
        assert_eq!("Kermit", result.to_string());
        assert_eq!(Some("Kermit".to_string()), dir.read(expected), "{encoded}");
    }
}

/// Port of `FileConcurrentWriteAppendSameFileTest`: parallel appends to the same file.
#[tokio::test]
async fn test_concurrent_write_append_same_file() {
    let dir = TestDirectory::new("FileConcurrentWriteAppendSameFileTest");
    let (from_uri, out) = (
        dir.file_uri("?initialDelay=0&delay=10"),
        dir.file_uri("outbox?fileExist=Append&fileName=result.txt"),
    );
    let t = ContextTestSupport::new();
    t.context
        .add_routes(move |r: &mut Routes| {
            let out = out.clone();
            r.add(
                from(&from_uri)
                    .route_id("foo")
                    .auto_startup(false)
                    .split(body().tokenize("\n"), move |s| {
                        s.set_body(body().append(":Status=OK").append("\n"))
                            .to(&out)
                            .to("mock:result")
                    })
                    .parallel_processing()
                    .streaming(),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    let data: String = (0..100).map(|i| format!("Line {i}\n")).collect();
    put(&t, &dir, "", &data, "input.txt").await.unwrap();
    let mock = t.mock("mock:result");
    mock.expected_message_count(100);
    mock.expects_no_duplicates(body().expression());
    mock.set_result_wait_time(30000);
    t.context.start_route("foo").await.unwrap();
    mock.assert_is_satisfied().await;
    let mut actual: Vec<String> = dir
        .read("outbox/result.txt")
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let mut expected: Vec<String> = data.lines().map(|l| format!("{l}:Status=OK")).collect();
    actual.sort();
    expected.sort();
    assert_eq!(expected, actual);
}

/// Port of `FileToFileWithFlattenTest`: flatten on the consumer or the producer.
#[tokio::test]
async fn test_file_to_file_with_flatten() {
    for (consumer, producer) in [("&flatten=true", ""), ("", "?flatten=true")] {
        let dir = TestDirectory::new("FileToFileWithFlattenTest");
        let t = ContextTestSupport::new();
        for (name, body) in [
            ("bye.txt", "Bye World"),
            ("sub/hello.txt", "Hello World"),
            ("sub/sub2/goodday.txt", "Goodday World"),
        ] {
            put(&t, &dir, "flatten-in", body, name).await.unwrap();
        }
        let (from_uri, to_uri) = (
            dir.file_uri(&format!(
                "flatten-in?initialDelay=0&delay=10&recursive=true{consumer}"
            )),
            dir.file_uri(&format!("flatten-out{producer}")),
        );
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from(&from_uri).to(&to_uri).to("mock:result"));
            })
            .await
            .unwrap();
        t.context.start().await.unwrap();
        let mock = t.mock("mock:result");
        mock.expected_message_count(3);
        for f in [
            "flatten-out/bye.txt",
            "flatten-out/hello.txt",
            "flatten-out/goodday.txt",
            "flatten-in/.camel/bye.txt",
            "flatten-in/sub/.camel/hello.txt",
            "flatten-in/sub/sub2/.camel/goodday.txt",
        ] {
            mock.expected_file_exists(dir.test_file(f), None);
        }
        mock.assert_is_satisfied().await;
        t.context.stop().await.unwrap();
    }
}

/// Port of `FileKeepLastModifiedTest`.
#[tokio::test]
async fn test_keep_last_modified() {
    use camel_core::api::exchange_constants::FILE_LAST_MODIFIED;
    for option in ["?keepLastModified=true", "?keepLastModified=false", ""] {
        let dir = TestDirectory::new("FileKeepLastModifiedTest");
        let (from_uri, out) = (
            dir.file_uri("?noop=true&initialDelay=0&delay=10"),
            dir.file_uri(&format!("out{option}")),
        );
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            r.add(
                from(&from_uri)
                    .delay(constant(10))
                    .to(&out)
                    .to("mock:result"),
            );
        })
        .await;
        let mock = t.mock("mock:result");
        mock.expected_message_count(1);
        mock.message_matches(0, header(FILE_LAST_MODIFIED).is_not_null());
        // an old last modified, so a new write differs
        dir.write("hello.txt", "Hello World");
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        std::fs::File::options()
            .write(true)
            .open(dir.test_file("hello.txt"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        mock.assert_is_satisfied().await;
        let t1 = mock.received_exchanges()[0]
            .message()
            .header(FILE_LAST_MODIFIED)
            .and_then(|v| v.as_i64())
            .unwrap();
        let t2 = std::fs::metadata(dir.test_file("out/hello.txt"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert_eq!(
            option == "?keepLastModified=true",
            t1 == t2,
            "{option}: {t1} {t2}"
        );
    }
}

/// Port of `FileProducerExpressionTest` (not the cases with an expression or a date as header).
#[tokio::test]
async fn test_producer_expression() {
    let dir = TestDirectory::new("FileProducerExpressionTest");
    let t = ContextTestSupport::new();
    let bean = camel_bean::MethodBean::new("MyGuidGenerator")
        .method("guid", |_| Ok(Value::from("123")))
        .into_value();
    t.context
        .registry()
        .unwrap()
        .bind("myguidgenerator", bean)
        .unwrap();
    // the header is not evaluated
    put(&t, &dir, "", "Hello World", "$simple{myfile-${id}}.txt")
        .await
        .unwrap();
    assert!(dir.test_file("$simple{myfile-${id}}.txt").exists());
    t.template
        .send_body(
            &dir.file_uri("?fileName=${bean:myguidgenerator}.bak"),
            "Hello World",
        )
        .await
        .unwrap();
    assert!(dir.test_file("123.bak").exists());
    // the date of today
    let mut e = Exchange::new(t.context.clone());
    let today = t
        .context
        .resolve_language("simple")
        .unwrap()
        .create_expression("${date:now:yyyyMMdd}")
        .unwrap()
        .evaluate_mut(&mut e)
        .unwrap()
        .to_string();
    t.template
        .send_body(
            &dir.file_uri("?fileName=myfile-${date:now:yyyyMMdd}.txt"),
            "Hello World",
        )
        .await
        .unwrap();
    assert!(dir.test_file(&format!("myfile-{today}.txt")).exists());
    t.template
        .send_body(
            &dir.file_uri("?jailStartingDirectory=false&fileName=target/filelanguageinbox/myfile-${bean:myguidgenerator.guid}-${date:now:yyyyMMdd}.txt"),
            "Hello World",
        )
        .await
        .unwrap();
    assert!(dir
        .test_file(&format!("target/filelanguageinbox/myfile-123-{today}.txt"))
        .exists());
    t.template
        .send_body_and_header(
            &dir.file_uri("?fileName=myfile-${in.header.foo}.txt"),
            "Hello World",
            "foo",
            "abc",
        )
        .await
        .unwrap();
    assert!(dir.test_file("myfile-abc.txt").exists());
}
