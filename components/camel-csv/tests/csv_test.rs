//! Ports of the camel-csv tests (the Spring XML ones in the XML DSL; the bodies are lists and
//! maps of values).

use camel_core::api::context::CamelContext;
use camel_core::api::exchange_constants::CHARSET_NAME;
use camel_core::api::spi::data_format::SharedDataFormat;
use camel_core::prelude::*;
use camel_csv::format::{CsvFormat, QuoteMode};
use camel_csv::parser::{parse, CsvRecord};
use camel_csv::{CsvDataFormat, CsvMarshaller, DefaultCsvMarshaller};
use std::sync::{Arc, Mutex};

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_csv::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

async fn xml_context(
    xml: &str,
    setup: impl FnOnce(&Arc<DefaultCamelContext>),
) -> Arc<DefaultCamelContext> {
    let c = camel_core::default_context();
    camel_csv::register(&c);
    setup(&c);
    c.add_routes(camel_xml_io_dsl::XmlRoutes::parse(xml).unwrap())
        .await
        .unwrap();
    c.start().await.unwrap();
    c
}

fn mock(c: &Arc<DefaultCamelContext>, uri: &str) -> MockEndpoint {
    MockEndpoint::resolve(c.as_ref(), uri).unwrap()
}

fn df(d: CsvDataFormat) -> SharedDataFormat {
    Arc::new(d)
}

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|s| Value::from(*s)).collect())
}

fn map(pairs: &[(&str, Value)]) -> Value {
    let mut m = ValueMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v.clone());
    }
    Value::Map(m)
}

fn text(v: &Value) -> String {
    match v {
        Value::Bytes(b) => String::from_utf8(b.clone()).unwrap(),
        other => other.to_string(),
    }
}

fn body_of(m: &MockEndpoint, i: usize) -> Value {
    m.received_exchanges()[i].message().body().clone()
}

