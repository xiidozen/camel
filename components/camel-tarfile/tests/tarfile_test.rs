//! Ports of the camel-tarfile tests: `TarFileDataFormatTest` (archives compared by their
//! entries), `SpringTarFileDataFormatTest` and `SpringTarSplitterRouteTest` (XML DSL),
//! `TarSplitterRouteIssueTest`, `TarFileMultipleFilesSplitterTest`, `TarFileNameStripPathTest`,
//! `TarFileSplitAndDeleteTest` and the aggregation strategy tests (but the optimistic locking
//! repository).

use camel_core::api::exchange_constants::FILE_NAME;
use camel_core::api::spi::data_format::SharedDataFormat;
use camel_core::model::expression::ExpressionDefinition;
use camel_core::prelude::*;
use camel_tarfile::{
    tar_entries, tar_splitter, untar_entries, TarAggregationStrategy, TarFileDataFormat,
    DEFAULT_MAXIMUM_DECOMPRESSED_SIZE, TARFILE_ENTRY_NAME_HEADER,
};
use std::sync::Arc;

const TEXT: &str = "The Masque of Queen Bersabe (excerpt) \nby: Algernon Charles Swinburne \n\nMy lips kissed dumb the word of Ah \nSighed on strange lips grown sick thereby. \nGod wrought to me my royal bed; \nThe inner work thereof was red, \nThe outer work was ivory. \nMy mouth's heat was the heat of flame \nFor lust towards the kings that came \nWith horsemen riding royally.";

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b.clone(),
        other => other.to_string().into_bytes(),
    }
}

fn entries(data: &[u8]) -> Vec<(String, String)> {
    untar_entries(data, DEFAULT_MAXIMUM_DECOMPRESSED_SIZE)
        .unwrap()
        .into_iter()
        .map(|(n, c, _)| (n, String::from_utf8(c).unwrap()))
        .collect()
}

fn tared(name: &str) -> Vec<u8> {
    tar_entries([(name, TEXT.as_bytes())]).unwrap()
}

fn test_dir(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "camel-tarfile-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().replace('\\', "/")
}

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_tarfile::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

fn df(d: TarFileDataFormat) -> SharedDataFormat {
    Arc::new(d)
}

fn mock(c: &Arc<DefaultCamelContext>, uri: &str) -> MockEndpoint {
    MockEndpoint::resolve(c.as_ref(), uri).unwrap()
}

