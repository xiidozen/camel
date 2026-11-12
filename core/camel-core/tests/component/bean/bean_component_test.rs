//! Rust tests of the bean component and EIP: the `bean:` endpoint, methods without result,
//! processors as beans, method parameters and the `method()` expression.

use super::{bind, my_foo_bean, with_beans};
use camel_bean::{processor_value, MethodBean};
use camel_core::api::processor::processor_fn;
use camel_core::prelude::*;
use std::sync::Arc;

fn beans() -> Vec<(&'static str, Value)> {
    let audit = MethodBean::new("Audit")
        .void_method("audit", |inv| {
            inv.exchange_mut().message_mut().set_header("audited", true);
            Ok(())
        })
        .method("greet", |inv| {
            let name = inv.parameter_as::<String>(0)?.unwrap_or_default();
            let greeting = inv.parameter_as::<String>(1)?.unwrap_or("Hello".into());
            Ok(Value::from(format!("{greeting} {name}")))
        })
        .into_value();
    let upper = processor_value(Arc::new(processor_fn(|e: &mut Exchange| {
        let body = e.message().body().to_string().to_uppercase();
        e.message_mut().set_body(body);
        Ok(())
    })));
    vec![("foo", my_foo_bean()), ("audit", audit), ("upper", upper)]
}

#[tokio::test]
async fn test_bean_endpoint() {
    let t = with_beans(&beans(), |r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("bean:foo?method=hello")
                .to("mock:result"),
        );
        r.add(from("direct:only").to("bean:foo").to("mock:only"));
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.mock("mock:only")
        .expected_bodies_received(["Hello Camel"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.template.send_body("direct:only", "Camel").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    let reply = t.template.request_body("bean:foo", "Rust").await.unwrap();
    assert_eq!(Value::from("Hello Rust"), reply);
}

#[tokio::test]
async fn test_void_method_keeps_the_body_and_parameters() {
    let t = with_beans(&beans(), |r: &mut Routes| {
        r.add(
            from("direct:start")
                .bean_method("audit", "audit")
                .to("mock:audited")
                .bean_method("audit", "greet(${body}, 'Hi')")
                .to("mock:result"),
        );
    })
    .await;
    let audited = t.mock("mock:audited");
    audited.expected_bodies_received(["World"]);
    audited.expected_header_received("audited", true);
    t.mock("mock:result").expected_bodies_received(["Hi World"]);
    t.template.send_body("direct:start", "World").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_processor_as_bean_and_method_expression() {
    let t = with_beans(&beans(), |r: &mut Routes| {
        r.add(from("direct:start").bean("upper").to("mock:result"));
        r.add(
            from("direct:method")
                .set_header("greeting", method("audit", "greet(${body})"))
                .to("mock:method"),
        );
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["HELLO"]);
    t.mock("mock:method")
        .expected_header_received("greeting", "Hello Camel");
    t.template.send_body("direct:start", "Hello").await.unwrap();
    t.template
        .send_body("direct:method", "Camel")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_bean_without_method_name_and_consumer() {
    let t = bind(&beans());
    // audit has two methods: one must be named
    let e = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:start").bean("audit").to("mock:result"));
        })
        .await;
    assert!(e.is_ok(), "the method is chosen when called: {e:?}");
    t.context.start().await.unwrap();
    let err = t
        .template
        .request_body("direct:start", "x")
        .await
        .expect_err("ambiguous");
    // the template wraps the error in a CamelExecutionException
    let cause = crate::support::cause_of(&err);
    assert!(
        cause
            .to_string()
            .contains("the bean has the methods audit, greet: name the one to call"),
        "{cause}"
    );
    // a bean endpoint has no consumer
    let e = t
        .context
        .add_routes(|r: &mut Routes| {
            r.add(from("bean:foo").to("mock:never"));
        })
        .await;
    let e = match e {
        Err(e) => e,
        Ok(_) => t.context.start().await.expect_err("cannot consume"),
    };
    assert!(
        format!("{e:?}").contains("You cannot consume from a bean endpoint"),
        "{e:?}"
    );
    assert_eq!(
        "Bean[ref:foo method:hello]",
        from("direct:x").bean_method("foo", "hello").build().steps[0].to_string()
    );
}
