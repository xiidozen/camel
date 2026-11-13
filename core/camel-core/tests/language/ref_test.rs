//! Ports of `RefTest` and `RefPredicateTest` (`RefLanguageTest` tests the Java DSL builder of
//! the language).

use camel_core::api::context::CamelContext;
use camel_core::api::processor::{expression_fn, expression_value, predicate_fn, predicate_value};
use camel_core::prelude::*;
use std::sync::Arc;

fn context() -> Arc<DefaultCamelContext> {
    let c = camel_core::default_context();
    let r = c.registry().unwrap();
    r.bind(
        "myExp",
        expression_value(Arc::new(expression_fn(|_| Ok(Value::from("Hello World"))))),
    )
    .unwrap();
    r.bind(
        "myPredicate",
        predicate_value(Arc::new(predicate_fn(|e: &Exchange| {
            Ok(matches!(e.message().body(), Value::Text(t) if t == "Hello World"))
        }))),
    )
    .unwrap();
    c
}

fn exchange(c: &Arc<DefaultCamelContext>, body: &str) -> Exchange {
    let shared: Arc<dyn CamelContext> = c.clone();
    let mut e = Exchange::new(shared);
    e.message_mut().set_body(body);
    e
}

fn evaluate(c: &Arc<DefaultCamelContext>, e: &Exchange, text: &str) -> CamelResult<Value> {
    c.resolve_language("ref")?
        .create_expression(text)?
        .evaluate(e)
}

#[test]
fn test_ref_expressions() {
    let c = context();
    let mut e = exchange(&c, "Hello");
    assert_eq!(
        "Hello World",
        evaluate(&c, &e, "myExp").unwrap().to_string()
    );
    assert_eq!(
        Some("Cannot find expression or predicate in registry with ref: foo"),
        evaluate(&c, &e, "foo").unwrap_err().message()
    );
    // dynamic
    e.message_mut().set_header("foo", "myExp");
    assert_eq!(
        "Hello World",
        evaluate(&c, &e, "${header.foo}").unwrap().to_string()
    );
    e.message_mut().set_header("foo", "myExp2");
    assert_eq!(
        Some("Cannot find expression or predicate in registry with ref: myExp2"),
        evaluate(&c, &e, "${header.foo}").unwrap_err().message()
    );
    // as a predicate
    let p = c
        .resolve_language("ref")
        .unwrap()
        .create_predicate("myExp")
        .unwrap();
    assert!(p.matches(&e).unwrap());
}

#[test]
fn test_ref_predicates() {
    let c = context();
    for (body, expected) in [("Hello World", true), ("Bye World", false)] {
        let e = exchange(&c, body);
        assert_eq!(
            expected.to_string(),
            evaluate(&c, &e, "myPredicate").unwrap().to_string()
        );
        let p = c
            .resolve_language("ref")
            .unwrap()
            .create_predicate("myPredicate")
            .unwrap();
        assert_eq!(expected, p.matches(&e).unwrap());
    }
}
