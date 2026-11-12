//! Port of `functions/BeanFunctionFactoryTest`: `${bean:..}`.

use super::simple_test::{evaluate, exchange};
use crate::support::ContextTestSupport;
use camel_bean::MethodBean;
use camel_core::prelude::*;

/// The exchange, with the Java `MyBean` (`hello(String name)`, `greet()`) bound as `myBean`.
fn bean_exchange() -> (ContextTestSupport, Exchange) {
    let (t, e) = exchange();
    let bean = MethodBean::new("MyBean")
        .method("hello", |inv| {
            let name = inv.parameter_as::<String>(0)?.unwrap_or_default();
            Ok(Value::from(format!("Hello {name}")))
        })
        .method("greet", |_| Ok(Value::from("Greetings")));
    t.context
        .registry()
        .unwrap()
        .bind("myBean", bean.into_value())
        .unwrap();
    (t, e)
}

#[test]
fn test_bean_dot_method() {
    let (_t, mut e) = bean_exchange();
    e.message_mut().set_body("World");
    assert_eq!(
        Value::from("Hello World"),
        evaluate(&e, "${bean:myBean.hello}")
    );
}

#[test]
fn test_bean_no_method() {
    let (_t, e) = bean_exchange();
    assert_eq!(
        Value::from("Greetings"),
        evaluate(&e, "${bean:myBean.greet}")
    );
}

#[test]
fn test_bean_double_colon_method() {
    let (_t, mut e) = bean_exchange();
    e.message_mut().set_body("World");
    assert_eq!(
        Value::from("Hello World"),
        evaluate(&e, "${bean:myBean::hello}")
    );
}

#[test]
fn test_bean_query_method() {
    let (_t, mut e) = bean_exchange();
    e.message_mut().set_body("World");
    assert_eq!(
        Value::from("Hello World"),
        evaluate(&e, "${bean:myBean?method=hello}")
    );
}

#[test]
fn test_unrecognized_function() {
    assert!(
        camel_core::languages::simple::bean_functions::bean_function("unknown", 0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_bean_with_nested_function_parameter() {
    let (_t, mut e) = bean_exchange();
    e.message_mut().set_header("name", "Camel");
    assert_eq!(
        Value::from("Hello Camel"),
        evaluate(&e, "${bean:myBean.hello(${header.name})}")
    );
    // a type: bean needs class loading
    let simple = e.context().resolve_language("simple").unwrap();
    assert!(simple
        .create_expression("${bean:type:com.foo.MyBean}")
        .is_err());
}
