//! Ports of the camel-zipfile data format and splitter tests: `ZipFileDataFormatTest` (the
//! archives compared by their entries; the stream cache case has no Rust equivalent),
//! `ZipFileIteratorDataFormatTest`, `ZipFileSplitIteratorEmptyTest`,
//! `ZipFileSplitIteratorCorruptTest` and `ZipSplitterRouteTest` (but its JVM memory cases).

use camel_core::api::error::ErrorType;
use camel_core::api::exchange_constants::{EXCEPTION_CAUGHT, FILE_NAME};
use camel_core::api::spi::data_format::SharedDataFormat;
use camel_core::model::expression::ExpressionDefinition;
use camel_core::prelude::*;
use camel_zipfile::{
    unzip_entries, zip_splitter, ZipFileDataFormat, DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
};
use std::sync::Arc;

const TEXT: &str = "The Masque of Queen Bersabe (excerpt) \nby: Algernon Charles Swinburne \n\nMy lips kissed dumb the word of Ah \nSighed on strange lips grown sick thereby. \nGod wrought to me my royal bed; \nThe inner work thereof was red, \nThe outer work was ivory. \nMy mouth's heat was the heat of flame \nFor lust towards the kings that came \nWith horsemen riding royally.";

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b.clone(),
        other => other.to_string().into_bytes(),
    }
}

/// The entries (name, text) of the archive.
fn entries(data: &[u8]) -> Vec<(String, String)> {
    unzip_entries(data, DEFAULT_MAXIMUM_DECOMPRESSED_SIZE)
        .unwrap()
        .into_iter()
        .map(|(n, c, _)| (n, String::from_utf8(c).unwrap()))
        .collect()
}

fn zipped(name: &str) -> Vec<u8> {
    camel_zipfile::zip_entries([(name, TEXT.as_bytes())]).unwrap()
}

/// A new empty directory of a test.
fn test_dir(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "camel-zipfile-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().replace('\\', "/")
}

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_zipfile::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

fn df(d: ZipFileDataFormat) -> SharedDataFormat {
    Arc::new(d)
}

