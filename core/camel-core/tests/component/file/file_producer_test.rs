//! Ports of `FileProducerFilenameConstantTest`, `NewFileProduceTest`,
//! `FileProducerFileExistIgnoreTest`, `FileProducerFileExistOverrideTest`,
//! `FileProducerFileExistTryRenameTest`, `FileProduceAppendCharsTest` and
//! `FileHeaderFileNameProducedTest`.

use crate::support::{ContextTestSupport, TestDirectory};
use camel_core::api::exchange_constants::{FILE_NAME, FILE_NAME_PRODUCED};
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
