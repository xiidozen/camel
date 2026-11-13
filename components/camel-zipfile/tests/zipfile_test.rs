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

/// The names of the entries of the only zip file of the directory.
fn zip_in_dir(dir: &str) -> Vec<String> {
    let files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!files.is_empty(), "Should be a file in {dir} directory");
    camel_zipfile::entry_names(&std::fs::read(files[0].path()).unwrap()).unwrap()
}

/// `ZipAggregationStrategyTest` and `AggregationStrategyWithPreservationTest`: the files of a
/// batch, aggregated in one zip file.
#[tokio::test]
async fn test_zip_aggregation_strategy_batch() {
    for preserve in [false, true] {
        let out = test_dir("aggregate");
        let o = out.clone();
        let c = context(move |r: &mut Routes| {
            let recursive = if preserve { "&recursive=true" } else { "" };
            r.add(
                from(&format!("file:tests/resources/org/apache/camel/aggregate/zipfile/data?initialDelay=0&delay=1000&noop=true{recursive}"))
                    .set_header("foo", constant("bar"))
                    .aggregate(constant(true), Arc::new(camel_zipfile::ZipAggregationStrategy::new(preserve, preserve)), |a| {
                        a.to(&format!("file:{o}")).to("mock:aggregateToZipEntry")
                    })
                    .completion_from_batch_consumer()
                    .eager_check_completion(),
            );
        })
        .await;
        let mock = MockEndpoint::resolve(c.as_ref(), "mock:aggregateToZipEntry").unwrap();
        mock.expected_message_count(1);
        mock.expected_header_received("foo", "bar");
        mock.assert_is_satisfied().await;
        let mut names = zip_in_dir(&out);
        names.sort();
        if preserve {
            assert_eq!(
                vec![
                    "another/hello.txt",
                    "chiau.txt",
                    "hi.txt",
                    "hola.txt",
                    "other/greetings.txt"
                ],
                names
            );
        } else {
            assert_eq!(3, names.len(), "{names:?}");
        }
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// `AggregationStrategyWithFilenameHeaderTest`: the entries named after the file name header.
#[tokio::test]
async fn test_zip_aggregation_strategy_with_filename_header() {
    let out = test_dir("aggregate-filename");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(
                    constant(true),
                    Arc::new(camel_zipfile::ZipAggregationStrategy::new(false, true)),
                    |a| a.to(&format!("file:{o}")).to("mock:aggregateToZipEntry"),
                )
                .completion_timeout(50),
        );
    })
    .await;
    let t = c.create_producer_template();
    t.send_body_and_header("direct:start", "foo", FILE_NAME, "foo")
        .await
        .unwrap();
    t.send_body_and_header("direct:start", "bar", FILE_NAME, "bar")
        .await
        .unwrap();
    let mock = MockEndpoint::resolve(c.as_ref(), "mock:aggregateToZipEntry").unwrap();
    mock.expected_message_count(1);
    mock.assert_is_satisfied().await;
    assert_eq!(vec!["foo", "bar"], zip_in_dir(&out));
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);
}

