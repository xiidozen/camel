//! Ports of `ValidationTest` and its route variants (`ValidationWithExceptionTest`,
//! `ValidationWithInFlowExceptionTest`, `ValidationWithErrorInHandleAndFinallyBlockTest`,
//! `ValidationWithHandlePipelineTest`, `ValidationWithFinallyBlockPipelineTest`,
//! `ValidationWithNestedFinallyBlockPipelineTest`, `ValidationWithMultipleHandlesTest`,
//! `ValidationWithHandlePipelineAndExceptionTest`, `ValidationWithTryCatchTest`), of
//! `ValidationFinallyBlockTest` and `ValidationFinallyBlockNoCatchTest` (with `MyValidator`).

use crate::support::ContextTestSupport;
use camel_core::api::error::ErrorType;
use camel_core::api::processor::SharedProcessor;
use camel_core::prelude::*;
use std::sync::Arc;

/// Java `MyValidator`: the foo header must be `bar`.
fn validator(e: &mut Exchange) -> CamelResult<()> {
    let value = e.message().header("foo").cloned().unwrap_or_default();
    if !matches!(&value, Value::Text(t) if t == "bar") {
        return Err(CamelError::validation(
            e,
            &format!("The foo header does not equal bar! Was: {value}"),
        ));
    }
    Ok(())
}

fn validation() -> ErrorType {
    ErrorType::Kind(ErrorKind::Validation)
}

/// The routes of each variant.
fn routes(variant: &str, r: &mut Routes, template: Arc<ProducerTemplate>) {
    match variant {
        "ValidationTest" => r.add(from("direct:start").do_try(|t| {
            t.process_fn(validator)
                .to("mock:valid")
                .do_catch(validation(), |c| c.to("mock:invalid"))
        })),
        "WithException" | "WithInFlowException" => {
            let dlc = dead_letter_channel("mock:error");
            r.error_handler(if variant == "WithInFlowException" {
                dlc.redelivery_delay(0).maximum_redeliveries(3)
            } else {
                dlc
            });
            r.on_exception(on_exception(validation()).to("mock:invalid"));
            r.add(from("direct:start").process_fn(validator).to("mock:valid"))
        }
        "WithErrorInHandleAndFinallyBlock" => r.add(
            from("direct:start")
                .error_handler(no_error_handler())
                .do_try(|t| {
                    t.process_fn(validator)
                        .do_catch(validation(), |c| c.process_fn(validator))
                        .do_finally(|f| {
                            f.choice(|c| {
                                c.when(header("foo").is_equal_to("bar"), |w| w.to("mock:valid"))
                                    .otherwise(|o| o.to("mock:invalid"))
                            })
                        })
                }),
        ),
        "WithHandlePipeline" => r.add(from("direct:start").do_try(|t| {
            t.process_fn(validator)
                .to("mock:valid")
                .do_catch(validation(), |c| {
                    c.do_try(|t| {
                        t.process_fn(validator)
                            .to("mock:valid")
                            .do_catch(validation(), |c| {
                                c.pipeline(|p| p.to("log:a").to("mock:invalid"))
                            })
                    })
                })
        })),
        "WithFinallyBlockPipeline" => r.add(from("direct:start").do_try(|t| {
            t.process_fn(validator)
                .set_header("valid", constant(true))
                .do_catch(validation(), |c| c.set_header("valid", constant(false)))
                .do_finally(|f| {
                    f.set_body(body()).choice(|c| {
                        c.when(header("valid").is_equal_to(true), |w| w.to("mock:valid"))
                            .otherwise(|o| o.to("mock:invalid"))
                    })
                })
        })),
        "WithNestedFinallyBlockPipeline" => {
            r.add(from("direct:start").do_try(|t| {
                t.to("direct:embedded")
                    .do_catch(validation(), |c| c.to("mock:invalid"))
            }));
            r.add(
                from("direct:embedded")
                    .error_handler(no_error_handler())
                    .do_try(|t| {
                        t.process_fn(validator)
                            .to("mock:valid")
                            .do_finally(|f| f.set_header("valid", constant(false)))
                    }),
            )
        }
        "WithMultipleHandles" => r.add(
            from("direct:start")
                .do_try(|t| {
                    t.process_fn(validator)
                        .do_catch(validation(), |c| c.set_header("xxx", constant("yyy")))
                })
                .do_try(|t| {
                    t.process_fn(validator)
                        .to("mock:valid")
                        .do_catch(validation(), |c| {
                            c.pipeline(|p| p.to("seda:a").to("mock:invalid"))
                        })
                }),
        ),
        "WithHandlePipelineAndException" => {
            r.error_handler(
                dead_letter_channel("mock:error")
                    .redelivery_delay(0)
                    .maximum_redeliveries(3),
            );
            r.on_exception(on_exception(validation()).to("mock:outer"));
            r.add(from("direct:start").do_try(|t| {
                t.process_fn(validator)
                    .to("mock:valid")
                    .do_catch(validation(), |c| c.to("mock:invalid").process_fn(validator))
            }))
        }
        "WithTryCatch" => {
            // the validation and the sending in a processor
            let p: SharedProcessor = Arc::new(camel_core::api::processor::async_processor_fn(
                move |e: &mut Exchange| {
                    let template = template.clone();
                    Box::pin(async move {
                        let target = if validator(e).is_ok() {
                            "mock:valid"
                        } else {
                            "mock:invalid"
                        };
                        let sent = template.send(target, e.clone()).await?;
                        *e = sent;
                        Ok(())
                    })
                },
            ));
            r.add(from("direct:start").process(p))
        }
        other => panic!("{other}"),
    };
}

