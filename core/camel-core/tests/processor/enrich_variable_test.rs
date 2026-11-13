//! Ports of `EnrichVariableTest`, `EnrichVariableHeadersTest`, `EnrichVariableErrorTest`,
//! `PollEnrichVariableTest` and `PollEnrichVariableHeadersTest`.

use crate::support::ContextTestSupport;
use camel_core::api::error::ErrorType;
use camel_core::prelude::*;

/// The routes of `EnrichVariableTest` (with `echo`: of `EnrichVariableHeadersTest`).
async fn enrich_context(echo: bool) -> ContextTestSupport {
    ContextTestSupport::with_routes(move |r: &mut Routes| {
        r.add(
            from("direct:send")
                .set_variable("hello", simple("Camel"))
                .to("mock:before")
                .enrich("direct:foo")
                .variable_send("hello")
                .to("mock:result"),
        );
        r.add(
            from("direct:receive")
                .enrich("direct:foo")
                .variable_receive("bye")
                .to("mock:after")
                .set_body(simple("${variable:bye}"))
                .to("mock:result"),
        );
        r.add(
            from("direct:sendAndReceive")
                .set_variable("hello", simple("Camel"))
                .to("mock:before")
                .enrich("direct:foo")
                .variable_send("hello")
                .variable_receive("bye")
                .to("mock:result"),
        );
        let sub = from("direct:foo");
        let sub = if echo {
            sub.set_header("echo", simple("${body}${body}"))
        } else {
            sub
        };
        r.add(sub.transform(simple("Bye ${body}")));
    })
    .await
}