fn lines(m: &MockEndpoint, i: usize) -> Vec<String> {
    text(&body_of(m, i))
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

/// The orders of the pipe delimiter tests.
fn orders() -> Value {
    Value::List(vec![
        map(&[
            ("orderId", Value::Int(123)),
            ("item", Value::from("Camel in Action")),
            ("amount", Value::Int(1)),
        ]),
        map(&[
            ("orderId", Value::Int(124)),
            ("item", Value::from("ActiveMQ in Action")),
            ("amount", Value::Int(2)),
        ]),
    ])
}

/// `CsvMarshalTest`.
#[tokio::test]
async fn test_marshal() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:default")
                .marshal(df(CsvDataFormat::new()))
                .to("mock:output"),
        );
        r.add(
            from("direct:headers")
                .marshal(df(CsvDataFormat::new().with_header(&["A", "C"])))
                .to("mock:output"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let out = mock(&c, "mock:output");
    let abc = |a: &str, b: &str, cc: &str| {
        map(&[
            ("A", Value::from(a)),
            ("B", Value::from(b)),
            ("C", Value::from(cc)),
        ])
    };
    t.send_body(
        "direct:default",
        Value::List(vec![list(&["1", "2", "3"]), list(&["one", "two", "three"])]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["1,2,3", "one,two,three"], lines(&out, 0));
    t.send_body(
        "direct:default",
        Value::List(vec![list(&["1"]), list(&["one"])]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["1", "one"], lines(&out, 1));
    t.send_body(
        "direct:default",
        Value::List(vec![abc("1", "2", "3"), abc("one", "two", "three")]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["1,2,3", "one,two,three"], lines(&out, 2));
    t.send_body("direct:default", abc("1", "2", "3"))
        .await
        .unwrap();
    assert_eq!(vec!["1,2,3"], lines(&out, 3));
    t.send_body(
        "direct:headers",
        Value::List(vec![abc("1", "2", "3"), abc("one", "two", "three")]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["A,C", "1,3", "one,three"], lines(&out, 4));
    t.send_body(
        "direct:default",
        map(&[("A", Value::from("1")), ("B", Value::from("2"))]),
    )
    .await
    .unwrap();
    t.send_body(
        "direct:default",
        map(&[
            ("X", Value::from("1")),
            ("Y", Value::from("2")),
            ("Z", Value::from("3")),
        ]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["1,2"], lines(&out, 5));
    assert_eq!(vec!["1,2,3"], lines(&out, 6));
}

/// `CsvUnmarshalTest`, `CsvUnmarshalStreamSpringTest` and `CsvRecordConvertersTest`.
#[tokio::test]
async fn test_unmarshal() {
    let sample = "A,B,C\r1,2,3\rone,two,three";
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:default")
                .unmarshal(df(CsvDataFormat::new()))
                .to("mock:output"),
        );
        r.add(
            from("direct:delimiter")
                .unmarshal(df(CsvDataFormat::new().with_delimiter('_')))
                .to("mock:output"),
        );
        r.add(
            from("direct:autoload_headers")
                .unmarshal(df(CsvDataFormat::new().with_capture_header_record(true)))
                .to("mock:output"),
        );
        r.add(
            from("direct:lazy")
                .unmarshal(df(CsvDataFormat::new().with_lazy_load(true)))
                .split(body(), |s| s.to("mock:line")),
        );
        r.add(
            from("direct:map")
                .unmarshal(df(CsvDataFormat::new().with_use_maps(true)))
                .to("mock:output"),
        );
        r.add(
            from("direct:orderedmap")
                .unmarshal(df(CsvDataFormat::new().with_use_ordered_maps(true)))
                .to("mock:output"),
        );
        r.add(
            from("direct:lazy_map")
                .unmarshal(df(CsvDataFormat::new()
                    .with_lazy_load(true)
                    .with_use_maps(true)))
                .split(body(), |s| s.to("mock:line")),
        );
        r.add(
            from("direct:map_headers")
                .unmarshal(df(CsvDataFormat::new()
                    .with_use_maps(true)
                    .with_skip_header_record(true)
                    .with_header(&["AA", "BB", "CC"])))
                .to("mock:output"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let out = mock(&c, "mock:output");
    let line = mock(&c, "mock:line");
    let rows = Value::List(vec![
        list(&["A", "B", "C"]),
        list(&["1", "2", "3"]),
        list(&["one", "two", "three"]),
    ]);
    t.send_body("direct:default", sample).await.unwrap();
    assert_eq!(rows, body_of(&out, 0));
    t.send_body("direct:delimiter", sample.replace(',', "_"))
        .await
        .unwrap();
    assert_eq!(rows, body_of(&out, 1));
    t.send_body("direct:autoload_headers", sample)
        .await
        .unwrap();
    assert_eq!(
        Some(&list(&["A", "B", "C"])),
        out.received_exchanges()[2]
            .message()
            .header("CamelCsvHeaderRecord")
    );
    assert_eq!(
        Value::List(vec![list(&["1", "2", "3"]), list(&["one", "two", "three"])]),
        body_of(&out, 2)
    );
    line.expected_message_count(3);
    t.send_body("direct:lazy", sample).await.unwrap();
    line.assert_is_satisfied().await;
    for (i, r) in [
        list(&["A", "B", "C"]),
        list(&["1", "2", "3"]),
        list(&["one", "two", "three"]),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(r, &body_of(&line, i));
    }
    let maps = |a: &str, b: &str, cc: &str| {
        Value::List(vec![
            map(&[
                (a, Value::from("1")),
                (b, Value::from("2")),
                (cc, Value::from("3")),
            ]),
            map(&[
                (a, Value::from("one")),
                (b, Value::from("two")),
                (cc, Value::from("three")),
            ]),
        ])
    };
    t.send_body("direct:map", sample).await.unwrap();
    assert_eq!(maps("A", "B", "C"), body_of(&out, 3));
    t.send_body("direct:orderedmap", sample).await.unwrap();
    let Value::List(l) = body_of(&out, 4) else {
        panic!("a list")
    };
    let keys: Vec<&String> = l[0].as_map().unwrap().keys().collect();
    assert_eq!(vec!["A", "B", "C"], keys);
    line.reset();
    line.expected_message_count(2);
    t.send_body("direct:lazy_map", sample).await.unwrap();
    line.assert_is_satisfied().await;
    let Value::List(expected) = maps("A", "B", "C") else {
        unreachable!()
    };
    assert_eq!(expected, vec![body_of(&line, 0), body_of(&line, 1)]);
    t.send_body("direct:map_headers", sample).await.unwrap();
    assert_eq!(maps("AA", "BB", "CC"), body_of(&out, 5));
    // CsvRecordConvertersTest
    let format = CsvFormat {
        header: Some(vec!["A".into(), "B".into(), "C".into()]),
        ..CsvFormat::default()
    };
    let record = &parse(&format, "1,2,3").unwrap()[0];
    assert_eq!(list(&["1", "2", "3"]), camel_csv::converters::list(record));
    assert_eq!(
        map(&[
            ("A", Value::from("1")),
            ("B", Value::from("2")),
            ("C", Value::from("3"))
        ]),
        camel_csv::converters::map(record)
    );
}

/// `CsvMarshalCharsetTest`, `CsvRouteCharsetTest` and `CsvRouteTest`.
#[tokio::test]
async fn test_routes_and_charsets() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:charset").marshal("csv").to("mock:daltons"));
        r.add(from("direct:start").marshal("csv").to("mock:result"));
        r.add(
            from("direct:startMultiCustom")
                .marshal(df(CsvDataFormat::new()
                    .with_delimiter(';')
                    .with_header(&["foo", "baz", "bar"])
                    .with_skip_header_record(true)))
                .to("mock:resultMultiCustom"),
        );
        r.add(
            from("file:tests/resources?fileName=daltons.csv&noop=true&initialDelay=0")
                .unmarshal("csv")
                .to("mock:file"),
        );
        r.add(
            from("file:tests/resources?fileName=daltons-utf-8.csv&noop=true&initialDelay=0")
                .unmarshal("csv")
                .to("mock:fileUtf8"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let shared: Arc<dyn CamelContext> = c.clone();
    let mut e = Exchange::new(shared);
    e.message_mut()
        .set_body(Value::List(vec![list(&["L\u{00fc}cky Luke"])]));
    e.set_property(CHARSET_NAME, "ISO-8859-1");
    t.send("direct:charset", e).await.unwrap();
    let Value::Bytes(b) = body_of(&mock(&c, "mock:daltons"), 0) else {
        panic!("bytes")
    };
    assert!(b.starts_with(&[b'L', 0xfc, b'c']), "{b:?}");
    let result = mock(&c, "mock:result");
    t.send_body(
        "direct:start",
        map(&[("foo", Value::from("abc")), ("bar", Value::Int(123))]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["abc,123"], lines(&result, 0));
    t.send_body(
        "direct:start",
        map(&[
            ("foo", Value::from("def")),
            ("bar", Value::Int(456)),
            ("baz", Value::Int(789)),
        ]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["def,456,789"], lines(&result, 1));
    let custom = mock(&c, "mock:resultMultiCustom");
    t.send_body(
        "direct:startMultiCustom",
        map(&[("foo", Value::from("abc")), ("bar", Value::Int(123))]),
    )
    .await
    .unwrap();
    t.send_body(
        "direct:startMultiCustom",
        map(&[
            ("foo", Value::from("def")),
            ("bar", Value::Int(456)),
            ("baz", Value::Int(789)),
            ("buz", Value::from("000")),
        ]),
    )
    .await
    .unwrap();
    assert_eq!(vec!["abc;;123"], lines(&custom, 0));
    assert_eq!(vec!["def;789;456"], lines(&custom, 1));
    let file = mock(&c, "mock:file");
    file.expected_message_count(1);
    file.assert_is_satisfied().await;
    let Value::List(rows) = body_of(&file, 0) else {
        panic!("a list")
    };
    assert_eq!(list(&["Jack Dalton", " 115", " mad at Averell"]), rows[0]);
    let utf8 = mock(&c, "mock:fileUtf8");
    utf8.expected_message_count(1);
    utf8.assert_is_satisfied().await;
    let Value::List(rows) = body_of(&utf8, 0) else {
        panic!("a list")
    };
    let names: Vec<Value> = rows
        .iter()
        .map(|r| r.as_list().unwrap()[0].clone())
        .collect();
    assert_eq!(
        vec![
            Value::from("Jäck Dalton"),
            Value::from("Jöe Dalton"),
            Value::from("Lücky Luke")
        ],
        names[..3]
    );
}

/// `CsvMarshalPipeDelimiterTest`, `CsvUnmarshalPipeDelimiterTest`,
/// `CsvUnmarshalTabDelimiterTest`, `CsvUnmarshalTwoCsvDataFormatConcurrentTest` and
/// `CsvUnmarshalStreamTest`.
#[tokio::test]
async fn test_delimiters_and_stream() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:marshalPipe")
                .marshal(df(CsvDataFormat::new()
                    .with_delimiter('|')
                    .with_header_disabled(true)))
                .convert_body_to("String")
                .to("mock:result"),
        );
        r.add(
            from("direct:pipe")
                .unmarshal(df(CsvDataFormat::new().with_delimiter('|')))
                .to("mock:result"),
        );
        r.add(
            from("direct:tab")
                .unmarshal(df(CsvDataFormat::new().with_delimiter('\t')))
                .to("mock:result"),
        );
        r.add(
            from("direct:both")
                .multicast(|m| m.to("direct:csv").to("direct:csv2"))
                .parallel_processing(),
        );
        r.add(
            from("direct:csv")
                .unmarshal(df(CsvDataFormat::new().with_delimiter('|')))
                .to("mock:result1"),
        );
        r.add(
            from("direct:csv2")
                .unmarshal(df(CsvDataFormat::new().with_delimiter(';')))
                .to("mock:result2"),
        );
        r.add(
            from("direct:stream")
                .unmarshal(df(CsvDataFormat::new()
                    .with_lazy_load(true)
                    .with_delimiter('|')))
                .split(body(), |s| s.to("mock:line")),
        );
    })
    .await;
    let t = c.create_producer_template();
    let result = mock(&c, "mock:result");
    t.send_body("direct:marshalPipe", orders()).await.unwrap();
    assert_eq!(
        vec!["123|Camel in Action|1", "124|ActiveMQ in Action|2"],
        lines(&result, 0)
    );
    let two = Value::List(vec![
        list(&["123", "Camel in Action", "1"]),
        list(&["124", "ActiveMQ in Action", "2"]),
    ]);
    let one = Value::List(vec![list(&["123", "Camel in Action", "1"])]);
    let mut i = 1;
    for (uri, d) in [("direct:pipe", "|"), ("direct:tab", "\t")] {
        t.send_body(
            uri,
            format!("123{d}Camel in Action{d}1\n124{d}ActiveMQ in Action{d}2"),
        )
        .await
        .unwrap();
        assert_eq!(two, body_of(&result, i));
        t.send_body(uri, format!("123{d}Camel in Action{d}1"))
            .await
            .unwrap();
        assert_eq!(one, body_of(&result, i + 1));
        i += 2;
    }
    for (d, m) in [("|", "mock:result1"), (";", "mock:result2")] {
        mock(&c, "mock:result1").reset();
        mock(&c, "mock:result2").reset();
        mock(&c, "mock:result1").expected_message_count(1);
        mock(&c, "mock:result2").expected_message_count(1);
        t.send_body(
            "direct:both",
            format!("123{d}Camel in Action{d}1\n124{d}ActiveMQ in Action{d}2"),
        )
        .await
        .unwrap();
        mock(&c, "mock:result1").assert_is_satisfied().await;
        mock(&c, "mock:result2").assert_is_satisfied().await;
        assert_eq!(two, body_of(&mock(&c, m), 0));
    }
    let line = mock(&c, "mock:line");
    let mut message = String::new();
    for i in 0..3 {
        message.push_str(&format!("{i}|\"{i}\n{i}\"\n"));
    }
    for b in [
        Value::from(message),
        Value::Bytes(std::fs::read("tests/resources/data.csv").unwrap()),
    ] {
        line.reset();
        line.expected_message_count(3);
        t.send_body("direct:stream", b).await.unwrap();
        line.assert_is_satisfied().await;
        for i in 0..3 {
            assert_eq!(
                list(&[&i.to_string(), &format!("{i}\n{i}")]),
                body_of(&line, i)
            );
        }
    }
}

/// The lines of a file, without the blank ones.
fn file_lines(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

/// A marshaller printing the header once (`CsvMarshalHeaderWithCustomMarshallFactoryTest`).
struct SinglePrinterCsvMarshaller {
    marshaller: DefaultCsvMarshaller,
    header_printed: Mutex<bool>,
}

impl CsvMarshaller for SinglePrinterCsvMarshaller {
    fn marshal(
        &self,
        _exchange: &mut Exchange,
        body: &Value,
        out: &mut Vec<u8>,
    ) -> camel_core::api::error::CamelResult<()> {
        let mut printed = self.header_printed.lock().unwrap();
        let mut text = String::new();
        if !*printed {
            self.marshaller.format().print_header(&mut text);
            *printed = true;
        }
        let Value::Map(m) = body else { panic!("a map") };
        self.marshaller
            .format()
            .print_record(&mut text, &self.marshaller.map_record_values(m));
        out.extend(text.into_bytes());
        Ok(())
    }
}

/// `CsvMarshalHeaderTest`, `CsvMarshalHeaderFromCsvFormatTest`, `CsvMarshalHeaderUriTest`
/// and `CsvMarshalHeaderWithCustomMarshallFactoryTest`.
#[tokio::test]
async fn test_marshal_header_to_file() {
    let header_df = || {
        CsvDataFormat::new()
            .with_delimiter('\t')
            .with_trim(true)
            .with_ignore_surrounding_spaces(true)
            .with_header(&["first_name", "last_name"])
    };
    let from_format = || {
        CsvDataFormat::of_format(CsvFormat {
            delimiter: "\t".into(),
            trim: true,
            ignore_surrounding_spaces: true,
            header: Some(vec!["first_name".into(), "last_name".into()]),
            ..CsvFormat::default()
        })
    };
    let custom = move || {
        header_df().with_marshaller_factory(|format: &CsvFormat, _: &CsvDataFormat| {
            Arc::new(SinglePrinterCsvMarshaller {
                marshaller: DefaultCsvMarshaller::new(format),
                header_printed: Mutex::new(false),
            }) as Arc<dyn CsvMarshaller>
        })
    };
    for (name, kind, lists) in [
        ("header", 0, true),
        ("format", 1, true),
        ("uri", 2, true),
        ("custom", 3, false),
    ] {
        let dir = std::env::current_dir()
            .unwrap()
            .join(format!("target/csv-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        let uri = format!("file:{}?charset=utf-8&fileExist=Append", dir.display());
        let c = context(move |r: &mut Routes| {
            let route = from("direct:start");
            let route = match kind {
                0 => route.marshal(df(header_df())),
                1 => route.marshal(df(from_format())),
                2 => route.to("dataformat:csv:marshal?header=first_name,last_name&trim=true&ignoreSurroundingSpaces=true"),
                _ => route.marshal(df(custom())),
            };
            r.add(route.to(&uri));
        })
        .await;
        let t = c.create_producer_template();
        let file = dir.join("output.csv");
        for (first, last) in [("John", "Doe"), ("Max", "Mustermann")] {
            let m = map(&[
                ("first_name", Value::from(first)),
                ("last_name", Value::from(last)),
            ]);
            t.send_body_and_header("direct:start", m, "CamelFileName", "output.csv")
                .await
                .unwrap();
        }
        assert_eq!(if kind == 3 { 3 } else { 4 }, file_lines(&file), "{name}");
        if lists {
            std::fs::remove_file(&file).unwrap();
            for (first, last) in [("John", "Doe"), ("Max", "Mustermann")] {
                t.send_body_and_header(
                    "direct:start",
                    Value::List(vec![list(&[first, last])]),
                    "CamelFileName",
                    "output.csv",
                )
                .await
                .unwrap();
            }
            assert_eq!(4, file_lines(&file), "{name}");
        }
        c.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `CsvDataFormatTest`: the active format of the options.
#[test]
fn test_active_format() {
    let active = |d: CsvDataFormat| d.active_format().unwrap();
    assert_eq!(CsvFormat::default(), active(CsvDataFormat::new()));
    assert_eq!(
        CsvFormat::excel(),
        active(CsvDataFormat::of_format(CsvFormat::excel()))
    );
    assert_eq!(
        CsvFormat::mysql(),
        active(CsvDataFormat::of_format(CsvFormat::mysql()))
    );
    assert_eq!(
        CsvFormat::mysql(),
        active(CsvDataFormat::new().with_format("MYSQL"))
    );
    assert_eq!(
        "\t",
        active(CsvDataFormat::new().with_format("MYSQL")).delimiter
    );
    assert_eq!(
        CsvFormat::excel(),
        active(CsvDataFormat::new().with_format("EXCEL"))
    );
    assert_eq!(
        None,
        active(
            CsvDataFormat::new()
                .with_comment_marker_disabled(true)
                .with_comment_marker('c')
        )
        .comment_marker
    );
    assert_eq!(
        Some('c'),
        active(CsvDataFormat::new().with_comment_marker('c')).comment_marker
    );
    assert_eq!(
        "d",
        active(CsvDataFormat::new().with_delimiter('d')).delimiter
    );
    assert_eq!(
        None,
        active(
            CsvDataFormat::new()
                .with_escape_disabled(true)
                .with_escape('e')
        )
        .escape
    );
    assert_eq!(
        Some('e'),
        active(CsvDataFormat::new().with_escape('e')).escape
    );
    let disabled = CsvDataFormat::new()
        .with_header_disabled(true)
        .with_header(&["a", "b", "c"]);
    assert_eq!(Some("a,b,c".to_string()), disabled.header());
    assert_eq!(None, active(disabled).header);
    assert_eq!(
        Some(vec!["a".to_string(), "b".into(), "c".into()]),
        active(CsvDataFormat::of_options(&[("header".into(), "a,b,c".into())]).unwrap()).header
    );
    for v in [true, false] {
        assert_eq!(
            v,
            active(CsvDataFormat::new().with_allow_missing_column_names(v))
                .allow_missing_column_names
        );
        assert_eq!(
            v,
            active(CsvDataFormat::new().with_ignore_empty_lines(v)).ignore_empty_lines
        );
        assert_eq!(
            v,
            active(CsvDataFormat::new().with_ignore_surrounding_spaces(v))
                .ignore_surrounding_spaces
        );
        assert_eq!(
            v,
            active(CsvDataFormat::new().with_skip_header_record(v)).skip_header_record
        );
        assert_eq!(v, active(CsvDataFormat::new().with_trim(v)).trim);
        assert_eq!(
            v,
            active(CsvDataFormat::new().with_ignore_header_case(v)).ignore_header_case
        );
        assert_eq!(
            v,
            active(CsvDataFormat::new().with_trailing_delimiter(v)).trailing_delimiter
        );
    }
    assert!(!active(CsvDataFormat::new()).trim);
    assert_eq!(
        None,
        active(
            CsvDataFormat::new()
                .with_null_string_disabled(true)
                .with_null_string("****")
        )
        .null_string
    );
    assert_eq!(
        Some("****".to_string()),
        active(CsvDataFormat::new().with_null_string("****")).null_string
    );
    assert_eq!(
        None,
        active(
            CsvDataFormat::new()
                .with_quote_disabled(true)
                .with_quote('q')
        )
        .quote
    );
    assert_eq!(
        Some('q'),
        active(CsvDataFormat::new().with_quote('q')).quote
    );
    assert_eq!(
        Some(QuoteMode::All),
        active(CsvDataFormat::new().with_quote_mode(QuoteMode::All)).quote_mode
    );
    assert_eq!(
        None,
        active(
            CsvDataFormat::new()
                .with_record_separator_disabled(true)
                .with_record_separator("separator")
        )
        .record_separator
    );
    assert_eq!(
        Some("separator".to_string()),
        active(CsvDataFormat::new().with_record_separator("separator")).record_separator
    );
    let lazy = CsvDataFormat::new().with_lazy_load(true);
    assert!(lazy.is_lazy_load());
    assert_eq!(CsvFormat::default(), active(lazy));
    let maps = CsvDataFormat::new().with_use_maps(true);
    assert!(maps.is_use_maps());
    assert_eq!(CsvFormat::default(), active(maps));
    let converter =
        CsvDataFormat::new().with_record_converter(|r: &CsvRecord| Value::from(r.to_string()));
    assert_eq!(CsvFormat::default(), active(converter));
    assert!(CsvDataFormat::of_options(&[("format".into(), "UNKNOWN".into())]).is_err());
}

/// The Spring XML tests in the XML DSL: `CsvMarshalAutogenColumnsSpringTest`,
/// `CsvMarshalAutogenColumnsSpringQuoteModeTest`, `CsvMarshalPipeDelimiterSpringTest`,
/// `CsvUnmarshalCommentMarkerSpringTest`, `CsvUnmarshalMapLineSpringTest`,
/// `CsvUnmarshalPipeDelimiterSpringTest`, `CsvUnmarshalSkipFirstLineSpringTest`,
/// `CsvUnmarshalStreamSpringTest` and `CsvUnmarshalTabDelimiterSpringTest`.
#[tokio::test]
async fn test_spring_xml() {
    let xml = r##"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route><from uri="direct:autogen"/><marshal><csv delimiter="|" skipHeaderRecord="true" header="item"/></marshal><convertBodyTo type="java.lang.String"/><to uri="mock:autogen"/></route>
  <route><from uri="direct:autogen2"/><marshal><csv format="EXCEL" delimiter="|" skipHeaderRecord="true" header="orderId,amount"/></marshal><convertBodyTo type="java.lang.String"/><to uri="mock:autogen2"/></route>
  <route><from uri="direct:quoteMode"/><marshal><csv delimiter="|" skipHeaderRecord="true" quoteMode="ALL" header="item"/></marshal><convertBodyTo type="java.lang.String"/><to uri="mock:quoteMode"/></route>
  <route><from uri="direct:marshalPipe"/><marshal><csv delimiter="|" skipHeaderRecord="true"/></marshal><convertBodyTo type="java.lang.String"/><to uri="mock:marshalPipe"/></route>
  <route><from uri="direct:comment"/><unmarshal><csv delimiter="|" commentMarker="#"/></unmarshal><to uri="mock:comment"/></route>
  <route><from uri="direct:mapLine"/><unmarshal><csv delimiter="|" useMaps="true"/></unmarshal><to uri="mock:mapLine"/></route>
  <route><from uri="direct:explicitHeader"/><unmarshal><csv delimiter="|" useMaps="true" header="MyOrderId,MyItem,MyAmount"/></unmarshal><to uri="mock:mapLine"/></route>
  <route><from uri="direct:replaceHeader"/><unmarshal><csv delimiter="|" useMaps="true" skipHeaderRecord="true" header="MyOrderId,MyItem,MyAmount"/></unmarshal><to uri="mock:mapLine"/></route>
  <route><from uri="direct:pipe"/><unmarshal><csv delimiter="|" headerDisabled="true"/></unmarshal><to uri="mock:pipe"/></route>
  <route><from uri="direct:skipFirst"/><unmarshal><csv delimiter="|" skipHeaderRecord="true"/></unmarshal><to uri="mock:skipFirst"/></route>
  <route><from uri="direct:stream"/><unmarshal><csv delimiter="," lazyLoad="true"/></unmarshal><split><simple>${body}</simple><to uri="mock:line"/></split></route>
  <route><from uri="direct:tab"/><unmarshal><csv delimiter="&#x9;"/></unmarshal><to uri="mock:tab"/></route>
</routes>"##;
    let c = xml_context(xml, |_| {}).await;
    let t = c.create_producer_template();
    for (uri, m, expected) in [
        (
            "direct:autogen",
            "mock:autogen",
            vec!["Camel in Action", "ActiveMQ in Action"],
        ),
        ("direct:autogen2", "mock:autogen2", vec!["123|1", "124|2"]),
        (
            "direct:quoteMode",
            "mock:quoteMode",
            vec!["\"Camel in Action\"", "\"ActiveMQ in Action\""],
        ),
        (
            "direct:marshalPipe",
            "mock:marshalPipe",
            vec!["123|Camel in Action|1", "124|ActiveMQ in Action|2"],
        ),
    ] {
        t.send_body(uri, orders()).await.unwrap();
        assert_eq!(expected, lines(&mock(&c, m), 0), "{uri}");
    }
    let two = Value::List(vec![
        list(&["123", "Camel in Action", "1"]),
        list(&["124", "ActiveMQ in Action", "2"]),
    ]);
    t.send_body(
        "direct:comment",
        "# Cool books list (comment)\n123|Camel in Action|1\n124|ActiveMQ in Action|2",
    )
    .await
    .unwrap();
    assert_eq!(two, body_of(&mock(&c, "mock:comment"), 0));
    let maps = |a: &str, b: &str, cc: &str| {
        Value::List(vec![
            map(&[
                (a, Value::from("123")),
                (b, Value::from("Camel in Action")),
                (cc, Value::from("1")),
            ]),
            map(&[
                (a, Value::from("124")),
                (b, Value::from("ActiveMQ in Action")),
                (cc, Value::from("2")),
            ]),
        ])
    };
    let map_line = mock(&c, "mock:mapLine");
    t.send_body(
        "direct:mapLine",
        "OrderId|Item|Amount\n123|Camel in Action|1\n124|ActiveMQ in Action|2",
    )
    .await
    .unwrap();
    assert_eq!(maps("OrderId", "Item", "Amount"), body_of(&map_line, 0));
    t.send_body("direct:mapLine", "OrderId|Item|Amount\n")
        .await
        .unwrap();
    assert_eq!(Value::List(vec![]), body_of(&map_line, 1));
    t.send_body(
        "direct:explicitHeader",
        "123|Camel in Action|1\n124|ActiveMQ in Action|2",
    )
    .await
    .unwrap();
    assert_eq!(
        maps("MyOrderId", "MyItem", "MyAmount"),
        body_of(&map_line, 2)
    );
    t.send_body(
        "direct:replaceHeader",
        "a|b|c\n123|Camel in Action|1\n124|ActiveMQ in Action|2",
    )
    .await
    .unwrap();
    assert_eq!(
        maps("MyOrderId", "MyItem", "MyAmount"),
        body_of(&map_line, 3)
    );
    for (uri, m, d) in [
        ("direct:pipe", "mock:pipe", "|"),
        ("direct:tab", "mock:tab", "\t"),
    ] {
        t.send_body(
            uri,
            format!("123{d}Camel in Action{d}1\n124{d}ActiveMQ in Action{d}2"),
        )
        .await
        .unwrap();
        assert_eq!(two, body_of(&mock(&c, m), 0), "{uri}");
        t.send_body(uri, format!("123{d}Camel in Action{d}1"))
            .await
            .unwrap();
        assert_eq!(
            Value::List(vec![list(&["123", "Camel in Action", "1"])]),
            body_of(&mock(&c, m), 1),
            "{uri}"
        );
    }
    let skip = mock(&c, "mock:skipFirst");
    t.send_body(
        "direct:skipFirst",
        "OrderId|Item|Amount\n123|Camel in Action|1\n124|ActiveMQ in Action|2",
    )
    .await
    .unwrap();
    assert_eq!(two, body_of(&skip, 0));
    t.send_body(
        "direct:skipFirst",
        "124|ActiveMQ in Action|2\n123|Camel in Action|1",
    )
    .await
    .unwrap();
    assert_eq!(
        Value::List(vec![list(&["123", "Camel in Action", "1"])]),
        body_of(&skip, 1)
    );
    t.send_body("direct:skipFirst", "123|Camel in Action|1\n")
        .await
        .unwrap();
    assert_eq!(Value::List(vec![]), body_of(&skip, 2));
    let line = mock(&c, "mock:line");
    line.expected_message_count(3);
    t.send_body("direct:stream", "A,B,C\r1,2,3\rone,two,three")
        .await
        .unwrap();
    line.assert_is_satisfied().await;
    assert_eq!(list(&["one", "two", "three"]), body_of(&line, 2));
}

/// `CsvDataFormatCustomRecordConverterTest`, `CsvDataFormatCustomFactoryTest` (the
/// `csv-dataformat-factory` bean) and `CsvDataFormatResourceFactoryTest`.
#[tokio::test]
async fn test_custom_beans() {
    let xml = r##"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route><from uri="direct:convert"/><unmarshal><csv recordConverterRef="record-converter" delimiter=";" headerDisabled="true"/></unmarshal><to uri="mock:unmarshaled"/></route>
  <route><from uri="direct:marshal"/><marshal><csv headerDisabled="true" quoteDisabled="true"/></marshal><convertBodyTo type="java.lang.String"/><to uri="mock:marshaled"/></route>
  <route><from uri="direct:unmarshal"/><unmarshal><csv delimiter=";" headerDisabled="true"/></unmarshal><to uri="mock:unmarshaled2"/></route>
</routes>"##;
    let data = Value::List(vec![list(&["A1", "B1", "C1"]), list(&["A2", "B2", "C2"])]);
    for factory in [true, false] {
        let c = xml_context(xml, |c| {
            let r = c.registry().unwrap();
            let converter: camel_csv::SharedRecordConverter =
                Arc::new(|_: &CsvRecord| list(&["Hello", "Again", "Democracy"]));
            r.bind(
                "record-converter",
                camel_csv::record_converter_value(converter),
            )
            .unwrap();
            if factory {
                // MyCsvDataFormatFactory: the data formats with the ':' delimiter
                let f: camel_core::api::spi::data_format::DataFormatFactory = Arc::new(|options| {
                    let mut options = options.to_vec();
                    options.insert(0, ("delimiter".to_string(), ":".to_string()));
                    Ok(Arc::new(CsvDataFormat::of_options(&options)?) as SharedDataFormat)
                });
                r.bind(
                    "csv-dataformat-factory",
                    camel_core::api::spi::data_format::data_format_factory_value(f),
                )
                .unwrap();
            }
        })
        .await;
        let t = c.create_producer_template();
        t.send_body("direct:convert", "A1;B1;C1").await.unwrap();
        assert_eq!(
            Value::List(vec![list(&["Hello", "Again", "Democracy"])]),
            body_of(&mock(&c, "mock:unmarshaled"), 0)
        );
        t.send_body("direct:marshal", data.clone()).await.unwrap();
        let expected = if factory {
            vec!["A1:B1:C1", "A2:B2:C2"]
        } else {
            vec!["A1,B1,C1", "A2,B2,C2"]
        };
        assert_eq!(expected, lines(&mock(&c, "mock:marshaled"), 0));
        t.send_body("direct:unmarshal", "A1;B1;C1").await.unwrap();
        assert_eq!(
            Value::List(vec![list(&["A1", "B1", "C1"])]),
            body_of(&mock(&c, "mock:unmarshaled2"), 0)
        );
    }
}