async fn context(variant: &'static str) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    let template = Arc::new(t.context.create_producer_template());
    t.context
        .add_routes(move |r: &mut Routes| routes(variant, r, template.clone()))
        .await
        .unwrap();
    t.context.start().await.unwrap();
    for (uri, body) in [
        ("mock:valid", "validResult"),
        ("mock:invalid", "invalidResult"),
    ] {
        let p: SharedProcessor = Arc::new(camel_core::api::processor::processor_fn(
            move |e: &mut Exchange| {
                e.message_mut().set_body(body);
                Ok(())
            },
        ));
        t.mock(uri).when_any_exchange_received(p);
    }
    t
}

const VARIANTS: [&str; 10] = [
    "ValidationTest",
    "WithException",
    "WithInFlowException",
    "WithErrorInHandleAndFinallyBlock",
    "WithHandlePipeline",
    "WithFinallyBlockPipeline",
    "WithNestedFinallyBlockPipeline",
    "WithMultipleHandles",
    "WithHandlePipelineAndException",
    "WithTryCatch",
];

#[tokio::test]
async fn test_valid_message() {
    for variant in VARIANTS {
        let t = context(variant).await;
        t.mock("mock:valid").expected_message_count(1);
        t.mock("mock:invalid").expected_message_count(0);
        let result = t
            .template
            .request_body_and_header("direct:start", "<valid/>", "foo", "bar")
            .await
            .unwrap();
        assert_eq!("validResult", result.to_string(), "{variant}");
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_invalid_message() {
    for variant in VARIANTS {
        let t = context(variant).await;
        t.mock("mock:valid").expected_message_count(0);
        t.mock("mock:invalid").expected_message_count(1);
        let _ = t
            .template
            .send_body_and_header("direct:start", "<invalid/>", "foo", "notMatchedHeaderValue")
            .await;
        t.assert_mock_endpoints_satisfied().await;
    }
}

#[tokio::test]
async fn test_invalid_then_valid_message() {
    for variant in VARIANTS {
        let t = context(variant).await;
        t.mock("mock:valid").expected_message_count(2);
        t.mock("mock:invalid").expected_message_count(1);
        let _ = t
            .template
            .send_body_and_header("direct:start", "<invalid/>", "foo", "notMatchedHeaderValue")
            .await;
        for _ in 0..2 {
            let result = t
                .template
                .request_body_and_header("direct:start", "<valid/>", "foo", "bar")
                .await
                .unwrap();
            assert_eq!("validResult", result.to_string(), "{variant}");
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// Ports of `ValidationFinallyBlockTest` and `ValidationFinallyBlockNoCatchTest`.
#[tokio::test]
async fn test_finally_block() {
    for no_catch in [false, true] {
        for valid in [true, false] {
            let t = ContextTestSupport::with_routes(move |r: &mut Routes| {
                if no_catch {
                    r.error_handler(
                        dead_letter_channel("mock:dead")
                            .redelivery_delay(0)
                            .maximum_redeliveries(3)
                            .log_stack_trace(false),
                    );
                    r.add(from("direct:start").do_try(|t| {
                        t.process_fn(validator)
                            .to("mock:valid")
                            .do_finally(|f| f.to("mock:all"))
                    }));
                } else {
                    r.add(from("direct:start").do_try(|t| {
                        t.process_fn(validator)
                            .to("mock:valid")
                            .do_catch(validation(), |c| c.to("mock:invalid"))
                            .do_finally(|f| f.to("mock:all"))
                    }));
                }
            })
            .await;
            t.mock("mock:valid")
                .expected_message_count(i32::from(valid));
            if !no_catch {
                t.mock("mock:invalid")
                    .expected_message_count(i32::from(!valid));
            } else {
                t.mock("mock:dead").expected_message_count(0);
            }
            t.mock("mock:all").expected_message_count(1);
            let (body, foo) = if valid {
                ("<valid/>", "bar")
            } else {
                ("<invalid/>", "notMatchedHeaderValue")
            };
            let r = t
                .template
                .send_body_and_header("direct:start", body, "foo", foo)
                .await;
            assert_eq!(no_catch && !valid, r.is_err(), "{no_catch} {valid}");
            t.assert_mock_endpoints_satisfied().await;
        }
    }
}
