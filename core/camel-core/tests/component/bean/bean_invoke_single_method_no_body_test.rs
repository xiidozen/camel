//! Port of `BeanInvokeSingleMethodNoBodyTest` (the bean instance instead of its class).

use super::with_beans;
use camel_bean::MethodBean;
use camel_core::prelude::*;

#[tokio::test]
async fn test_bean_invoke_single_method_no_body() {
    // Java MySingleMethodNoBodyBean
    let bean = MethodBean::new("MySingleMethodNoBodyBean")
        .method("saySomething", |_| Ok(Value::from("Hello")))
        .into_value();
    let t = with_beans(&[], move |r: &mut Routes| {
        r.add(from("direct:start").bean(bean.clone()).to("mock:result"));
    })
    .await;
    t.mock("mock:result").expected_bodies_received(["Hello"]);
    t.template.send_body("direct:start", "Hi").await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}
