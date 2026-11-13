//! Ports of the camel-zip-deflater tests: `ZipDeflaterDataFormatTest` (but the stream cache
//! case), `GzipDataFormatTest`, `ZipDeflaterDataFormatFileDeleteTest`,
//! `ZipDeflaterDataFormatFileUnmarshalDeleteTest`, `GzipDataFormatFileDeleteTest` and
//! `GzipDataFormatFileUnmarshalDeleteTest`.

use camel_core::api::exchange_constants::FILE_NAME;
use camel_core::api::spi::data_format::DataFormat;
use camel_core::model::builder::data_format;
use camel_core::prelude::*;
use camel_zip_deflater::{
    gunzip, inflate, ZipDeflaterDataFormat, BEST_COMPRESSION, BEST_SPEED, DEFAULT_COMPRESSION,
};
use std::sync::Arc;
use std::time::Duration;

const TEXT: &str = "The Cow in Apple Time \nby: Robert Frost \n\nSomething inspires the only cow of late\nTo make no more of a wall than an open gate,\nAnd think no more of wall-builders than fools.\nHer face is flecked with pomace and she drools\nA cider syrup. Having tasted fruit,\nShe scorns a pasture withering to the root.\nShe runs from tree to tree where lie and sweeten.\nThe windfalls spiked with stubble and worm-eaten.\nShe leaves them bitten when she has to fly.\nShe bellows on a knoll against the sky.\nHer udder shrivels and the milk goes dry.";

const HAMLET: &str = "Hamlet by William Shakespeare\nTo be, or not to be: that is the question:\nWhether 'tis nobler in the mind to suffer\nThe slings and arrows of outrageous fortune,\nOr to take arms against a sea of troubles,\nAnd by opposing end them? To die: to sleep;";

async fn context(routes: impl Fn(&mut Routes) + Send + Sync + 'static) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    camel_zip_deflater::register(&context);
    context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::Bytes(b) => b.clone(),
        other => other.to_string().into_bytes(),
    }
}

#[test]
fn test_marshal_mandatory_conversion_failed() {
    let mut e = Exchange::new(camel_core::api::context::StandaloneContext::shared());
    let r = ZipDeflaterDataFormat::default().marshal(&mut e, &Value::List(vec![]), &mut Vec::new());
    assert_eq!(ErrorKind::NoTypeConversionAvailable, r.unwrap_err().kind());
}

#[tokio::test]
async fn test_marshal_text_to_zip() {
    for level in [BEST_COMPRESSION, BEST_SPEED, DEFAULT_COMPRESSION] {
        let c = context(move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .marshal(data_format("zipDeflater").option("compressionLevel", level)),
            );
        })
        .await;
        let out = c
            .create_producer_template()
            .request_body("direct:start", TEXT)
            .await
            .unwrap();
        assert_eq!(
            TEXT.as_bytes(),
            &inflate(&bytes(&out)).unwrap()[..],
            "level {level}"
        );
    }
}

#[tokio::test]
async fn test_unmarshal_text_to_zip() {
    let c = context(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .marshal("zipDeflater")
                .unmarshal("zipDeflater")
                .to("mock:result"),
        );
    })
    .await;
    let result = MockEndpoint::resolve(c.as_ref(), "mock:result").unwrap();
    result.expected_bodies_received([TEXT]);
    c.create_producer_template()
        .send_body("direct:start", TEXT)
        .await
        .unwrap();
    result.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_gzip() {
    let c = context(|r: &mut Routes| {
        r.add(from("direct:marshal").marshal("gzipDeflater"));
        r.add(
            from("direct:start")
                .marshal("gzipDeflater")
                .unmarshal("gzipDeflater")
                .to("mock:result"),
        );
        r.add(
            from("direct:unmarshal")
                .unmarshal("gzipDeflater")
                .to("mock:concatenated"),
        );
    })
    .await;
    let t = c.create_producer_template();
    let out = t
        .request_body("direct:marshal", Value::Bytes(HAMLET.as_bytes().to_vec()))
        .await
        .unwrap();
    assert_eq!(HAMLET.as_bytes(), &gunzip(&bytes(&out)).unwrap()[..]);

    let result = MockEndpoint::resolve(c.as_ref(), "mock:result").unwrap();
    result.expected_bodies_received([Value::Bytes(HAMLET.as_bytes().to_vec())]);
    t.send_body("direct:start", Value::Bytes(HAMLET.as_bytes().to_vec()))
        .await
        .unwrap();
    result.assert_is_satisfied().await;

    // a gzip file with two members (RFC 1952 2.2)
    let gz = std::fs::read("tests/concatenatedZippedFiles.gz").unwrap();
    let concatenated = MockEndpoint::resolve(c.as_ref(), "mock:concatenated").unwrap();
    concatenated.expected_bodies_received([Value::Bytes(b"foo\nfoobar\n".to_vec())]);
    t.send_body("direct:unmarshal", Value::Bytes(gz))
        .await
        .unwrap();
    concatenated.assert_is_satisfied().await;
}

/// A new empty directory of a test.
fn test_dir(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "camel-deflater-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().replace('\\', "/")
}

/// The file tests: the consumed file is deleted, and the compressed file is created (or
/// consumed and deleted with `unmarshal`).
#[tokio::test]
async fn test_file_delete() {
    for (format, ext) in [("zipDeflater", "zip"), ("gzipDeflater", "gz")] {
        for unmarshal in [false, true] {
            let dir = test_dir(ext);
            let d = dir.clone();
            let c = context(move |r: &mut Routes| {
                let route = from(&format!("file:{d}?initialDelay=0&delay=10&delete=true"))
                    .marshal(format)
                    .to(&format!("file:{d}/out?fileName=${{file:name}}.{ext}"));
                if unmarshal {
                    r.add(route);
                    r.add(
                        from(&format!("file:{d}/out?initialDelay=0&delay=10&delete=true"))
                            .unmarshal(format)
                            .to("mock:result"),
                    );
                } else {
                    r.add(route.to("mock:result"));
                }
            })
            .await;
            let mock = MockEndpoint::resolve(c.as_ref(), "mock:result").unwrap();
            if unmarshal {
                mock.expected_bodies_received([Value::Bytes(b"Hello World".to_vec())]);
            } else {
                mock.expected_message_count(1);
            }
            c.create_producer_template()
                .send_body_and_header(
                    &format!("file:{dir}"),
                    "Hello World",
                    FILE_NAME,
                    "hello.txt",
                )
                .await
                .unwrap();
            mock.assert_is_satisfied().await;
            let input = std::path::Path::new(&dir).join("hello.txt");
            let out = std::path::Path::new(&dir).join(format!("out/hello.txt.{ext}"));
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            while input.exists() || (unmarshal && out.exists()) {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "should have been deleted: {format} {unmarshal}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if !unmarshal {
                assert!(out.exists(), "should have been created {out:?}");
            }
            c.stop().await.unwrap();
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