/// The data format tests, with the routes of the Java DSL or of the Spring XML.
async fn data_format_tests(c: Arc<DefaultCamelContext>, out: &str) {
    let t = c.create_producer_template();
    t.send_body("direct:tar", TEXT).await.unwrap();
    let received = mock(&c, "mock:tar")
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
        Some(format!("{id}.tar")),
        received.message().header(FILE_NAME).map(|v| v.to_string())
    );
    assert_eq!(
        vec![(id, TEXT.to_string())],
        entries(&bytes(received.message().body()))
    );
    for name in ["poem.txt", "poems/poem.txt"] {
        let out = t
            .request_body_and_header("direct:tar", TEXT, FILE_NAME, name)
            .await
            .unwrap();
        assert_eq!(
            vec![("poem.txt".to_string(), TEXT.to_string())],
            entries(&bytes(&out)),
            "{name}"
        );
    }
    let untar = mock(&c, "mock:untar");
    untar.expected_bodies_received([Value::Bytes(TEXT.as_bytes().to_vec())]);
    untar.expected_header_received(FILE_NAME, "file");
    t.send_body("direct:untar", Value::Bytes(tared("file")))
        .await
        .unwrap();
    untar.assert_is_satisfied().await;
    t.send_body("direct:tarAndUntar", TEXT).await.unwrap();
    let received = mock(&c, "mock:tarAndUntar").received_exchanges()[0].clone();
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
    t.send_body_and_header("direct:tarToFile", TEXT, FILE_NAME, "poem.txt")
        .await
        .unwrap();
    let file = std::fs::read(format!("{out}/poem.txt.tar")).unwrap();
    assert_eq!(
        vec![("poem.txt".to_string(), TEXT.to_string())],
        entries(&file)
    );
    let dsl_untar = mock(&c, "mock:dslUntar");
    dsl_untar.expected_bodies_received([Value::Bytes(TEXT.as_bytes().to_vec())]);
    dsl_untar.expected_header_received(FILE_NAME, "test.txt");
    t.send_body("direct:dslUntar", Value::Bytes(tared("test.txt")))
        .await
        .unwrap();
    dsl_untar.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_tar_file_data_format() {
    let out = test_dir("data-format");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:tar")
                .marshal(df(TarFileDataFormat::new()))
                .to("mock:tar"),
        );
        r.add(
            from("direct:tarPreserve")
                .marshal(df(
                    TarFileDataFormat::new().with_preserve_path_elements(true)
                ))
                .to("mock:tarPreserve"),
        );
        r.add(from("direct:untar").unmarshal("tarFile").to("mock:untar"));
        r.add(
            from("direct:tarAndUntar")
                .marshal("tarFile")
                .unmarshal("tarFile")
                .to("mock:tarAndUntar"),
        );
        r.add(
            from("direct:tarToFile")
                .marshal("tarFile")
                .to(&format!("file:{o}"))
                .to("mock:tarToFile"),
        );
        r.add(
            from("direct:dslUntar")
                .unmarshal("tarFile")
                .to("mock:dslUntar"),
        );
        r.add(
            from("direct:corruptUntar")
                .unmarshal("tarFile")
                .to("mock:corruptUntar"),
        );
        r.add(
            from("direct:untarMaxDecompressedSize")
                .unmarshal(df(TarFileDataFormat::new().with_max_decompressed_size(10)))
                .to("mock:untar"),
        );
        r.add(
            from("direct:untarMaxDecompressedSizeIterator")
                .unmarshal(df(TarFileDataFormat::new()
                    .with_using_iterator(true)
                    .with_max_decompressed_size(10)))
                .split(body(), |s| s.to("mock:it"))
                .streaming(),
        );
        r.add(
            from("direct:tarSplitterMaxDecompressedSize")
                .split(ExpressionDefinition::Expression(tar_splitter(10)), |s| {
                    s.to("mock:split")
                })
                .streaming(),
        );
    })
    .await;
    data_format_tests(c.clone(), &out).await;
    let t = c.create_producer_template();
    let p = t
        .request_body_and_header("direct:tarPreserve", TEXT, FILE_NAME, "poems/poem.txt")
        .await
        .unwrap();
    assert_eq!(
        vec![
            ("poems/".to_string(), String::new()),
            ("poems/poem.txt".to_string(), TEXT.to_string())
        ],
        entries(&bytes(&p))
    );
    let corrupt = std::fs::read("tests/resources/data/corrupt.tar").unwrap();
    assert!(t
        .send_body("direct:corruptUntar", Value::Bytes(corrupt))
        .await
        .is_err());
    for uri in [
        "direct:untarMaxDecompressedSize",
        "direct:untarMaxDecompressedSizeIterator",
        "direct:tarSplitterMaxDecompressedSize",
    ] {
        assert!(
            t.send_body(uri, Value::Bytes(tared("file"))).await.is_err(),
            "{uri}"
        );
    }
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);
}

/// `SpringTarFileDataFormatTest`: the Spring XML routes in the XML DSL.
#[tokio::test]
async fn test_spring_tar_file_data_format() {
    let out = test_dir("spring");
    let xml = format!(
        r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route><from uri="direct:tar"/><marshal><tarFile/></marshal><to uri="mock:tar"/></route>
  <route><from uri="direct:untar"/><unmarshal><tarFile/></unmarshal><to uri="mock:untar"/></route>
  <route><from uri="direct:tarAndUntar"/><marshal><tarFile/></marshal><unmarshal><tarFile/></unmarshal><to uri="mock:tarAndUntar"/></route>
  <route><from uri="direct:tarToFile"/><marshal><tarFile/></marshal><to uri="file:{out}"/><to uri="mock:tarToFile"/></route>
  <route><from uri="direct:dslUntar"/><unmarshal><tarFile/></unmarshal><to uri="mock:dslUntar"/></route>
</routes>"#
    );
    let c = camel_core::default_context();
    camel_tarfile::register(&c);
    c.add_routes(camel_xml_io_dsl::XmlRoutes::parse(&xml).unwrap())
        .await
        .unwrap();
    c.start().await.unwrap();
    data_format_tests(c.clone(), &out).await;
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);
}

