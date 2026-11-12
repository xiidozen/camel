//! Tests of `org.apache.camel.component.bean`, with the Java test beans as
//! [`MethodBean`]s.

mod bean_component_test;
mod bean_explicit_method_test;
mod bean_invoke_single_method_no_body_test;
mod bean_ref_method_not_found_test;
mod bean_ref_not_found_test;
mod method_call_ref_or_bean_prefix_test;

use crate::support::ContextTestSupport;
use camel_bean::MethodBean;
use camel_core::prelude::*;

/// Java `MyFooBean`: `hello(String s)` answers `Hello <s>`.
pub fn my_foo_bean() -> Value {
    MethodBean::new("MyFooBean")
        .method("hello", |inv| {
            Ok(Value::from(format!("Hello {}", inv.parameter(0))))
        })
        .into_value()
}

/// Java `MyOtherFooBean`: `echo(String)` answers the text twice, `echo(Integer)` the square
/// (Java chooses the overload by the body type).
pub fn my_other_foo_bean() -> Value {
    MethodBean::new("MyOtherFooBean")
        .method("echo", |inv| {
            Ok(match inv.parameter(0) {
                Value::Int(i) => Value::Int(i * i),
                v => Value::from(format!("{v}{v}")),
            })
        })
        .into_value()
}

/// A context with the beans bound in its registry, then the routes added and the context
/// started (Java `createCamelRegistry` + `createRouteBuilder`).
pub async fn with_beans(beans: &[(&str, Value)], routes: impl RouteBuilder) -> ContextTestSupport {
    let t = bind(beans);
    t.context
        .add_routes(routes)
        .await
        .expect("the routes of the test");
    t.context.start().await.expect("the context starts");
    t
}

/// A context with the beans bound in its registry, without routes.
pub fn bind(beans: &[(&str, Value)]) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    let registry = t.context.registry().unwrap();
    for (name, bean) in beans {
        registry.bind(name, bean.clone()).unwrap();
    }
    t
}
