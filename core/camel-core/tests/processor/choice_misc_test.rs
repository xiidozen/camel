//! Ports of `ChoiceWithSimpleExpressionTest` (the cases of `ChoiceTest` with simple
//! predicates), `ChoiceNoActionTest`, `ChoiceNoErrorHandlerTest`,
//! `ChoiceWhenNotPredicateTest`, `ChoiceWhenNoOutputTest` and `ChoiceWithTranfromTest`.

use crate::support::{cause_of, ContextTestSupport};
use camel_core::prelude::*;

#[tokio::test]
async fn test_choice_with_simple_expression() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .choice(|c| {
                    c.when(simple("${header.foo} == 'bar'"), |w| w.to("mock:x"))
                        .when(simple("${in.header.foo} == 'cheese'"), |w| w.to("mock:y"))
                        .otherwise(|o| o.to("mock:z"))
                })
                .to("mock:end"),
        );
    })
    .await;
    for (foo, body, target) in [
        ("bar", "<one/>", "mock:x"),
        ("cheese", "<two/>", "mock:y"),
        ("somethingUndefined", "<three/>", "mock:z"),
    ] {
        for m in ["mock:x", "mock:y", "mock:z", "mock:end"] {
            let mock = t.mock(m);
            mock.reset();
            if m == target || m == "mock:end" {
                mock.expected_bodies_received([body]);
            } else {
                mock.expected_message_count(0);
            }
        }
        t.template
            .send_body_and_header("direct:start", body, "foo", foo)
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_choice_no_action() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .choice(|c| c.when(body().is_equal_to("Hello World"), |w| w.to("mock:world"))),
        );
    })
    .await;
    for (body, count) in [("Hello World", 1), ("No action please", 0)] {
        let mock = t.mock("mock:world");
        mock.reset();
        mock.expected_message_count(count);
        t.template.send_body("direct:start", body).await.unwrap();
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_choice_no_error_handler() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.error_handler(no_error_handler());
        r.add(from("direct:start").choice(|c| {
            c.when(header("foo").is_equal_to("bar"), |w| w.to("direct:end"))
                .otherwise(|o| o)
        }));
        r.add(
            from("direct:end")
                .process_fn(|e| {
                    assert_eq!(&Value::from("Hello World"), e.message().body());
                    Ok(())
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World"]);
    t.template
        .send_body_and_header("direct:start", "Hello World", "foo", "bar")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_choice_when_not_predicate() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:start").choice(|c| {
            c.when(not(header("username").regex("goofy|pluto")), |w| {
                w.to("mock:people")
            })
            .otherwise(|o| o.to("mock:animals"))
        }));
    })
    .await;
    for (user, target) in [("donald", "mock:people"), ("pluto", "mock:animals")] {
        let mock = t.mock(target);
        mock.expected_message_count(1);
        t.template
            .send_body_and_header("direct:start", "Hello World", "username", user)
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }
}

#[tokio::test]
async fn test_choice_when_no_output() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .choice(|c| {
                    c.when(header("test").is_equal_to("1"), |w| w)
                        .when(header("test").is_equal_to("2"), |w| w.to("mock:2"))
                        .otherwise(|o| {
                            o.throw_exception(CamelError::illegal_argument("Validation error!"))
                        })
                })
                .to("mock:result"),
        );
    })
    .await;
    t.mock("mock:result")
        .expected_bodies_received(["Hello World", "Bye World"]);
    t.mock("mock:2").expected_bodies_received(["Bye World"]);
    t.template
        .send_body_and_header("direct:start", "Hello World", "test", "1")
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:start", "Bye World", "test", "2")
        .await
        .unwrap();
    let e = t
        .template
        .send_body_and_header("direct:start", "Hi World", "test", "3")
        .await
        .unwrap_err();
    let cause = cause_of(&e);
    assert!(cause.is(ErrorKind::IllegalArgument), "{e}");
    assert_eq!(Some("Validation error!"), cause.message());
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_choice_with_transform() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(from("direct:outerRoute").route_id("out").choice(|c| {
            c.when(header("test-header").is_not_null(), |w| {
                w.to("direct:mainProcess")
            })
            .otherwise(|o| {
                o.to("log:badMessage")
                    .transform(simple("Rejecting ${body}"))
            })
        }));
        r.add(from("direct:mainProcess").transform(simple("Processing ${body}")));
    })
    .await;
    let out = t
        .template
        .request_body_and_header("direct:outerRoute", "body", "test-header", "headerValue")
        .await
        .unwrap();
    assert_eq!(Value::from("Processing body"), out);
    let out = t
        .template
        .request_body("direct:outerRoute", "body")
        .await
        .unwrap();
    assert_eq!(Value::from("Rejecting body"), out);
}