#[tokio::test]
async fn test_enrich_variable() {
    for echo in [false, true] {
        // send
        let t = enrich_context(echo).await;
        t.mock("mock:before").expected_bodies_received(["World"]);
        t.mock("mock:before")
            .expected_variable_received("hello", "Camel");
        let result = t.mock("mock:result");
        result.expected_bodies_received(["Bye Camel"]);
        result.expected_variable_received("hello", "Camel");
        if echo {
            result.message_matches(0, header("echo").is_equal_to("CamelCamel"));
        }
        t.template.send_body("direct:send", "World").await.unwrap();
        t.assert_mock_endpoints_satisfied().await;

        // receive
        let t = enrich_context(echo).await;
        let after = t.mock("mock:after");
        after.expected_bodies_received(["World"]);
        after.expected_variable_received("bye", "Bye World");
        let result = t.mock("mock:result");
        result.expected_bodies_received(["Bye World"]);
        result.expected_variable_received("bye", "Bye World");
        if echo {
            after.message_matches(0, header("echo").is_null());
            result.message_matches(0, header("echo").is_null());
        }
        t.template
            .send_body("direct:receive", "World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;

        // send and receive
        let t = enrich_context(echo).await;
        t.mock("mock:before").expected_bodies_received(["World"]);
        t.mock("mock:before")
            .expected_variable_received("hello", "Camel");
        let result = t.mock("mock:result");
        result.expected_bodies_received(["World"]);
        result.expected_variable_received("bye", "Bye Camel");
        if echo {
            result.message_matches(0, header("echo").is_null());
        }
        t.template
            .send_body("direct:sendAndReceive", "World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// The exchange of `direct:receive` with the body `World`.
async fn request(t: &ContextTestSupport) -> Exchange {
    let mut e = t
        .template
        .create_exchange("direct:receive", ExchangePattern::InOut)
        .unwrap();
    e.message_mut().set_body("World");
    t.template.send("direct:receive", e).await.unwrap()
}

/// Port of `EnrichVariableErrorTest`: a failed, handled, stopped or rolled back reply is not
/// stored in the variable.
#[tokio::test]
async fn test_enrich_variable_error() {
    // (case, failed, has variables, body, mock:result count)
    for (case, failed, variables, body, count) in [
        ("throw", true, false, "Bye World", 0),
        ("tryCatch", false, true, "World", 1),
        ("handled", false, false, "Error: Bye World", 0),
        ("notHandled", true, false, "Error: Bye World", 0),
        ("dlc", false, false, "Bye World", 0),
        ("default", true, false, "Bye World", 0),
        ("stop", false, false, "Bye World", 0),
        ("rollback", true, false, "Bye World", 0),
        ("markRollbackOnly", false, false, "Bye World", 0),
        ("markRollbackOnlyLast", false, false, "Bye World", 0),
    ] {
        let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
            match case {
                "handled" | "notHandled" => r.on_exception(
                    on_exception(ErrorType::Any)
                        .handled(case == "handled")
                        .set_body(simple("Error: ${body}")),
                ),
                "dlc" => r.error_handler(dead_letter_channel("mock:dead")),
                "default" => r.error_handler(default_error_handler()),
                _ => r,
            };
            r.add(
                from("direct:receive")
                    .enrich("direct:foo")
                    .variable_receive("bye")
                    .to("mock:result"),
            );
            let sub = from("direct:foo").transform(simple("Bye ${body}"));
            let sub = match case {
                "tryCatch" => sub.do_try(|t| {
                    t.throw_exception(CamelError::illegal_argument("Forced"))
                        .do_catch(ErrorType::Any, |c| c.set_body(simple("Catch: ${body}")))
                }),
                "stop" => sub.stop(),
                "rollback" => sub.rollback(),
                "markRollbackOnly" => sub.mark_rollback_only(),
                "markRollbackOnlyLast" => sub.mark_rollback_only_last(),
                _ => sub.throw_exception(CamelError::illegal_argument("Forced")),
            };
            r.add(sub);
        })
        .await;
        t.mock("mock:result").expected_message_count(count);
        if case == "dlc" {
            t.mock("mock:dead").expected_message_count(1);
        }
        let out = request(&t).await;
        assert_eq!(failed, out.is_failed(), "{case}");
        assert_eq!(variables, !out.variables().is_empty(), "{case}");
        assert_eq!(body, out.message().body().to_string(), "{case}");
        if case == "tryCatch" {
            assert_eq!(
                Some("Catch: Bye World".to_string()),
                out.variable("bye").map(|v| v.to_string())
            );
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Ports of `PollEnrichVariableTest` and `PollEnrichVariableHeadersTest`.
#[tokio::test]
async fn test_poll_enrich_variable() {
    for echo in [false, true] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(
                from("direct:receive")
                    .poll_enrich("seda:foo", 1000)
                    .variable_receive("bye")
                    .to("mock:after")
                    .set_body(simple("${variable:bye}"))
                    .to("mock:result"),
            );
        })
        .await;
        if echo {
            t.template
                .send_body_and_header("seda:foo", "Bye World", "echo", "CamelCamel")
                .await
                .unwrap();
        } else {
            t.template.send_body("seda:foo", "Bye World").await.unwrap();
        }
        let after = t.mock("mock:after");
        after.expected_bodies_received(["World"]);
        after.expected_variable_received("bye", "Bye World");
        let result = t.mock("mock:result");
        result.expected_bodies_received(["Bye World"]);
        result.expected_variable_received("bye", "Bye World");
        if echo {
            result.expected_variable_received("header:bye.echo", "CamelCamel");
            result.message_matches(0, header("echo").is_null());
        }
        t.template
            .send_body("direct:receive", "World")
            .await
            .unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Port of `PollEnrichVariableErrorTest`: a polled failed, stopped or rolled back exchange is
/// not stored in the variable.
#[tokio::test]
async fn test_poll_enrich_variable_error() {
    for case in ["throw", "stop", "rollbackOnly"] {
        let t = ContextTestSupport::with_routes(|r: &mut Routes| {
            r.add(
                from("direct:receive")
                    .poll_enrich("seda:foo", 1000)
                    .variable_receive("bye")
                    .to("mock:result"),
            );
        })
        .await;
        let mut ex = t
            .template
            .create_exchange("seda:foo", ExchangePattern::InOnly)
            .unwrap();
        ex.message_mut().set_body("Bye World");
        match case {
            "throw" => ex.set_exception(Some(CamelError::illegal_argument(""))),
            "stop" => ex.set_route_stop(true),
            _ => ex.set_rollback_only(true),
        }
        let endpoint = t.context.endpoint("seda:foo").unwrap();
        let seda = endpoint
            .as_any()
            .downcast_ref::<camel_seda::SedaEndpoint>()
            .unwrap();
        seda.queue().add(ex).unwrap();
        t.mock("mock:result").expected_message_count(0);
        let out = request(&t).await;
        assert_eq!(case == "throw", out.is_failed(), "{case}");
        assert!(out.variables().is_empty(), "{case}");
        assert_eq!(case == "stop", out.is_route_stop(), "{case}");
        assert_eq!(case == "rollbackOnly", out.is_rollback_only(), "{case}");
        assert_eq!("Bye World", out.message().body().to_string(), "{case}");
        t.assert_mock_endpoints_satisfied().await;
    }
}
