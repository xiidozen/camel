//! Ports of `SimpleCustomFunctionTest`, `SimpleResourceTest`,
//! `SimpleResourceWithoutSchemeTest` and `SimpleResourceDynamicTest`.

use crate::support::ContextTestSupport;
use camel_core::api::context::context_plugin;
use camel_core::api::spi::simple_function::{SimpleFunction, SimpleFunctionRegistry};
use camel_core::prelude::*;
use std::sync::Arc;

struct FooSimpleFunction;

impl SimpleFunction for FooSimpleFunction {
    fn name(&self) -> &str {
        "foo"
    }

    fn apply(&self, _exchange: &Exchange, input: Value) -> CamelResult<Value> {
        Ok(Value::from(format!("I was here {input}")))
    }
}

async fn custom_function_context() -> ContextTestSupport {
    let t = ContextTestSupport::new();
    let reg = context_plugin::<SimpleFunctionRegistry>(t.context.as_ref()).unwrap();
    reg.add_function(Arc::new(FooSimpleFunction));
    let bar = t
        .context
        .resolve_language("simple")
        .unwrap()
        .create_expression("${trim()} ~> ${normalizeWhitespace()} ~> ${capitalize()} ~> ${quote()}")
        .unwrap();
    reg.add_expression_function("bar", bar);
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(
                from("direct:start")
                    .set_body(simple("Hello ${function(foo)}"))
                    .to("mock:result"),
            );
            r.add(
                from("direct:start2")
                    .set_body(simple("Hello ${foo}"))
                    .to("mock:result"),
            );
            r.add(
                from("direct:start3")
                    .set_variable("msg", constant("Moon"))
                    .set_body(simple("Bye ${function(foo,${variable.msg})}"))
                    .to("mock:result"),
            );
            r.add(
                from("direct:start3b")
                    .set_variable("msg", constant("Pluto"))
                    .set_body(simple("Bye ${variable.msg} ~> ${foo}"))
                    .to("mock:result"),
            );
            r.add(
                from("direct:start4")
                    .set_body(simple("${bar}"))
                    .to("mock:result"),
            );
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t
}

#[tokio::test]
async fn test_custom_functions() {
    let t = custom_function_context().await;
    let reg = context_plugin::<SimpleFunctionRegistry>(t.context.as_ref()).unwrap();
    assert_eq!(2, reg.custom_size());
    for (uri, body, expected) in [
        ("direct:start", "World", "Hello I was here World"),
        ("direct:start2", "Earth", "Hello I was here Earth"),
        ("direct:start3", "World", "Bye I was here Moon"),
        ("direct:start3b", "World", "Bye I was here Pluto"),
        (
            "direct:start4",
            "  hello  world i was here   ",
            "\"Hello World I Was Here\"",
        ),
    ] {
        let mock = t.mock("mock:result");
        mock.reset();
        mock.expected_bodies_received([expected]);
        t.template.send_body(uri, body).await.unwrap();
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_unknown_function_stays_a_syntax_error() {
    let t = ContextTestSupport::new();
    let e = t
        .context
        .resolve_language("simple")
        .unwrap()
        .create_expression("${nope}")
        .err()
        .expect("unknown function");
    assert!(e.is(ErrorKind::ExpressionIllegalSyntax), "{e}");
}

#[tokio::test]
async fn test_simple_resource() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .transform(simple("resource:classpath:mysimple.txt"))
                .to("mock:result"),
        );
        r.add(
            from("direct:dynamic")
                .transform(simple("resource:classpath:${header.myFileName}"))
                .to("mock:result"),
        );
        r.add(
            from("direct:plain")
                .set_body(simple("resource:templates/${header.type}.json"))
                .to("mock:plain"),
        );
        r.add(
            from("direct:named")
                .set_body(simple("resource:classpath:${header.name}.txt"))
                .to("mock:result"),
        );
    })
    .await;
    let mock = t.mock("mock:result");
    mock.expected_bodies_received(["The name is Camel", "The name is Camel", "The name is x"]);
    t.template.send_body("direct:start", "Camel").await.unwrap();
    t.template
        .send_body_and_header("direct:dynamic", "Camel", "myFileName", "mysimple.txt")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:named", "x", "name", "mysimple")
        .await
        .unwrap();
    mock.assert_is_satisfied().await;
    // without a scheme it is plain text
    let plain = t.mock("mock:plain");
    plain.expected_bodies_received(["resource:templates/order.json"]);
    t.template
        .send_body_and_header("direct:plain", "x", "type", "order")
        .await
        .unwrap();
    plain.assert_is_satisfied().await;
}