#[tokio::test]
async fn test_zip_file_data_format() {
    let out = test_dir("data-format");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:zip")
                .marshal(df(ZipFileDataFormat::new()))
                .to("mock:zip"),
        );
        r.add(
            from("direct:zipPreserve")
                .marshal(df(
                    ZipFileDataFormat::new().with_preserve_path_elements(true)
                ))
                .to("mock:zipPreserve"),
        );
        r.add(
            from("direct:unzip")
                .unmarshal(df(ZipFileDataFormat::new()))
                .to("mock:unzip"),
        );
        r.add(
            from("direct:zipAndUnzip")
                .marshal("zipFile")
                .unmarshal("zipFile")
                .to("mock:zipAndUnzip"),
        );
        r.add(
            from("direct:zipToFile")
                .marshal("zipFile")
                .to(&format!("file:{o}"))
                .to("mock:zipToFile"),
        );
        r.add(
            from("direct:corruptUnzip")
                .unmarshal("zipFile")
                .to("mock:corruptUnzip"),
        );
        r.add(
            from("direct:unzipMaxDecompressedSize")
                .unmarshal(df(ZipFileDataFormat::new().with_max_decompressed_size(10)))
                .to("mock:unzip"),
        );
        r.add(
            from("direct:unzipMaxDecompressedSizeIterator")
                .unmarshal(df(ZipFileDataFormat::new()
                    .with_using_iterator(true)
                    .with_max_decompressed_size(10)))
                .split(body(), |s| s.to("mock:unzipMaxDecompressedSizeIterator"))
                .streaming(),
        );
        r.add(
            from("direct:zipSplitterMaxDecompressedSize")
                .split(ExpressionDefinition::Expression(zip_splitter(10)), |s| {
                    s.to("mock:split")
                })
                .streaming(),
        );
    })
    .await;
    let t = c.create_producer_template();
    let mock = |uri: &str| MockEndpoint::resolve(c.as_ref(), uri).unwrap();

    // without file name: the message id
    t.send_body("direct:zip", TEXT).await.unwrap();
    let received = mock("mock:zip")
        .received_exchanges()
        .last()
        .unwrap()
        .clone();
    let id = received
        .message()
        .message_id()
        .unwrap_or(received.exchange_id())
        .to_string();
    assert_eq!(
        Some(format!("{id}.zip")),
        received.message().header(FILE_NAME).map(|v| v.to_string())
    );
    assert_eq!(
        vec![(id.clone(), TEXT.to_string())],
        entries(&bytes(received.message().body()))
    );
    // with a file name, with path elements stripped or preserved
    for (uri, name, expected) in [
        ("direct:zip", "poem.txt", vec![("poem.txt", TEXT)]),
        ("direct:zip", "poems/poem.txt", vec![("poem.txt", TEXT)]),
        (
            "direct:zipPreserve",
            "poems/poem.txt",
            vec![("poems/", ""), ("poems/poem.txt", TEXT)],
        ),
    ] {
        let out = t
            .request_body_and_header(uri, TEXT, FILE_NAME, name)
            .await
            .unwrap();
        let expected: Vec<(String, String)> = expected
            .into_iter()
            .map(|(n, c)| (n.to_string(), c.to_string()))
            .collect();
        assert_eq!(expected, entries(&bytes(&out)), "{name}");
    }
    t.send_body_and_header("direct:zip", TEXT, FILE_NAME, "poem.txt")
        .await
        .unwrap();
    let received = mock("mock:zip")
        .received_exchanges()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        Some("poem.txt.zip".to_string()),
        received.message().header(FILE_NAME).map(|v| v.to_string())
    );

    // unzip
    let unzip = mock("mock:unzip");
    unzip.expected_bodies_received([Value::Bytes(TEXT.as_bytes().to_vec())]);
    unzip.expected_header_received(FILE_NAME, "file");
    t.send_body("direct:unzip", Value::Bytes(zipped("file")))
        .await
        .unwrap();
    unzip.assert_is_satisfied().await;

    // zip and unzip: the message id as name
    t.send_body("direct:zipAndUnzip", TEXT).await.unwrap();
    let received = mock("mock:zipAndUnzip").received_exchanges()[0].clone();
    let id = received
        .message()
        .message_id()
        .unwrap_or(received.exchange_id())
        .to_string();
    assert_eq!(
        Some(id),
        received.message().header(FILE_NAME).map(|v| v.to_string())
    );
    assert_eq!(TEXT.as_bytes(), &bytes(received.message().body())[..]);

    // to a file
    t.send_body_and_header("direct:zipToFile", TEXT, FILE_NAME, "poem.txt")
        .await
        .unwrap();
    let file = std::fs::read(format!("{out}/poem.txt.zip")).unwrap();
    assert_eq!(
        vec![("poem.txt".to_string(), TEXT.to_string())],
        entries(&file)
    );

    // a corrupted file, and too big entries
    let corrupt = std::fs::read("tests/resources/corrupt.zip").unwrap();
    assert!(t
        .send_body("direct:corruptUnzip", Value::Bytes(corrupt))
        .await
        .is_err());
    for uri in [
        "direct:unzipMaxDecompressedSize",
        "direct:unzipMaxDecompressedSizeIterator",
        "direct:zipSplitterMaxDecompressedSize",
    ] {
        assert!(
            t.send_body(uri, Value::Bytes(zipped("file")))
                .await
                .is_err(),
            "{uri}"
        );
    }
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);
}

/// `ZipFileDataFormatTest` (empty directories): the iterator gives the directories when
/// allowed.
#[tokio::test]
async fn test_unzip_with_empty_directory() {
    let odt = std::fs::read("tests/resources/hello.odt").unwrap();
    for allow in [true, false] {
        let c = context(move |r: &mut Routes| {
            r.add(
                from("direct:unzipWithEmptyDirectory")
                    .unmarshal(df(ZipFileDataFormat::new()
                        .with_using_iterator(true)
                        .with_allow_empty_directory(allow)))
                    .split(body(), |s| s.to("mock:entry"))
                    .streaming(),
            );
        })
        .await;
        c.create_producer_template()
            .send_body("direct:unzipWithEmptyDirectory", Value::Bytes(odt.clone()))
            .await
            .unwrap();
        let names: Vec<String> = MockEndpoint::resolve(c.as_ref(), "mock:entry")
            .unwrap()
            .received_exchanges()
            .iter()
            .filter_map(|e| e.message().header("zipFileName").map(|v| v.to_string()))
            .collect();
        assert_eq!(
            allow,
            names
                .iter()
                .any(|n| n.starts_with("Configurations2") && n.ends_with('/')),
            "{names:?}"
        );
    }
}