/// `ZipAggregationStrategyNullBodyTest`: the null bodies are skipped.
#[tokio::test]
async fn test_zip_aggregation_strategy_null_body() {
    for (bodies, expected) in [
        (["Hello", "Hello again", ""], 2),
        (["", "Hello", "Hello again"], 2),
        (["Hello", "", "Hello again"], 2),
        (["", "", ""], 0),
        (["", "", "Hello"], 1),
    ] {
        let out = test_dir("aggregate-null");
        let o = out.clone();
        let c = context(move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .aggregate(
                        constant(true),
                        Arc::new(camel_zipfile::ZipAggregationStrategy::default()),
                        |a| a.to(&format!("file:{o}")).to("mock:aggregateToZipEntry"),
                    )
                    .completion_size(3)
                    .eager_check_completion(),
            );
        })
        .await;
        let t = c.create_producer_template();
        for b in bodies {
            let body = if b.is_empty() {
                Value::Null
            } else {
                Value::from(b)
            };
            t.send_body("direct:start", body).await.unwrap();
        }
        let mock = MockEndpoint::resolve(c.as_ref(), "mock:aggregateToZipEntry").unwrap();
        mock.expected_message_count(1);
        mock.assert_is_satisfied().await;
        assert_eq!(expected, zip_in_dir(&out).len(), "{bodies:?}");
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// `ZipAggregationStrategyEmptyFileTest`: the empty files are skipped unless allowed.
#[tokio::test]
async fn test_zip_aggregation_strategy_empty_file() {
    for allow in [false, true] {
        let (input, out) = (
            test_dir("aggregate-empty-in"),
            test_dir("aggregate-empty-out"),
        );
        let (i, o) = (input.clone(), out.clone());
        let c = context(move |r: &mut Routes| {
            r.add(
                from(&format!("file:{i}?initialDelay=0&delay=10"))
                    .aggregate(
                        constant(true),
                        Arc::new(
                            camel_zipfile::ZipAggregationStrategy::default()
                                .with_allow_empty_files(allow),
                        ),
                        |a| a.to(&format!("file:{o}")).to("mock:aggregateToZipEntry"),
                    )
                    .completion_size(4)
                    .eager_check_completion(),
            );
        })
        .await;
        let t = c.create_producer_template();
        for b in ["Hello", "", "Bye", "Howdy"] {
            t.send_body(&format!("file:{input}"), b).await.unwrap();
        }
        let mock = MockEndpoint::resolve(c.as_ref(), "mock:aggregateToZipEntry").unwrap();
        mock.expected_message_count(1);
        mock.assert_is_satisfied().await;
        assert_eq!(if allow { 4 } else { 3 }, zip_in_dir(&out).len());
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&input);
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// `ZipAggregationStrategySplitEmptyBodyTest`: splitting an empty body with the strategy.
#[tokio::test]
async fn test_zip_aggregation_strategy_split_empty_body() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_body(constant(""))
                .split(body(), |s| s.to("mock:split"))
                .aggregation_strategy(Arc::new(camel_zipfile::ZipAggregationStrategy::default()))
                .to("mock:result"),
        );
    })
    .await;
    let split = MockEndpoint::resolve(c.as_ref(), "mock:split").unwrap();
    split.expected_message_count(0);
    let result = MockEndpoint::resolve(c.as_ref(), "mock:result").unwrap();
    result.expected_message_count(1);
    c.create_producer_template()
        .send_body("direct:start", Value::Null)
        .await
        .unwrap();
    result.assert_is_satisfied().await;
    split.assert_is_satisfied().await;
}