/// `TarFileDataFormatTest` (empty directories): the iterator gives the directories when
/// allowed.
#[tokio::test]
async fn test_untar_with_empty_directory() {
    let data = std::fs::read("tests/resources/data/hello.tar").unwrap();
    let dirs: Vec<bool> = untar_entries(&data, u64::MAX)
        .unwrap()
        .iter()
        .map(|(_, _, d)| *d)
        .collect();
    for allow in [true, false] {
        let c = context(move |r: &mut Routes| {
            r.add(
                from("direct:untarWithEmptyDirectory")
                    .unmarshal(df(TarFileDataFormat::new()
                        .with_using_iterator(true)
                        .with_allow_empty_directory(allow)))
                    .split(body(), |s| s.to("mock:entry")),
            );
        })
        .await;
        c.create_producer_template()
            .send_body("direct:untarWithEmptyDirectory", Value::Bytes(data.clone()))
            .await
            .unwrap();
        let n = mock(&c, "mock:entry").received_counter();
        let expected = if allow {
            dirs.len()
        } else {
            dirs.iter().filter(|d| !**d).count()
        };
        assert_eq!(expected, n, "allow={allow}");
    }
}

/// `TarSplitterRouteIssueTest`: an archive of three entries, and data that is not one.
#[tokio::test]
async fn test_tar_splitter_route_issue() {
    let c = context(|r: &mut Routes| {
        r.error_handler(dead_letter_channel("mock:errors"));
        r.add(
            from("direct:decompressFiles")
                .split(
                    ExpressionDefinition::Expression(tar_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.to("log:entry").to("mock:entry"),
                )
                .streaming(),
        );
    })
    .await;
    let t = c.create_producer_template();
    let entry = mock(&c, "mock:entry");
    entry.expected_message_count(3);
    t.send_body(
        "direct:decompressFiles",
        Value::Bytes(std::fs::read("tests/resources/data/tarfile3.tar").unwrap()),
    )
    .await
    .unwrap();
    entry.assert_is_satisfied().await;
    entry.reset();
    entry.expected_message_count(0);
    let errors = mock(&c, "mock:errors");
    errors.expected_message_count(1);
    t.send_body("direct:decompressFiles", Value::Bytes(vec![1u8; 700]))
        .await
        .unwrap();
    errors.assert_is_satisfied().await;
    entry.assert_is_satisfied().await;
}

/// `TarFileMultipleFilesSplitterTest` and `SpringTarSplitterRouteTest`.
#[tokio::test]
async fn test_tar_file_multiple_files_splitter() {
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
                    .or_else(|| old.message().header(TARFILE_ENTRY_NAME_HEADER))
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let name = new
                    .message()
                    .header(TARFILE_ENTRY_NAME_HEADER)
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
            from("file:tests/resources/org/apache/camel/dataformat/tarfile/data/?initialDelay=0&delay=1000&noop=true")
                .unmarshal(df(TarFileDataFormat::new().with_using_iterator(true)))
                .split(body(), |s| s.convert_body_to("String").to("mock:processTarEntry"))
                .streaming()
                .aggregation_strategy(Arc::new(UpdateHeader))
                .set_body(header("processedFiles"))
                .split(body(), |s| s.to("mock:splitResult")),
        );
    })
    .await;
    let entries = mock(&c, "mock:processTarEntry");
    entries.expected_bodies_received_in_any_order(["chau", "hi", "hola", "hello", "greetings"]);
    let result = mock(&c, "mock:splitResult");
    result.expected_bodies_received_in_any_order([
        "chiau.txt",
        "hi.txt",
        "hola.txt",
        "another/hello.txt",
        "other/greetings.txt",
    ]);
    entries.assert_is_satisfied().await;
    result.assert_is_satisfied().await;
    c.stop().await.unwrap();

    // the Spring XML route, the splitter by ref
    let xml = r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route>
    <from uri="file:tests/resources/org/apache/camel/dataformat/tarfile/data?initialDelay=0&amp;delay=1000&amp;noop=true"/>
    <split streaming="true">
      <ref>tarSplitter</ref>
      <convertBodyTo type="java.lang.String" charset="UTF-8"/>
      <to uri="log:line"/>
      <to uri="mock:processTarEntry"/>
    </split>
  </route>
