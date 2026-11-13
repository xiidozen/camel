//! The data format component with the base64 and zipDeflater data formats (the Java module has
//! no tests of its own; `DataFormatComponentTest` of camel-jaxb and `CsvMarshalHeaderUriTest`
//! use it).

use camel_core::prelude::*;

#[tokio::test]
async fn test_dataformat_endpoints() {
    let context = camel_core::default_context();
    camel_base64::register(&context);
    camel_zip_deflater::register(&context);
    context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .to("dataformat:zipDeflater:marshal?compressionLevel=9")
                    .to("dataformat:base64:marshal?lineLength=0")
                    .to("mock:encoded")
                    .to("dataformat:base64:unmarshal")
                    .to("dataformat:zipDeflater:unmarshal")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    context.start().await.unwrap();
    let result = MockEndpoint::resolve(context.as_ref(), "mock:result").unwrap();
    result.expected_bodies_received(["Hello World"]);
    context
        .create_producer_template()
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    result.assert_is_satisfied().await;
    let encoded = MockEndpoint::resolve(context.as_ref(), "mock:encoded").unwrap();
    let body = encoded.received_exchanges()[0].message().body().clone();
    let text = match body {
        Value::Bytes(b) => String::from_utf8(b).unwrap(),
        other => other.to_string(),
    };
    assert!(!text.contains('\n') && text.len() > 8, "{text}");

    let e = context.endpoint("dataformat:base64:foo").err().unwrap();
    assert!(
        e.to_string()
            .contains("Operation must be either marshal or unmarshal, was: foo"),
        "{e}"
    );
}