/// `ZipFileSplitOneFileTest`: an archive of one file, consumed and split.
#[tokio::test]
async fn test_zip_file_split_one_file() {
    let dir = test_dir("split-one");
    std::fs::write(
        format!("{dir}/test.zip"),
        camel_zipfile::zip_entries([("test.txt", &b"Hello World"[..])]).unwrap(),
    )
    .unwrap();
    let d = dir.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from(&format!(
                "file://{d}?noop=true&include=.*zip&initialDelay=0&delay=10"
            ))
            .to("mock:input")
            .unmarshal(df(ZipFileDataFormat::new().with_using_iterator(true)))
            .split(body(), |s| s.convert_body_to("String").to("mock:end"))
            .streaming(),
        );
    })
    .await;
    let input = MockEndpoint::resolve(c.as_ref(), "mock:input").unwrap();
    input.expected_header_received(
        camel_core::api::exchange_constants::FILE_NAME_ONLY,
        "test.zip",
    );
    let end = MockEndpoint::resolve(c.as_ref(), "mock:end").unwrap();
    end.expected_bodies_received(["Hello World"]);
    input.assert_is_satisfied().await;
    end.assert_is_satisfied().await;
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// `ZipFileSplitAndDeleteTest`: the consumed archive is deleted after the split, with the
/// data format and with the splitter.
#[tokio::test]
async fn test_zip_file_split_and_delete() {
    for with_data_format in [true, false] {
        let dir = test_dir("split-delete");
        let d = dir.clone();
        let c = context(move |r: &mut Routes| {
            let route = from(&format!("file://{d}?initialDelay=0&delay=10&delete=true"));
            let route = if with_data_format {
                route
                    .unmarshal(df(ZipFileDataFormat::new().with_using_iterator(true)))
                    .split(body(), |s| s.convert_body_to("String").to("mock:end"))
            } else {
                route.split(
                    ExpressionDefinition::Expression(zip_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.convert_body_to("String").to("mock:end"),
                )
            };
            r.add(route.streaming());
        })
        .await;
        let end = MockEndpoint::resolve(c.as_ref(), "mock:end").unwrap();
        end.expected_message_count(2);
        std::fs::copy("tests/resources/data.zip", format!("{dir}/data.zip")).unwrap();
        end.assert_is_satisfied().await;
        let file = std::path::PathBuf::from(format!("{dir}/data.zip"));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while file.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "File should been deleted"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `SpringZipSplitterRouteTest`: the Spring XML route in the XML DSL, the splitter a bean
/// referred by the ref language.
#[tokio::test]
async fn test_spring_zip_splitter_route() {
    let xml = r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route>
    <from uri="file:tests/resources/org/apache/camel/dataformat/zipfile/data?initialDelay=0&amp;delay=1000&amp;noop=true"/>
    <split streaming="true">
      <ref>zipSplitter</ref>
      <convertBodyTo type="java.lang.String" charset="UTF-8"/>
      <to uri="mock:processZipEntry"/>
    </split>
  </route>
</routes>"#;
    let context = camel_core::default_context();
    camel_zipfile::register(&context);
    context
        .registry()
        .unwrap()
        .bind(
            "zipSplitter",
            camel_core::api::processor::expression_value(zip_splitter(
                DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
            )),
        )
        .unwrap();
    context
        .add_routes(camel_xml_io_dsl::XmlRoutes::parse(xml).unwrap())
        .await
        .unwrap();
    context.start().await.unwrap();
    let mock = MockEndpoint::resolve(context.as_ref(), "mock:processZipEntry").unwrap();
    mock.expected_bodies_received_in_any_order([
        "chau",
        "hi",
        "hola",
        "another_chiau",
        "another_hi",
    ]);
    mock.assert_is_satisfied().await;
    context.stop().await.unwrap();
}

/// `ZipAggregationStrategyRepositoryTest` (the repository storing copies; the optimistic
/// locking of the memory repository is not ported).
#[tokio::test]
async fn test_zip_aggregation_strategy_repository() {
    use camel_core::api::spi::aggregation_repository::AggregationRepository;
    use std::collections::HashMap;
    use std::sync::Mutex;
    #[derive(Default)]
    struct CopyingRepository(Mutex<HashMap<String, Exchange>>);
    impl AggregationRepository for CopyingRepository {
        fn add(&self, key: &str, exchange: Exchange) -> Option<Exchange> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), exchange.clone())
        }
        fn get(&self, key: &str) -> Option<Exchange> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn remove(&self, key: &str, _: &Exchange) {
            self.0.lock().unwrap().remove(key);
        }
        fn confirm(&self, _: &str) {}
        fn keys(&self) -> Vec<String> {
            self.0.lock().unwrap().keys().cloned().collect()
        }
    }
    let out = test_dir("aggregate-repository");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:copies")
                .aggregate(
                    constant(true),
                    Arc::new(camel_zipfile::ZipAggregationStrategy::default()),
                    |a| a.to(&format!("file:{o}")).to("mock:copies"),
                )
                .aggregation_repository(Arc::new(CopyingRepository::default()))
                .completion_size(3),
        );
    })
    .await;
    let t = c.create_producer_template();
    for b in ["Hello", "Hello again", "Bye"] {
        t.send_body("direct:copies", b).await.unwrap();
    }
    let mock = MockEndpoint::resolve(c.as_ref(), "mock:copies").unwrap();
    mock.expected_message_count(1);
    mock.assert_is_satisfied().await;
    assert_eq!(3, zip_in_dir(&out).len());
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);
}