/// `ZipFileIteratorDataFormatTest`: a zipped file, unzipped.
#[tokio::test]
async fn test_zip_file_iterator() {
    let out = test_dir("iterator");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:zip")
                .set_header(FILE_NAME, constant("report.txt"))
                .marshal("zipFile")
                .to(&format!("file:{o}"))
                .to("mock:result"),
        );
        r.add(from("direct:unzip").unmarshal("zipFile").to("mock:unzip"));
    })
    .await;
    let t = c.create_producer_template();
    t.send_body("direct:zip", "ABCDEF1234567890").await.unwrap();
    let file = std::fs::read(format!("{out}/report.txt.zip")).unwrap();
    let unzip = MockEndpoint::resolve(c.as_ref(), "mock:unzip").unwrap();
    unzip.expected_bodies_received([Value::Bytes(b"ABCDEF1234567890".to_vec())]);
    t.send_body("direct:unzip", Value::Bytes(file))
        .await
        .unwrap();
    unzip.assert_is_satisfied().await;
    let _ = std::fs::remove_dir_all(&out);
}

/// `ZipFileSplitIteratorEmptyTest` and `ZipFileSplitIteratorCorruptTest`: an empty archive
/// and a corrupted one, consumed from files.
#[tokio::test]
async fn test_split_iterator_empty_and_corrupt() {
    for file in ["empty.zip", "corrupt.zip"] {
        let c = context(move |r: &mut Routes| {
            r.error_handler(dead_letter_channel("mock:dead"));
            r.add(
                from(&format!(
                    "file://tests/resources?delay=10&initialDelay=0&fileName={file}&noop=true"
                ))
                .unmarshal(df(ZipFileDataFormat::new().with_using_iterator(true)))
                .split(body(), |s| s.convert_body_to("String").to("mock:end"))
                .streaming(),
            );
        })
        .await;
        let end = MockEndpoint::resolve(c.as_ref(), "mock:end").unwrap();
        end.expected_message_count(0);
        let dead = MockEndpoint::resolve(c.as_ref(), "mock:dead").unwrap();
        if file == "corrupt.zip" {
            dead.expected_message_count(1);
            dead.message_matches(0, exchange_property(EXCEPTION_CAUGHT).is_not_null());
        } else {
            dead.expected_message_count(0);
            dead.set_sleep_for_empty_test(500);
        }
        dead.assert_is_satisfied().await;
        end.set_sleep_for_empty_test(500);
        end.assert_is_satisfied().await;
        c.stop().await.unwrap();
    }
}

/// `ZipSplitterRouteTest.testSplitter`: the entries of a consumed archive.
#[tokio::test]
async fn test_zip_splitter_route() {
    let dir = test_dir("splitter");
    std::fs::create_dir_all(format!("{dir}/in")).unwrap();
    std::fs::copy(
        "tests/resources/org/apache/camel/dataformat/zipfile/data/resources.zip",
        format!("{dir}/in/resources.zip"),
    )
    .unwrap();
    let d = dir.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from(&format!("file:{d}/in?initialDelay=0&delay=10"))
                .split(
                    ExpressionDefinition::Expression(zip_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.convert_body_to("String").to("mock:processZipEntry"),
                )
                .streaming(),
        );
    })
    .await;
    let mock = MockEndpoint::resolve(c.as_ref(), "mock:processZipEntry").unwrap();
    mock.expected_bodies_received_in_any_order([
        "chau",
        "hi",
        "hola",
        "another_chiau",
        "another_hi",
    ]);
    mock.assert_is_satisfied().await;
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// `ZipSplitterRouteIssueTest` (without `shareUnitOfWork`): an archive, and data that is not
/// one.
#[tokio::test]
async fn test_zip_splitter_route_issue() {
    let c = context(|r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:errors"));
        r.add(
            from("direct:decompressFiles")
                .split(
                    ExpressionDefinition::Expression(zip_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.to("log:entry").to("mock:entry"),
                )
                .streaming(),
        );
    })
    .await;
    let t = c.create_producer_template();
    let entry = MockEndpoint::resolve(c.as_ref(), "mock:entry").unwrap();
    let errors = MockEndpoint::resolve(c.as_ref(), "mock:errors").unwrap();
    entry.expected_message_count(2);
    t.send_body(
        "direct:decompressFiles",
        Value::Bytes(std::fs::read("tests/resources/data.zip").unwrap()),
    )
    .await
    .unwrap();
    entry.assert_is_satisfied().await;
    entry.reset();
    entry.expected_message_count(0);
    errors.expected_message_count(1);
    t.send_body("direct:decompressFiles", "not a zip file")
        .await
        .unwrap();
    errors.assert_is_satisfied().await;
    entry.assert_is_satisfied().await;
}

