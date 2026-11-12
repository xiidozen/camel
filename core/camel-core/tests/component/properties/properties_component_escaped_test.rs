//! Port of `PropertiesComponentEscapedTest`.

use super::{constant_text, resolve, start, with_location};
use camel_core::prelude::*;

#[tokio::test]
async fn test_escaped() {
    let t = with_location();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_body(constant_text(
                    "\\{{before\\}}{{cool.concat.escaped}}\\{{after\\}}",
                ))
                .set_header("foo", constant_text(
                    "Hello {{cool.concat.escaped}} and \\{{before\\}}{{cool.other.name}}\\{{after\\}}/\\\\{{cool.other.name}} how are you?",
                ))
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received([
        "{{before}}mock:{{cool.result}}{\"query\":{\"match_all\":{}}}{{after}}",
    ]);
    mock.expected_header_received(
        "foo",
        "Hello mock:{{cool.result}}{\"query\":{\"match_all\":{}}} and {{before}}Cheese{{after}}/\\Cheese how are you?",
    );
    t.template
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[test]
fn test_trailing_backslash_preserved() {
    let t = with_location();
    let r = |text: &str| resolve(&t, text).unwrap();
    assert_eq!("test\\n", r("{{cool.trailing.n}}"));
    assert_eq!("test\\t", r("{{cool.trailing.t}}"));
    assert_eq!("test\\", r("{{cool.trailing.backslash}}"));
    assert_eq!("test\\{", r("{{cool.trailing.open.brace}}"));
    assert_eq!("test\\}", r("{{cool.trailing.close.brace}}"));
    assert_eq!("\\", r("{{cool.lone.backslash}}"));
    assert_eq!("\\\\", r("{{cool.double.backslash}}"));
    assert_eq!("\\\\\\", r("{{cool.triple.backslash}}"));
    assert_eq!("hello\\nworld", r("{{cool.mid.n}}"));
}