/// `ZipSplitAggregateTransactedIssueTest` and its streaming variant (without the Spring
/// transaction): all the entries of a base64 archive are split and aggregated.
#[tokio::test]
async fn test_zip_split_aggregate() {
    const ZIP: &str = "UEsDBBQAAAAIAFlrtFDFAfecUAAAAB4BAAALAAAAT3JkZXJzMS54bWyzyS9KSS0qtuPl4oQwQSxOm8wUOxMb/cwUCK+gKD+lNLkEzOG0yUvMTbWDCik42uiD+WB1+kgKbfThxqEZbEqUwU6kG2xGlMHOhA2GsortAFBLAwQUAAAACABBW9hQgBf0tVgAAAAqAQAACwAAAE9yZGVyczIueG1ss8kvSkktKrbj5eKEMEEsTpvMFDtDQ0Mb/cwUCL+gKD+lNLkEzOG0yUvMTbWDCimA1YFFwCr1kZTa6MONRDPcyMiIKMPB6kg13NjYmCjDweoIGQ5lFdsBAFBLAQIfABQAAAAIAFlrtFDFAfecUAAAAB4BAAALACQAAAAAAAAAIAAAAAAAAABPcmRlcnMxLnhtbAoAIAAAAAAAAQAYAAD57I2ZLtYBg97kuHn02gEA+eyNmS7WAVBLAQIfABQAAAAIAEFb2FCAF/S1WAAAACoBAAALACQAAAAAAAAAIAAAAHkAAABPcmRlcnMyLnhtbAoAIAAAAAAAAQAYAAAxPXoJStYBjn3iuHn02gEAMT16CUrWAVBLBQYAAAAAAgACALoAAAD6AAAAAAA=";
    struct Last;
    impl camel_core::api::processor::AggregationStrategy for Last {
        fn aggregate(
            &self,
            old: Option<Exchange>,
            new: Option<Exchange>,
        ) -> CamelResult<Option<Exchange>> {
            Ok(new.or(old))
        }
    }
    for streaming in [false, true] {
        let c = camel_core::default_context();
        camel_zipfile::register(&c);
        camel_base64::register(&c);
        c.registry()
            .unwrap()
            .bind(
                "zipSplitter",
                camel_core::api::processor::expression_value(zip_splitter(
                    DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                )),
            )
            .unwrap();
        c.add_routes(move |r: &mut Routes| {
            let route = from("direct:start")
                .set_body(constant(ZIP))
                .unmarshal("base64")
                .split(ExpressionDefinition::language("ref", "zipSplitter"), |s| {
                    s.log("Splitting ${header.CamelFileName}")
                })
                .aggregation_strategy(Arc::new(Last));
            let route = if streaming { route.streaming() } else { route };
            r.add(route.to("mock:result"));
        })
        .await
        .unwrap();
        c.start().await.unwrap();
        c.create_producer_template()
            .send_body("direct:start", "")
            .await
            .unwrap();
        let mock = MockEndpoint::resolve(c.as_ref(), "mock:result").unwrap();
        mock.expected_message_count(1);
        mock.assert_is_satisfied().await;
        assert_eq!(
            Some("Orders2.xml".to_string()),
            mock.received_exchanges()[0]
                .message()
                .header(FILE_NAME)
                .map(|v| v.to_string())
        );
    }
}

/// `ZipAggregationStrategySplitTest`: the messages of a batch grouped (Java
/// `GroupedMessageAggregationStrategy`), then split and zipped by the strategy of the split.
#[tokio::test]
async fn test_zip_aggregation_strategy_split() {
    struct Grouped;
    impl camel_core::api::processor::AggregationStrategy for Grouped {
        fn aggregate(
            &self,
            old: Option<Exchange>,
            new: Option<Exchange>,
        ) -> CamelResult<Option<Exchange>> {
            let Some(new) = new else { return Ok(old) };
            let message = Value::Object(camel_util::value::ObjectRef::new(new.message().clone()));
            Ok(Some(match old {
                None => {
                    let mut e = new;
                    e.message_mut().set_body(Value::List(vec![message]));
                    e
                }
                Some(mut o) => {
                    if let Value::List(l) = o.message().body().clone() {
                        let mut l = l;
                        l.push(message);
                        o.message_mut().set_body(Value::List(l));
                    }
                    o
                }
            }))
        }
    }
    let out = test_dir("aggregate-split");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("file:tests/resources/org/apache/camel/aggregate/zipfile/data?initialDelay=0&delay=1000&noop=true")
                .aggregate(constant(true), Arc::new(Grouped), |a| {
                    a.split(body(), |s| s.process_fn(|_| Ok(())))
                        .aggregation_strategy(Arc::new(camel_zipfile::ZipAggregationStrategy::new(true, true)))
                        .streaming()
                        .to(&format!("file:{o}"))
                        .to("mock:aggregateToZipEntry")
                })
                .completion_from_batch_consumer()
                .eager_check_completion(),
        );
    })
    .await;
    let mock = MockEndpoint::resolve(c.as_ref(), "mock:aggregateToZipEntry").unwrap();
    mock.expected_message_count(1);
    mock.assert_is_satisfied().await;
    assert_eq!(3, zip_in_dir(&out).len());
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);
}
