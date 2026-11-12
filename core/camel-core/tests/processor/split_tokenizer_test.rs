//! Ports of `SplitTokenizerTest` (not its xpath case), `SplitTokenizerGroupTest`,
//! `SplitTokenizerGroupDynamicTest`, `SplitGroupSkipFirstTest`, `SplitGroupMultiLinesTest`
//! and `SplitTokenizerRegexpGroupTest`.

use crate::support::ContextTestSupport;
use camel_core::model::expression::ExpressionDefinition;
use camel_core::prelude::*;

async fn split(expression: impl Into<ExpressionDefinition>, streaming: bool) -> ContextTestSupport {
    let expression = expression.into();
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        let route = from("direct:start").split(expression.clone(), |s| s.to("mock:split"));
        r.add(if streaming { route.streaming() } else { route });
    })
    .await
}

async fn assert_split(
    t: &ContextTestSupport,
    body: impl Into<Value>,
    headers: &[(&str, Value)],
    expected: &[&str],
) {
    let mock = t.mock("mock:split");
    mock.reset();
    mock.expected_bodies_received(expected.iter().copied());
    let h: Vec<(String, Value)> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    t.template
        .send_body_and_headers("direct:start", body, h)
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_split_tokenizer() {
    let names = ["Claus", "James", "Willem"];
    let t = split(tokenize(","), false).await;
    assert_split(&t, "Claus,James,Willem", &[], &names).await;
    let t = split(tokenize(",").source("header:myHeader"), false).await;
    assert_split(
        &t,
        "Hello World",
        &[("myHeader", "Claus,James,Willem".into())],
        &names,
    )
    .await;
    let t = split(tokenize_regex(r"(\W+)\s*"), false).await;
    assert_split(&t, "Claus James Willem", &[], &names).await;
    let t = split(tokenize_pair("[", "]", true), false).await;
    assert_split(
        &t,
        "[Claus][James][Willem]",
        &[],
        &["[Claus]", "[James]", "[Willem]"],
    )
    .await;
    let t = split(tokenize_xml("person", None), false).await;
    assert_split(
        &t,
        "<persons><person>Claus</person><person>James</person><person>Willem</person></persons>",
        &[],
        &[
            "<person>Claus</person>",
            "<person>James</person>",
            "<person>Willem</person>",
        ],
    )
    .await;
    assert_split(
        &t,
        "<persons><person attr='/' /></persons>",
        &[],
        &["<person attr='/' />"],
    )
    .await;
}

#[tokio::test]
async fn test_split_tokenizer_group() {
    let t = split(tokenize(",").group("2"), false).await;
    assert_split(&t, "Claus,James,Willem", &[], &["Claus,James", "Willem"]).await;
    let t = split(tokenize(",").group("2").skip_first(), false).await;
    assert_split(&t, b"Claus,James,Willem".to_vec(), &[], &["James,Willem"]).await;
    // a dynamic group
    let t = split(tokenize(",").group("${header.groups}"), false).await;
    assert_split(
        &t,
        "Claus,James,Willem",
        &[("groups", 2.into())],
        &["Claus,James", "Willem"],
    )
    .await;
    let t = split(tokenize(",").group("${header.groups}").skip_first(), false).await;
    assert_split(
        &t,
        b"Claus,James,Willem".to_vec(),
        &[("groups", 2.into())],
        &["James,Willem"],
    )
    .await;
}

#[tokio::test]
async fn test_split_group_lines() {
    let t = split(tokenize("\n").group("3").skip_first(), true).await;
    assert_split(
        &t,
        "##comment\nABC\nDEF\nGHI\nJKL\nMN",
        &[],
        &["ABC\nDEF\nGHI", "JKL\nMN"],
    )
    .await;
    assert_split(&t, "##comment\n", &[], &[""]).await;
    let t = split(tokenize("\n").group("3"), true).await;
    assert_split(
        &t,
        "ABC\nDEF\nGHI\nJKL\nMN",
        &[],
        &["ABC\nDEF\nGHI", "JKL\nMN"],
    )
    .await;
    let t = split(
        tokenize_regex("\r\n|\n")
            .group("2")
            .group_delimiter("\n")
            .skip_first(),
        false,
    )
    .await;
    assert_split(
        &t,
        "Header\nClaus\r\nJames\nWillem\r\nJon\nAndrea",
        &[],
        &["Claus\nJames", "Willem\nJon", "Andrea"],
    )
    .await;
}
