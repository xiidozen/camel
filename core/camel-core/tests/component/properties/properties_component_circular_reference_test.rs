//! Port of `PropertiesComponentCircularReferenceTest` (the timer endpoint case with a direct
//! endpoint, timer is not ported yet).

use super::resolve;
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

fn assert_circular_reference(t: &ContextTestSupport, text: &str, key: &str) {
    let e = t
        .context
        .resolve_property_placeholders(text)
        .expect_err("a circular reference");
    let message = e.message().unwrap_or_default().to_string();
    assert!(
        message.starts_with(&format!("Circular reference detected with key [{key}]")),
        "{message}"
    );
}

#[test]
fn test_required_circular_reference() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("loop1", "{{loop2}}");
    t.context
        .properties()
        .add_initial_property("loop2", "{{loop1}}");
    assert_circular_reference(&t, "{{loop1}}", "loop1");
}

#[test]
fn test_optional_self_reference() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("self", "{{?self}}");
    assert_circular_reference(&t, "{{self}}", "?self");
    assert_circular_reference(&t, "{{?self}}", "?self");
}

#[test]
fn test_optional_mutual_reference() {
    let t = ContextTestSupport::new();
    t.context.properties().add_initial_property("a", "{{?b}}");
    t.context.properties().add_initial_property("b", "{{?a}}");
    assert_circular_reference(&t, "x{{a}}y", "?b");
}

#[test]
fn test_optional_self_reference_with_default_value() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("timeout", "{{?timeout:5000}}");
    assert_circular_reference(&t, "{{timeout}}", "?timeout:5000");
}

#[tokio::test]
async fn test_optional_self_reference_in_endpoint_uri() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("timeout", "{{?timeout:5000}}");
    let result = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").to("mock:result?resultWaitTime={{timeout}}"));
        })
        .await;
    let e = match result {
        Err(e) => e,
        Ok(_) => t.context.start().await.expect_err("Should fail"),
    };
    assert!(
        format!("{e:?}").contains("Circular reference detected with key [?timeout:5000]"),
        "{e:?}"
    );
}

#[test]
fn test_optional_without_circular_reference() {
    let t = ContextTestSupport::new();
    let pc = t.context.properties();
    pc.add_initial_property("a", "{{b}}-{{?c}}");
    pc.add_initial_property("b", "B");
    pc.add_initial_property("timeout", "{{?override.timeout:5000}}");
    pc.add_initial_property("delay", "{{?override.delay}}");
    let r = |text: &str| resolve(&t, text);
    assert_eq!(Some("B-".to_string()), r("{{a}}"));
    // the same key twice is not a circular reference
    assert_eq!(Some("B-B-".to_string()), r("{{a}}{{a}}"));
    assert_eq!(Some("xy".to_string()), r("x{{?nope}}y"));
    assert_eq!(None, r("{{?nope}}"));
    assert_eq!(Some("5000".to_string()), r("{{?nope:5000}}"));
    assert_eq!(Some("5000".to_string()), r("{{timeout}}"));
    assert_eq!(Some("x-y".to_string()), r("x-{{delay}}y"));
}

#[tokio::test]
async fn test_optional_without_circular_reference_in_endpoint_uri() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .add_initial_property("timeout", "{{?override.timeout:5000}}");
    super::start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:result?retainFirst={{?maxKeep}}&resultWaitTime={{timeout}}"),
        );
    })
    .await;
    assert!(
        t.context
            .has_endpoint("mock:result?resultWaitTime=5000")
            .is_some(),
        "Endpoints: {:?}",
        t.context
            .endpoints()
            .iter()
            .map(|e| e.endpoint_uri().to_string())
            .collect::<Vec<_>>()
    );
}