</routes>"#;
    let c = camel_core::default_context();
    c.registry()
        .unwrap()
        .bind(
            "tarSplitter",
            camel_core::api::processor::expression_value(tar_splitter(
                DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
            )),
        )
        .unwrap();
    c.add_routes(camel_xml_io_dsl::XmlRoutes::parse(xml).unwrap())
        .await
        .unwrap();
    c.start().await.unwrap();
    let entries = mock(&c, "mock:processTarEntry");
    entries.expected_bodies_received_in_any_order(["chau", "hi", "hola", "hello", "greetings"]);
    entries.assert_is_satisfied().await;
    c.stop().await.unwrap();
}

/// `TarFileNameStripPathTest`.
#[tokio::test]
async fn test_tar_file_name_strip_path() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:start").unmarshal("tarFile").to("mock:result"));
        r.add(
            from("direct:iterate")
                .split(
                    ExpressionDefinition::Expression(tar_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.to("mock:iterated"),
                )
                .streaming(),
        );
    })
    .await;
    let tar = tar_entries([("subdir/evil.txt", &b"payload"[..])]).unwrap();
    let t = c.create_producer_template();
    t.send_body("direct:start", Value::Bytes(tar.clone()))
        .await
        .unwrap();
    t.send_body("direct:iterate", Value::Bytes(tar))
        .await
        .unwrap();
    let header = |uri: &str, name: &str| {
        mock(&c, uri).received_exchanges()[0]
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
        header("mock:iterated", TARFILE_ENTRY_NAME_HEADER)
    );
}

