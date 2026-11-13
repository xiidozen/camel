//! Ports of the language component tests (`LanguageRouteTest`, `LanguageRouteNoTransformTest`,
//! `LanguageHeaderRouteTest`, `LanguageRouteConvertBodyTest`, `LanguageScriptRouteTest`,
//! `LanguageScriptInHeaderRouteTest`, `LanguageScriptInHeaderRouteTakePrecedenceTest`,
//! `LanguageCacheScriptTest`, `LanguageNoCacheScriptTest`, `LanguageLoadScriptFromClasspathTest`,
//! `LanguageLoadScriptFromClasspathHeaderTest`, `LanguageResourceLoadScriptFromClasspathTest`,
//! `LanguageResourceLoadConstantFromClasspathTest` and `LanguageLoadScriptFromFileUpdateTest`).

use crate::support::{ContextTestSupport, TestDirectory};
use camel_core::api::exchange_constants::{FILE_NAME, LANGUAGE_SCRIPT};
use camel_core::prelude::*;
use camel_language::LanguageEndpoint;
use std::sync::Arc;

/// `URLEncoder.encode("Hello ${body}")`.
const HELLO: &str = "Hello+%24%7Bbody%7D";

/// Sends the body (with the header if any) to `direct:start` routed to the uri, expecting the
/// bodies on `mock:result`.
async fn run(uri: &str, sends: &[(&str, Option<&str>)], expected: &[&str]) -> ContextTestSupport {
    let uri = uri.to_string();
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:start").to(&uri).to("mock:result"));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(expected.to_vec());
    for (body, script) in sends {
        match script {
            Some(s) => t
                .template
                .send_body_and_header("direct:start", *body, LANGUAGE_SCRIPT, *s)
                .await
                .unwrap(),
            None => t.template.send_body("direct:start", *body).await.unwrap(),
        };
    }
    t.assert_mock_endpoints_satisfied().await;
    t
}

#[tokio::test]
async fn test_language_route() {
    run(
        &format!("language:simple:{HELLO}"),
        &[("World", None)],
        &["Hello World"],
    )
    .await;
    run(
        &format!("language:simple:{HELLO}?transform=false"),
        &[("World", None)],
        &["World"],
    )
    .await;
    run(
        "language:simple:%24%7BmandatoryBodyAs%28String%29%7D",
        &[("123;Camel in Action", None)],
        &["123;Camel in Action"],
    )
    .await;
}

#[tokio::test]
async fn test_language_header() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("language:header:foo")
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body_and_header("direct:start", "Hi", "foo", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_language_script_from_registry() {
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("myScript", Value::from("Hello ${body}"))
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .to("language:simple?script=#myScript")
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_language_script_in_header() {
    run(
        "language:simple?allowTemplateFromHeader=true",
        &[("World", Some("Hello ${body}"))],
        &["Hello World"],
    )
    .await;
    run(
        "language:simple?allowTemplateFromHeader=true",
        &[(
            "World",
            Some("classpath:org/apache/camel/component/language/mysimplescript.txt"),
        )],
        &["Hello World"],
    )
    .await;
    // the header takes precedence
    let uri = "language:simple:Bye+%24%7Bbody%7D?allowTemplateFromHeader=true";
    run(uri, &[("World", Some("Hello ${body}"))], &["Hello World"]).await;
    run(uri, &[("World", None)], &["Bye World"]).await;
}

#[tokio::test]
async fn test_language_cache_script() {
    for cache in [true, false] {
        let uri = format!("language:simple:{HELLO}?transform=false&cacheScript={cache}");
        let t = run(&uri, &[], &[]).await;
        let endpoint = t.context.endpoint(&uri).unwrap();
        let mock = t.mock("mock:result");
        mock.reset();
        mock.expected_bodies_received(["World", "Camel"]);
        t.template.send_body("direct:start", "World").await.unwrap();
        let le = endpoint
            .as_any()
            .downcast_ref::<LanguageEndpoint>()
            .unwrap();
        let first = le.expression();
        t.template.send_body("direct:start", "Camel").await.unwrap();
        let second = le.expression();
        mock.assert_is_satisfied().await;
        if cache {
            assert!(Arc::ptr_eq(
                first.as_ref().unwrap(),
                second.as_ref().unwrap()
            ));
        } else {
            assert!(first.is_none() && second.is_none());
        }
    }
}

#[tokio::test]
async fn test_language_load_script_from_classpath() {
    let script = "org/apache/camel/component/language/mysimplescript.txt";
    for uri in [
        format!("language:simple:classpath:{script}"),
        format!("language:simple:resource:classpath:{script}"),
    ] {
        run(&uri, &[("World", None)], &["Hello World"]).await;
    }
    run(
        "language:constant:resource:classpath:org/apache/camel/component/language/hello.txt",
        &[("World", None)],
        &["Hello this is some text."],
    )
    .await;
}

#[tokio::test]
async fn test_language_load_script_from_file_update() {
    let dir = TestDirectory::new("language-file-update");
    let uri = format!(
        "language:simple:{}",
        dir.file_uri("myscript.txt?contentCache=false")
    );
    let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(from("direct:start").to(&uri).to("mock:result"));
    })
    .await;
    t.template
        .send_body_and_header(
            &dir.file_uri(""),
            "Hello ${body}",
            FILE_NAME,
            "myscript.txt",
        )
        .await
        .unwrap();
    t.mock("mock:result")
        .expected_bodies_received(["Hello World", "Bye World"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.template
        .send_body_and_header(&dir.file_uri(""), "Bye ${body}", FILE_NAME, "myscript.txt")
        .await
        .unwrap();
    t.template.send_body("direct:start", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
