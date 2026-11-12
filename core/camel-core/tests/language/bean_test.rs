//! Port of `org.apache.camel.language.BeanTest`. The Java tests creating the bean from its
//! class (`testBeanTypeExpression`, `testBeanTypeAndMethodExpression`) are ported with a bean
//! instance: Rust has no class loading.

use super::simple_test::exchange;
use camel_bean::{BeanExpression, MethodBean};
use camel_core::prelude::*;

/// Java `MyBean`: `cheese(Exchange)`, `echo(String)`, `isFooHeaderAbc(@Header("foo") String)`.
fn my_bean() -> Value {
    MethodBean::new("org.apache.camel.language.BeanTest$MyBean")
        .method("cheese", |inv| {
            Ok(inv
                .exchange()
                .message()
                .header("foo")
                .cloned()
                .unwrap_or_default())
        })
        .method("echo", |inv| Ok(inv.parameter(0)))
        .method("isFooHeaderAbc", |inv| {
            Ok(Value::Bool(inv.header("foo") == Some(&Value::from("abc"))))
        })
        .into_value()
}

/// Java `MyUser`: `hello(String name)`.
fn my_user() -> Value {
    MethodBean::new("org.apache.camel.language.BeanTest$MyUser")
        .method("hello", |inv| {
            let name = inv.parameter_as::<String>(0)?.unwrap_or_default();
            Ok(Value::from(format!("Hello {name}")))
        })
        .into_value()
}

fn bean_exchange() -> (crate::support::ContextTestSupport, Exchange) {
    let (t, e) = exchange();
    let registry = t.context.registry().unwrap();
    registry.bind("foo", my_bean()).unwrap();
    registry.bind("my.company.MyClass", my_bean()).unwrap();
    (t, e)
}

fn assert_expression(e: &Exchange, text: &str, expected: &str) {
    let bean = e.context().resolve_language("bean").unwrap();
    let v = bean.create_expression(text).unwrap().evaluate(e).unwrap();
    assert_eq!(Value::from(expected), v, "Expression: {text}");
}

fn assert_predicate(e: &Exchange, text: &str) {
    let bean = e.context().resolve_language("bean").unwrap();
    let p = bean.create_predicate(text).unwrap();
    assert!(p.matches(e).unwrap(), "Predicate: {text}");
}

#[test]
fn test_simple_expressions() {
    let (_t, e) = bean_exchange();
    assert_expression(&e, "foo.echo('e::o')", "e::o");
    assert_expression(&e, "foo.echo('e.o')", "e.o");
    assert_expression(&e, "my.company.MyClass::echo('a')", "a");
    assert_expression(&e, "my.company.MyClass::echo('a.b')", "a.b");
    assert_expression(&e, "my.company.MyClass::echo('a::b')", "a::b");
    assert_expression(&e, "foo.cheese", "abc");
    assert_expression(&e, "foo?method=cheese", "abc");
    assert_expression(&e, "my.company.MyClass::cheese", "abc");
    assert_expression(&e, "foo?method=echo('e::o')", "e::o");
}

#[test]
fn test_predicates() {
    let (_t, e) = bean_exchange();
    assert_predicate(&e, "foo.isFooHeaderAbc");
    assert_predicate(&e, "foo?method=isFooHeaderAbc");
    assert_predicate(&e, "my.company.MyClass::isFooHeaderAbc");
}

#[test]
fn test_double_colon() {
    let (_t, e) = bean_exchange();
    assert_predicate(&e, "foo::isFooHeaderAbc");
    let bean = e.context().resolve_language("bean").unwrap();
    let p = bean.create_predicate("foo:isFooHeaderAbc").unwrap();
    let err = p.matches(&e).expect_err("Should throw exception");
    assert!(err.is(ErrorKind::NoSuchBean), "{err:?}");
    assert_eq!(Some("foo:isFooHeaderAbc"), err.detail());
}

#[test]
fn test_bean_instance_expression() {
    // Java testBeanTypeExpression, with an instance: the only method of the bean is called
    let (_t, mut e) = bean_exchange();
    e.message_mut().set_body("Claus");
    let exp = BeanExpression::of_bean(my_user(), None);
    exp.init(e.context()).unwrap();
    assert_eq!(Value::from("Hello Claus"), exp.evaluate(&e).unwrap());
}

#[test]
fn test_bean_instance_and_method_expression() {
    let (_t, mut e) = bean_exchange();
    e.message_mut().set_body("Claus");
    let exp = BeanExpression::of_bean(my_user(), Some("hello"));
    exp.init(e.context()).unwrap();
    assert_eq!(Value::from("Hello Claus"), exp.evaluate(&e).unwrap());
}

#[test]
fn test_no_method() {
    let (_t, e) = bean_exchange();
    let exp = BeanExpression::of_bean(my_user(), Some("unknown"));
    let err = exp.init(e.context()).expect_err("Should throw exception");
    assert!(err.is_subtype("MethodNotFoundException"), "{err:?}");
    assert_eq!(Some("unknown"), err.detail());
}

#[test]
fn test_no_method_bean_lookup() {
    let (_t, e) = bean_exchange();
    let exp = BeanExpression::new("foo", Some("cake"));
    let err = exp.init(e.context()).expect_err("Should throw exception");
    assert!(err.is_subtype("MethodNotFoundException"), "{err:?}");
    assert_eq!(Some("cake"), err.detail());
}

// Rust tests of the bean language

#[test]
fn test_ognl_on_the_result_and_errors() {
    let (_t, e) = bean_exchange();
    let bean = e.context().resolve_language("bean").unwrap();
    // OGNL on the result of the method
    let exp = bean
        .create_expression("foo.echo('Camel').length()")
        .unwrap();
    assert_eq!(Value::Int(5), exp.evaluate(&e).unwrap());
    // more than one method and no method name
    let exp = bean.create_expression("foo").unwrap();
    let err = exp.evaluate(&e).err().unwrap();
    assert!(err.is_subtype("AmbiguousMethodCallException"), "{err}");
    // a plain value in the registry: OGNL on it
    e.context()
        .registry()
        .unwrap()
        .bind("text", Value::from("Hello"))
        .unwrap();
    let exp = bean.create_expression("text.toUpperCase()").unwrap();
    assert_eq!(Value::from("HELLO"), exp.evaluate(&e).unwrap());
    assert!(bean.create_expression("type:com.foo.MyBean").is_err());
    assert_eq!(
        "BeanExpression[foo method:cheese]",
        bean.create_expression("foo.cheese").unwrap().describe()
    );
}

#[test]
fn test_headers_and_properties_are_copied_back() {
    let (_t, mut e) = bean_exchange();
    let changer = MethodBean::new("Changer")
        .method("change", |inv| {
            inv.exchange_mut().message_mut().set_header("changed", true);
            inv.exchange_mut().set_property("p", 1);
            Ok(Value::from("done"))
        })
        .into_value();
    e.context()
        .registry()
        .unwrap()
        .bind("changer", changer)
        .unwrap();
    let bean = e.context().resolve_language("bean").unwrap();
    let exp = bean.create_expression("changer.change").unwrap();
    // evaluate does not change the exchange
    assert_eq!(Value::from("done"), exp.evaluate(&e).unwrap());
    assert!(e.message().header("changed").is_none());
    // evaluate_mut copies the headers and properties back, as Java
    assert_eq!(Value::from("done"), exp.evaluate_mut(&mut e).unwrap());
    assert_eq!(Some(&Value::Bool(true)), e.message().header("changed"));
    assert_eq!(Some(&Value::Int(1)), e.property("p"));
}
