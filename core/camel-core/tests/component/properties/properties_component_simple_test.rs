//! Port of `PropertiesComponentSetHeaderSimpleTest`, `PropertiesComponentEIPChoiceSimpleTest`
//! and `PropertiesComponentConcatenatePropertiesTest`, plus Rust tests of
//! `${properties:..}`, `${propertiesExist:..}` and `${ref:..}`.

use super::{start, with_location};
use crate::support::ContextTestSupport;
use camel_core::prelude::*;

#[tokio::test]
async fn test_properties_and_simple() {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .set_locations(&["classpath:org/apache/camel/component/properties/cheese.properties"])
        .unwrap();
    start(&t, |r: &mut Routes| {
        r.add(
            from("direct:start")
                .set_header("foo", simple("{{cheese.server}}/${header.app}"))
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_header_received("foo", "http://mycoolserver/myapp");
    t.template
        .send_body_and_header("direct:start", "Hello World", "app", "myapp")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_choice() {
    let t = with_location();
    start(&t, |r: &mut Routes| {
        r.add(from("direct:start").choice(|c| {
            c.when(simple("${body} contains '{{cool.name}}'"), |w| {
                w.to("mock:camel")
            })
            .otherwise(|o| o.to("mock:other"))
        }));
    })
    .await;
    t.mock("mock:camel")
        .expected_bodies_received(["Hello Camel", "Hi Camel"]);
    t.mock("mock:other").expected_bodies_received(["Bye World"]);
    for text in ["Hello {{cool.name}}", "Hi {{cool.name}}"] {
        let body = super::resolve(&t, text).unwrap();
        t.template.send_body("direct:start", body).await.unwrap();
    }
    t.template
        .send_body("direct:start", "Bye World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_concat_properties() {
    for key in ["{{concat.property}}", "{{property.complete}}"] {
        let t = ContextTestSupport::new();
        t.context
            .properties()
            .set_locations(&[
                "classpath:org/apache/camel/component/properties/concatenation.properties",
            ])
            .unwrap();
        t.context.properties().set_nested_placeholder(true);
        let key = key.to_string();
        start(&t, move |r: &mut Routes| {
            r.add(
                from("direct:start")
                    .set_body(simple(&key))
                    .to("mock:result"),
            );
        })
        .await;
        t.mock("mock:result")
            .expected_bodies_received(["file:dirname"]);
        t.template.send_body("direct:start", "Test").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[test]
fn test_simple_properties_functions() {
    let t = with_location();
    t.context
        .registry()
        .unwrap()
        .bind("myBean", Value::from("Hello Bean"))
        .unwrap();
    let mut e = Exchange::new(t.context.as_shared());
    e.message_mut().set_header("key", "cool.name");
    let simple = t.context.resolve_language("simple").unwrap();
    let eval = |text: &str| {
        simple
            .create_expression(text)
            .unwrap()
            .evaluate(&e)
            .unwrap()
    };
    assert_eq!(Value::from("Camel"), eval("${properties:cool.name}"));
    assert_eq!(Value::from("Camel"), eval("${properties:${header.key}}"));
    assert_eq!(Value::from("Beer"), eval("${properties:cool.unknown:Beer}"));
    assert_eq!(Value::Bool(true), eval("${propertiesExist:cool.name}"));
    assert_eq!(Value::Bool(false), eval("${propertiesExist:cool.unknown}"));
    assert_eq!(Value::Bool(true), eval("${propertiesExist:!cool.unknown}"));
    assert_eq!(Value::from("Hello Bean"), eval("${ref:myBean}"));
    assert_eq!(Value::Null, eval("${ref:noBean}"));
    let err = simple
        .create_expression("${properties:cool.unknown}")
        .unwrap()
        .evaluate(&e)
        .expect_err("missing property");
    assert!(
        format!("{err:?}").contains("Property with key [cool.unknown] not found"),
        "{err:?}"
    );
    // the boolean function uses the simple language
    let pc = t.context.properties();
    use camel_core::api::spi::properties::PropertiesComponent;
    assert_eq!(
        Some("true".to_string()),
        pc.parse_uri("{{boolean:myQueueSize > 5}}").unwrap()
    );
    assert_eq!(
        Some("false".to_string()),
        pc.parse_uri("{{boolean:myQueueSize > 50}}").unwrap()
    );
}
