//! Port of the camel-lzf `LZFDataFormatTest`.

use camel_core::prelude::*;
use camel_lzf::LzfDataFormat;
use std::sync::Arc;

const TEXT: &str = "Hamlet by William Shakespeare\nTo be, or not to be: that is the question:\nWhether 'tis nobler in the mind to suffer\nThe slings and arrows of outrageous fortune,\nOr to take arms against a sea of troubles,\nAnd by opposing end them? To die: to sleep;";

#[tokio::test]
async fn test_lzf() {
    let c = camel_core::default_context();
    camel_lzf::register(&c);
    c.add_routes(|r: &mut Routes| {
        let parallel: camel_core::api::spi::data_format::SharedDataFormat =
            Arc::new(LzfDataFormat::new().with_using_parallel_compression(true));
        r.add(from("direct:textToLzf").marshal("lzf"));
        r.add(
            from("direct:unMarshalTextToLzf")
                .marshal("lzf")
                .unmarshal("lzf")
                .to("mock:unMarshalTextToLzf"),
        );
        r.add(
            from("direct:parallelUnMarshalTextToLzf")
                .marshal(parallel.clone())
                .unmarshal(parallel)
                .to("mock:parallelUnMarshalTextToLzf"),
        );
    })
    .await
    .unwrap();
    c.start().await.unwrap();
    let t = c.create_producer_template();
    // testMarshalTextToLzf
    let out = t
        .request_body("direct:textToLzf", Value::Bytes(TEXT.as_bytes().to_vec()))
        .await
        .unwrap();
    let Value::Bytes(compressed) = out else {
        panic!("bytes: {out}")
    };
    assert!(compressed.starts_with(b"ZV"));
    assert_eq!(
        TEXT.as_bytes(),
        camel_lzf::decompress(&compressed).unwrap(),
        "Uncompressed something different than compressed"
    );
    // testUnMarshalTextToLzf and testParallelUnMarshalTextToLzf
    for (uri, m) in [
        ("direct:unMarshalTextToLzf", "mock:unMarshalTextToLzf"),
        (
            "direct:parallelUnMarshalTextToLzf",
            "mock:parallelUnMarshalTextToLzf",
        ),
    ] {
        let result = MockEndpoint::resolve(c.as_ref(), m).unwrap();
        result.expected_bodies_received([Value::Bytes(TEXT.as_bytes().to_vec())]);
        t.request_body(uri, Value::Bytes(TEXT.as_bytes().to_vec()))
            .await
            .unwrap();
        result.assert_is_satisfied().await;
    }
}