/// `TarFileSplitAndDeleteTest`.
#[tokio::test]
async fn test_tar_file_split_and_delete() {
    for with_data_format in [true, false] {
        let dir = test_dir("split-delete");
        let d = dir.clone();
        let c = context(move |r: &mut Routes| {
            let route = from(&format!("file://{d}?initialDelay=0&delay=10&delete=true"));
            let route = if with_data_format {
                route
                    .unmarshal(df(TarFileDataFormat::new().with_using_iterator(true)))
                    .split(body(), |s| s.convert_body_to("String").to("mock:end"))
            } else {
                route.split(
                    ExpressionDefinition::Expression(tar_splitter(
                        DEFAULT_MAXIMUM_DECOMPRESSED_SIZE,
                    )),
                    |s| s.convert_body_to("String").to("mock:end"),
                )
            };
            r.add(route.streaming());
        })
        .await;
        let end = mock(&c, "mock:end");
        end.expected_message_count(3);
        std::fs::copy(
            "tests/resources/data/tarfile3.tar",
            format!("{dir}/tarfile3.tar"),
        )
        .unwrap();
        end.assert_is_satisfied().await;
        let file = std::path::PathBuf::from(format!("{dir}/tarfile3.tar"));
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

/// The names of the entries of the only tar file of the directory.
fn tar_in_dir(dir: &str) -> Vec<(String, String)> {
    let files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(1, files.len(), "Should be one file in {dir} directory");
    entries(&std::fs::read(files[0].path()).unwrap())
}

/// `TarAggregationStrategyTest` and `AggregationStrategyWithPreservationTest`.
#[tokio::test]
async fn test_tar_aggregation_strategy_batch() {
    for preserve in [false, true] {
        let out = test_dir("aggregate");
        let o = out.clone();
        let c = context(move |r: &mut Routes| {
            let recursive = if preserve { "&recursive=true" } else { "" };
            r.add(
                from(&format!("file:tests/resources/org/apache/camel/aggregate/tarfile/data?initialDelay=0&delay=1000&noop=true{recursive}"))
                    .set_header("foo", constant("bar"))
                    .aggregate(constant(true), Arc::new(TarAggregationStrategy::new(preserve, preserve)), |a| {
                        a.to(&format!("file:{o}")).to("mock:aggregateToTarEntry")
                    })
                    .completion_from_batch_consumer()
                    .eager_check_completion(),
            );
        })
        .await;
        let m = mock(&c, "mock:aggregateToTarEntry");
        m.expected_message_count(1);
        m.expected_header_received("foo", "bar");
        m.assert_is_satisfied().await;
        let mut names: Vec<String> = tar_in_dir(&out).into_iter().map(|(n, _)| n).collect();
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

/// `AggregationStrategyWithFilenameHeaderTest`, `TarAggregationStrategyNullBodyTest` and the
/// repository storing copies of `TarAggregationStrategyRepositoryTest`.
#[tokio::test]
async fn test_tar_aggregation_strategy_direct() {
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
    // the file name header
    let out = test_dir("aggregate-filename");
    let o = out.clone();
    let c = context(move |r: &mut Routes| {
        r.add(
            from("direct:start")
                .aggregate(
                    constant(true),
                    Arc::new(TarAggregationStrategy::new(false, true)),
                    |a| a.to(&format!("file:{o}")).to("mock:aggregateToTarEntry"),
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
    mock(&c, "mock:aggregateToTarEntry").expected_message_count(1);
    mock(&c, "mock:aggregateToTarEntry")
        .assert_is_satisfied()
        .await;
    let names: Vec<String> = tar_in_dir(&out).into_iter().map(|(n, _)| n).collect();
    assert_eq!(vec!["foo", "bar"], names);
    c.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&out);

    // the null bodies, and the copying repository
    for (bodies, expected, copies) in [
        (["Hello", "Hello again", ""], 2, false),
        (["", "Hello", "Hello again"], 2, false),
        (["Hello", "", "Hello again"], 2, false),
        (["", "", ""], 0, false),
        (["", "", "Hello"], 1, false),
        (["Hello", "Hello again", "Bye"], 3, true),
    ] {
        let out = test_dir("aggregate-null");
        let o = out.clone();
        let c = context(move |r: &mut Routes| {
            let route = from("direct:start").aggregate(
                constant(true),
                Arc::new(TarAggregationStrategy::new(false, false)),
                |a| a.to(&format!("file:{o}")).to("mock:aggregateToTarEntry"),
            );
            let route = if copies {
                route.aggregation_repository(Arc::new(CopyingRepository::default()))
            } else {
                route.eager_check_completion()
            };
            r.add(route.completion_size(3));
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
        let m = mock(&c, "mock:aggregateToTarEntry");
        m.expected_message_count(1);
        m.assert_is_satisfied().await;
        assert_eq!(expected, tar_in_dir(&out).len(), "{bodies:?}");
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&out);
    }
}

/// `TarAggregationStrategyEmptyFirstFileTest`: the empty parts of a split are not added.
#[tokio::test]
async fn test_tar_aggregation_strategy_empty_first_file() {
    for messages in [
        vec!["A", "B", "C"],
        vec!["", "A"],
        vec![""],
        vec!["Start", "", "", "End"],
    ] {
        let out = test_dir("aggregate-empty");
        let o = out.clone();
        let c = context(move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .split(body().tokenize("#"), |s| {
                        s.set_header(FILE_NAME, simple("${exchangeProperty.CamelSplitIndex}"))
                    })
                    .aggregation_strategy(Arc::new(TarAggregationStrategy::new(false, true)))
                    .to(&format!("file:{o}"))
                    .to("mock:aggregateToTarEntry"),
            );
        })
        .await;
        // the parts (the Java entry names are their index; the contents are checked here)
        let input = messages.join("#");
        c.create_producer_template()
            .send_body("direct:start", input.as_str())
            .await
            .unwrap();
        let m = mock(&c, "mock:aggregateToTarEntry");
        m.expected_message_count(1);
        m.assert_is_satisfied().await;
        if std::fs::read_dir(&out).unwrap().next().is_some() {
            let tar = tar_in_dir(&out);
            let expected: Vec<String> = messages
                .iter()
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string())
                .collect();
            let contents: Vec<String> = tar.iter().map(|(_, c)| c.clone()).collect();
            assert_eq!(expected, contents, "{messages:?}");
        } else {
            assert!(messages.iter().all(|m| m.is_empty()));
        }
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&out);
    }
}
