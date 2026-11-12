//! Ports of `SimpleIncHeaderTest`, `SimpleDecHeaderTest`, `SimpleParserRegexpPredicateTest`
//! and `SimpleWhiteSpaceTest`.

use crate::simple_test::{assert_predicate, exchange};
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_inc_and_dec_header() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:inc")
                .set_header("myCounter", simple("${header.myCounter}++"))
                .to("mock:inc"),
        );
        r.add(
            from("direct:dec")
                .set_header("myCounter", simple("${header.myCounter}--"))
                .to("mock:dec"),
        );
    })
    .await;
    for (uri, mock, expected) in [
        ("direct:inc", "mock:inc", 124),
        ("direct:dec", "mock:dec", 122),
    ] {
        let m = t.mock(mock);
        m.expected_message_count(1);
        m.expected_header_received("myCounter", expected);
        t.template
            .send_body_and_header(uri, "Hello World", "myCounter", 123)
            .await
            .unwrap();
        m.assert_is_satisfied().await;
        let received = m.received_exchanges();
        assert!(matches!(
            received[0].message().header("myCounter"),
            Some(Value::Int(_))
        ));
    }
}

#[test]
fn test_simple_regexp() {
    let (_t, mut e) = exchange();
    let p = r"${body} regex '^\d{2}\.\d{2}\.\d{4}$'";
    e.message_mut().set_body("12.34.5678");
    assert_predicate(&e, p, true);
    e.message_mut().set_body("12.2a.22ab");
    assert_predicate(&e, p, false);
}

#[test]
fn test_white_space() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("some text");
    for sep in [" ", "\t", "\n", "\r\n", "\r"] {
        assert_predicate(
            &e,
            &format!("${{in.body}} contains 'some' &&{sep}${{in.body}} contains 'text'"),
            true,
        );
    }
    e.message_mut().set_body("\tsome\nmulti\rline\r\ntext");
    assert_predicate(&e, "${in.body} == '\tsome\nmulti\rline\r\ntext'", true);
}
