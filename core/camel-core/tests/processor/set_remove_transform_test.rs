//! Ports of `TransformProcessorTest`, `TransformProcessorWithHeaderTest`, `TransformViaDSLTest`,
//! `TransformTest`, `SetPropertyTest`, `SetVariableTest`, `RemoveHeaderTest`,
//! `RemovePropertyTest`, `RemoveVariableTest`, `RemoveHeadersExcludeTest`,
//! `RemovePropertiesWithoutExclusionTest`, `RemovePropertiesWithExclusionTest`,
//! `RemoveHeadersAllTest`, `RemoveHeadersTest` and the `SetBodyProcessorTest` case with a
//! header (the other uses a Java message subclass).

use crate::support::ContextTestSupport;
use camel_core::prelude::*;

/// A case: the uri, the headers sent, the headers expected, the number of headers left.
type Case = (
    &'static str,
    Vec<(String, Value)>,
    Vec<(&'static str, &'static str)>,
    usize,
);

fn headers(pairs: &[(&str, &str)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::from(*v)))
        .collect()
}

#[tokio::test]
async fn test_transform() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:a")
                .transform(body().append(" World!"))
                .to("mock:result"),
        );
        r.add(
            from("direct:b")
                .set_body(body().append(" World!"))
                .to("mock:result"),
        );
        r.add(
            from("direct:c")
                .process_fn(|e| {
                    let b = format!("{} World!", e.message().body());
                    e.message_mut().set_body(b);
                    Ok(())
                })
                .to("mock:result"),
        );
        r.add(
            from("direct:in")
                .transform(constant("London"))
                .to("mock:result"),
        );
    })
    .await;
    for uri in ["direct:a", "direct:b", "direct:c"] {
        let m = t.mock("mock:result");
        m.reset();
        m.expected_bodies_received(["Hello World!"]);
        t.template.send_body(uri, "Hello").await.unwrap();
        m.assert_is_satisfied().await;
    }
    let m = t.mock("mock:result");
    m.reset();
    m.expected_bodies_received(["London"]);
    m.expected_header_received("foo", "bar");
    t.template
        .send_body_and_header(
            "direct:in",
            "<person name='James' city='London'/>",
            "foo",
            "bar",
        )
        .await
        .unwrap();
    m.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_set_and_remove() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:property")
                .set_property("foo", constant("bar"))
                .to("mock:end"),
        );
        r.add(
            from("direct:variable")
                .set_variable("foo", constant("bar"))
                .to("mock:end"),
        );
        r.add(
            from("direct:header-remove")
                .set_header("foo", constant("bar"))
                .to("mock:mid")
                .remove_header("foo")
                .to("mock:end"),
        );
        r.add(
            from("direct:property-remove")
                .set_property("foo", constant("bar"))
                .to("mock:mid")
                .remove_property("foo")
                .to("mock:end"),
        );
        r.add(
            from("direct:variable-remove")
                .set_variable("foo", constant("bar"))
                .to("mock:mid")
                .remove_variable("foo")
                .to("mock:end"),
        );
    })
    .await;
    for uri in ["direct:property", "direct:variable"] {
        let end = t.mock("mock:end");
        end.reset();
        end.expected_message_count(1);
        t.template.send_body(uri, "<blah/>").await.unwrap();
        end.assert_is_satisfied().await;
        let e = &end.received_exchanges()[0];
        let v = if uri == "direct:property" {
            e.property("foo")
        } else {
            e.variable("foo")
        };
        assert_eq!(Some(&Value::from("bar")), v);
    }
    for uri in [
        "direct:header-remove",
        "direct:property-remove",
        "direct:variable-remove",
    ] {
        let (mid, end) = (t.mock("mock:mid"), t.mock("mock:end"));
        mid.reset();
        end.reset();
        mid.expected_message_count(1);
        end.expected_message_count(1);
        t.template.send_body(uri, "<blah/>").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
        let get = |e: &Exchange| -> Option<Value> {
            match uri {
                "direct:header-remove" => e.message().header("foo").cloned(),
                "direct:property-remove" => e.property("foo").cloned(),
                _ => e.variable("foo").cloned(),
            }
        };
        assert_eq!(Some(Value::from("bar")), get(&mid.received_exchanges()[0]));
        assert_eq!(None, get(&end.received_exchanges()[0]));
    }
}

