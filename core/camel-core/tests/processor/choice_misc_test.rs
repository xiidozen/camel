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

/// Port of `ChoiceCompoundPredicateTest`.
#[tokio::test]
async fn test_choice_compound_predicate() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        let user = header("username").is_not_null();
        let admin = and(user.clone(), header("admin").is_equal_to("true"));
        let god = and(
            admin.clone(),
            or(
                body().contains("Camel Rider"),
                header("type").is_equal_to("god"),
            ),
        );
        r.add(from("direct:start").choice(move |c| {
            c.when(god, |w| w.to("mock:god"))
                .when(admin, |w| w.to("mock:admin"))
                .when(user, |w| w.to("mock:user"))
                .otherwise(|o| o.to("mock:guest"))
        }));
    })
    .await;
    let h = |pairs: &[(&str, &str)]| -> Vec<(String, Value)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Value::from(*v)))
            .collect()
    };
    for (body, headers, target) in [
        ("Hello World", h(&[]), "mock:guest"),
        ("Hello World", h(&[("username", "goofy")]), "mock:user"),
        (
            "Hello World",
            h(&[("username", "donald"), ("admin", "true")]),
            "mock:admin",
        ),
        (
            "Hello World",
            h(&[("username", "pluto"), ("admin", "true"), ("type", "god")]),
            "mock:god",
        ),
        (
            "Hello Camel Rider",
            h(&[("username", "Camel"), ("admin", "true")]),
            "mock:god",
        ),
    ] {
        let mock = t.mock(target);
        mock.reset();
        mock.expected_message_count(1);
        t.template
            .send_body_and_headers("direct:start", body, headers)
            .await
            .unwrap();
        mock.assert_is_satisfied().await;
    }
}

/// Port of `ChoiceWithEndTest` (not the case navigating the Java processors).
#[tokio::test]
async fn test_choice_with_end() {
    let t = ContextTestSupport::with_routes(|r: &mut Routes| {
        r.add(
            from("direct:start")
                .to("mock:start")
                .choice(|c| {
                    c.when(body().contains("Hello"), |w| {
                        w.transform(simple("echo ${body}")).to("mock:echo")
                    })
                    .when(body().contains("Bye"), |w| {
                        w.to("direct:bye").to("mock:bye")
                    })
                    .otherwise(|o| o.transform(simple("other ${body}")).to("mock:other"))
                })
                .transform(body().prepend("last "))
                .to("mock:last"),
        );
        r.add(from("direct:bye").do_try(|b| {
            b.process_fn(|_| Err(CamelError::illegal_argument("Damn does not work")))
                .to("mock:bye")
                .do_catch(camel_core::api::error::ErrorType::Any, |c| {
                    c.set_body(constant("We do not care"))
                })
        }));
    })
    .await;
    for (body, mock, mock_body, last) in [
        (
            "Hello World",
            "mock:echo",
            "echo Hello World",
            "last echo Hello World",
        ),
        (
            "Bye World",
            "mock:bye",
            "We do not care",
            "last We do not care",
        ),
        ("Camel", "mock:other", "other Camel", "last other Camel"),
    ] {
        for m in ["mock:start", mock, "mock:last"] {
            t.mock(m).reset();
        }
        t.mock("mock:start").expected_bodies_received([body]);
        t.mock(mock).expected_bodies_received([mock_body]);
        t.mock("mock:last").expected_bodies_received([last]);
        t.template.send_body("direct:start", body).await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `ChoicePredicateThrowExceptionTest`.
#[tokio::test]
async fn test_choice_predicate_throw_exception() {
    fn failing() -> camel_core::api::processor::SharedPredicate {
        camel_core::support::builder::named_predicate("forced", |_| {
            Err(CamelError::illegal_argument("Forced"))
        })
    }
    fn choice(route: RouteDsl) -> RouteDsl {
        route.choice(|c| {
            c.when(failing(), |w| w.to("mock:a").to("mock:b").to("mock:c"))
                .otherwise(|o| o.to("mock:d").to("mock:e").to("mock:f"))
        })
    }
    for sub_route in [false, true] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            if sub_route {
                r.add(from("direct:start").to("direct:sub").to("mock:result"));
                r.add(choice(from("direct:sub").error_handler(no_error_handler())));
            } else {
                r.error_handler(no_error_handler());
                r.add(choice(from("direct:start")).to("mock:result"));
            }
        })
        .await;
        for m in [
            "mock:a",
            "mock:b",
            "mock:c",
            "mock:d",
            "mock:e",
            "mock:f",
            "mock:result",
        ] {
            t.mock(m).expected_message_count(0);
        }
        let e = t
            .template
            .send_body("direct:start", "Hello World")
            .await
            .unwrap_err();
        assert!(cause_of(&e).is(ErrorKind::IllegalArgument), "{e}");
        t.assert_mock_endpoints_satisfied().await;
    }
}