/// `ZipFileMultipleFilesSplitterTest`: the names of the entries collected by the aggregation
/// strategy of the split.
#[tokio::test]
async fn test_zip_file_multiple_files_splitter() {
    struct UpdateHeader;
    impl camel_core::api::processor::AggregationStrategy for UpdateHeader {
        fn aggregate(
            &self,
            old: Option<Exchange>,
            new: Option<Exchange>,
        ) -> CamelResult<Option<Exchange>> {
            let Some(mut new) = new else { return Ok(old) };
            if let Some(old) = old {
                let processed = old
                    .message()
                    .header("processedFiles")
                    .or_else(|| old.message().header("zipFileName"))
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let name = new
                    .message()
                    .header("zipFileName")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                new.message_mut()
                    .set_header("processedFiles", format!("{processed},{name}"));
            }
            Ok(Some(new))
        }
    }
    let c = context(|r: &mut Routes| {
        r.add(
            from("file:tests/resources/org/apache/camel/dataformat/zipfile/data/?initialDelay=0&delay=1000&noop=true")
                .unmarshal(df(ZipFileDataFormat::new().with_using_iterator(true)))
                .split(body(), |s| s.convert_body_to("String").to("mock:processZipEntry"))
                .streaming()
                .aggregation_strategy(Arc::new(UpdateHeader))
                .log("Done processing big file: ${header.CamelFileName}")
                .set_body(header("processedFiles"))
                .split(body(), |s| s.to("mock:splitResult")),
        );
    })
    .await;
    let entries = MockEndpoint::resolve(c.as_ref(), "mock:processZipEntry").unwrap();
    entries.expected_bodies_received_in_any_order([
        "chau",
        "hi",
        "hola",
        "another_chiau",
        "another_hi",
    ]);
    let result = MockEndpoint::resolve(c.as_ref(), "mock:splitResult").unwrap();
    result.expected_bodies_received_in_any_order([
        "chiau.txt",
        "hi.txt",
        "hola.txt",
        "directoryOne/another_chiau.txt",
        "directoryOne/another_hi.txt",
    ]);
    entries.assert_is_satisfied().await;
    result.assert_is_satisfied().await;
    c.stop().await.unwrap();
}

/// `ZipFileNameStripPathTest`: the file name of an entry is without its path.
#[tokio::test]
async fn test_zip_file_name_strip_path() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").unmarshal("zipFile").to("mock:result"));
        r.add(
            from("direct:iterate")
                .split(
                    ExpressionDefinition::Expression(zip_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.to("mock:iterated"),
                )
                .streaming(),
        );
    })
    .await;
    let zip = camel_zipfile::zip_entries([("subdir/evil.txt", &b"payload"[..])]).unwrap();
    let t = c.create_producer_template();
    t.send_body("direct:start", Value::Bytes(zip.clone()))
        .await
        .unwrap();
    t.send_body("direct:iterate", Value::Bytes(zip))
        .await
        .unwrap();
    let header = |uri: &str, name: &str| {
        MockEndpoint::resolve(c.as_ref(), uri)
            .unwrap()
            .received_exchanges()[0]
            .message()
            .header(name)
            .map(|v| v.to_string())
    };
    assert_eq!(
        Some("evil.txt".to_string()),
        header("mock:result", FILE_NAME)
    );
    assert_eq!(
        Some("evil.txt".to_string()),
        header("mock:iterated", FILE_NAME)
    );
    assert_eq!(
        Some("subdir/evil.txt".to_string()),
        header("mock:iterated", "zipFileName")
    );
}

/// `ZipSplitterUseOriginalMessageTest`: the entries with an onException using the original
/// message.
#[tokio::test]
async fn test_zip_splitter_use_original_message() {
    let c = context(|r: &mut Routes| {
        r.on_exception(on_exception(ErrorType::Any).use_original_message());
        r.add(
            from("file:tests/resources/org/apache/camel/dataformat/zipfile/data?initialDelay=0&delay=1000&noop=true")
                .unmarshal(df(ZipFileDataFormat::new().with_using_iterator(true)))
                .split(body(), |s| s.log("${body}").convert_body_to("String").to("mock:processZipEntry"))
                .streaming(),
        );
    })
    .await;
    let entries = MockEndpoint::resolve(c.as_ref(), "mock:processZipEntry").unwrap();
    entries.expected_bodies_received_in_any_order([
        "chau",
        "hi",
        "hola",
        "another_chiau",
        "another_hi",
    ]);
    entries.assert_is_satisfied().await;
    c.stop().await.unwrap();
}