#[tokio::test]
async fn test_remove_headers() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:exclude")
                .remove_headers("dude*")
                .remove_headers_excluding("Beer*", &[".*Heineken.*", ".*Tuborg.*"])
                .remove_headers("foo")
                .to("mock:end"),
        );
        r.add(
            from("direct:regex")
                .remove_headers("dude*")
                .remove_headers("Beer(Carlsberg|Tuborg)")
                .remove_headers("foo")
                .to("mock:end"),
        );
        r.add(from("direct:all").remove_headers("*").to("mock:end"));
        r.add(
            from("direct:allExclude")
                .remove_headers_excluding("*", &["duck"])
                .to("mock:end"),
        );
        r.add(
            from("direct:allExcludeNoMatch")
                .remove_headers_excluding("*", &["dog"])
                .to("mock:end"),
        );
    })
    .await;
    let cases: Vec<Case> = vec![
        (
            "direct:exclude",
            headers(&[
                ("dudeCool", "cool"),
                ("dudeWicket", "wicket"),
                ("duck", "Donald"),
                ("foo", "bar"),
            ]),
            vec![("duck", "Donald")],
            1,
        ),
        (
            "direct:exclude",
            headers(&[
                ("dudeCool", "cool"),
                ("dudeWicket", "wicket"),
                ("duck", "Donald"),
                ("BeerCarlsberg", "Great"),
                ("BeerTuborg", "Also Great"),
                ("BeerHeineken", "Good"),
            ]),
            vec![
                ("duck", "Donald"),
                ("BeerHeineken", "Good"),
                ("BeerTuborg", "Also Great"),
            ],
            3,
        ),
        (
            "direct:regex",
            headers(&[
                ("dudeCool", "cool"),
                ("DudeWicket", "wicket"),
                ("DUDEbig", "upper"),
                ("duck", "Donald"),
                ("foo", "bar"),
            ]),
            vec![("duck", "Donald")],
            1,
        ),
        (
            "direct:regex",
            headers(&[
                ("dudeCool", "cool"),
                ("DudeWicket", "wicket"),
                ("duck", "Donald"),
                ("DUDEbig", "upper"),
                ("BeerCarlsberg", "Great"),
                ("BeerTuborg", "Also Great"),
                ("BeerHeineken", "Good"),
                ("foo", "bar"),
            ]),
            vec![("duck", "Donald"), ("BeerHeineken", "Good")],
            2,
        ),
        (
            "direct:regex",
            headers(&[
                ("dudeCool", "cool"),
                ("DudeWicket", "wicket"),
                ("DUDEbig", "upper"),
                ("duck", "Donald"),
                ("Beercarlsberg", "Great"),
                ("BeerTUBORG", "Also Great"),
                ("BeerHeineken", "Good"),
                ("FOO", "bar"),
            ]),
            vec![("duck", "Donald"), ("BeerHeineken", "Good")],
            2,
        ),
    ];
    let all = headers(&[
        ("dudeCool", "cool"),
        ("DudeWicket", "wicket"),
        ("DUDEbig", "upper"),
        ("duck", "Donald"),
        ("foo", "bar"),
    ]);
    let mut cases = cases;
    cases.push(("direct:all", all.clone(), vec![], 0));
    cases.push((
        "direct:allExclude",
        all.clone(),
        vec![("duck", "Donald")],
        1,
    ));
    cases.push(("direct:allExcludeNoMatch", all, vec![], 0));
    for (uri, sent, expected, size) in cases {
        let m = t.mock("mock:end");
        m.reset();
        m.expected_bodies_received(["Hello World"]);
        for (k, v) in &expected {
            m.expected_header_received(k, *v);
        }
        t.template
            .send_body_and_headers(uri, "Hello World", sent)
            .await
            .unwrap();
        m.assert_is_satisfied().await;
        assert_eq!(
            size,
            m.received_exchanges()[0].message().headers().len(),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn test_remove_properties() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:without")
                .set_property("foo", constant("bar"))
                .set_property("fee", constant("bar1"))
                .to("mock:mid")
                .remove_properties("f*")
                .to("mock:end"),
        );
        r.add(
            from("direct:with")
                .set_property("foo", constant("bar"))
                .set_property("fee", constant("bar1"))
                .set_property("fiu", constant("bar2"))
                .to("mock:mid")
                .remove_properties_excluding("f*", &["fiu"])
                .to("mock:end"),
        );
    })
    .await;
    for uri in ["direct:without", "direct:with"] {
        let (mid, end) = (t.mock("mock:mid"), t.mock("mock:end"));
        mid.reset();
        end.reset();
        mid.expected_message_count(1);
        end.expected_message_count(1);
        t.template.send_body(uri, "message").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
        let m = &mid.received_exchanges()[0];
        assert_eq!(Some(&Value::from("bar")), m.property("foo"));
        assert_eq!(Some(&Value::from("bar1")), m.property("fee"));
        let e = &end.received_exchanges()[0];
        assert_eq!(None, e.property("foo"));
        assert_eq!(None, e.property("fee"));
        if uri == "direct:with" {
            assert_eq!(Some(&Value::from("bar2")), e.property("fiu"));
        }
    }
}

#[tokio::test]
async fn test_set_body_with_header() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start2")
                .set_body(simple("${header.text.replace('a','b')}"))
                .to("mock:test"),
        );
    })
    .await;
    t.mock("mock:test").expected_bodies_received(["bbb"]);
    t.mock("mock:test").expected_header_received("text", "aab");
    t.template
        .send_body_and_header("direct:start2", "aab", "text", "aab")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
