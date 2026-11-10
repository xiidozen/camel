//! Tests of the core EIP processors (ported from the Java camel-core processor tests:
//! PipelineTest, FilterTest, ChoiceTest, SetHeaderTest, StopTest, ...).

use camel_api::context::{CamelContext, StandaloneContext};
use camel_api::endpoint::{Endpoint, Producer, SharedConsumer, SharedEndpoint, SharedProducer};
use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{
    block_on, expression_fn, predicate_fn, processor_fn, BoxFuture, ConstantExpression, Processor,
    SharedProcessor,
};
use camel_api::service::Service;
use camel_core_processor::choice::ChoiceProcessor;
use camel_core_processor::filter::FilterProcessor;
use camel_core_processor::log_processor::{to_log_level, LogProcessor};
use camel_core_processor::pipeline::Pipeline;
use camel_core_processor::send::SendProcessor;
use camel_core_processor::transform::*;
use camel_util::value::Value;
use std::any::Any;
use std::sync::{Arc, Mutex};

fn ex(body: &str) -> Exchange {
    Exchange::with_body(StandaloneContext::shared(), body)
}

fn body(e: &Exchange) -> String {
    e.message().body_as::<String>().unwrap().unwrap_or_default()
}

/// A processor appending a suffix to the body.
fn append(s: &'static str) -> SharedProcessor {
    Arc::new(processor_fn(move |e: &mut Exchange| {
        let b = body(e);
        e.message_mut().set_body(format!("{b}{s}"));
        Ok(())
    }))
}

fn constant(v: &str) -> Arc<ConstantExpression> {
    Arc::new(ConstantExpression(Value::from(v)))
}

#[test]
fn pipeline_runs_in_sequence() {
    let p = Pipeline::new(vec![append("1"), append("2"), append("3")]);
    let mut e = ex("x");
    block_on(p.process(&mut e)).unwrap();
    assert_eq!("x123", body(&e));
    assert_eq!(3, p.processors().len());
}

#[test]
fn pipeline_out_becomes_in() {
    let to_out: SharedProcessor = Arc::new(processor_fn(|e: &mut Exchange| {
        e.out_message_mut().set_body("from-out");
        Ok(())
    }));
    let p = Pipeline::new(vec![to_out, append("!")]);
    let mut e = ex("x");
    block_on(p.process(&mut e)).unwrap();
    assert_eq!("from-out!", body(&e));
}

#[test]
fn pipeline_stops_on_error_and_keeps_it() {
    let fail: SharedProcessor = Arc::new(processor_fn(|_| Err(CamelError::runtime("Forced"))));
    let p = Pipeline::new(vec![append("1"), fail, append("2")]);
    let mut e = ex("x");
    block_on(p.process(&mut e)).unwrap();
    assert_eq!("x1", body(&e));
    assert!(e.is_failed());
    assert_eq!(Some("Forced"), e.exception().unwrap().message());
}

#[test]
fn stop_processor_ends_routing() {
    let p = Pipeline::new(vec![append("1"), Arc::new(StopProcessor), append("2")]);
    let mut e = ex("x");
    block_on(p.process(&mut e)).unwrap();
    assert_eq!("x1", body(&e));
    assert!(e.is_route_stop());
    assert!(!e.is_failed());
}

#[test]
fn filter() {
    let f = FilterProcessor::new(
        Arc::new(predicate_fn(|e: &Exchange| Ok(body(e).contains("gold")))),
        append("!"),
    )
    .with_status_property_name("CamelFilterMatched");
    let mut gold = ex("gold");
    let mut silver = ex("silver");
    block_on(f.process(&mut gold)).unwrap();
    block_on(f.process(&mut silver)).unwrap();
    assert_eq!("gold!", body(&gold));
    assert_eq!("silver", body(&silver));
    assert_eq!(
        Some(&Value::Bool(true)),
        gold.property("CamelFilterMatched")
    );
    assert_eq!(
        Some(&Value::Bool(false)),
        silver.property("CamelFilterMatched")
    );
    assert_eq!(1, f.filtered_count());
}

#[test]
fn filter_predicate_error_fails_exchange() {
    let f = FilterProcessor::new(
        Arc::new(predicate_fn(|_| Err(CamelError::runtime("bad predicate")))),
        append("!"),
    );
    let mut e = ex("x");
    block_on(f.process(&mut e)).unwrap();
    assert_eq!("x", body(&e));
    assert!(e.is_failed());
}

fn when(text: &'static str, suffix: &'static str) -> FilterProcessor {
    FilterProcessor::new(
        Arc::new(predicate_fn(move |e: &Exchange| Ok(body(e).contains(text)))),
        append(suffix),
    )
}

#[test]
fn choice_picks_first_match_or_otherwise() {
    let c = ChoiceProcessor::new(vec![when("a", "-A"), when("b", "-B")], Some(append("-O")));
    for (input, expected) in [("a", "a-A"), ("ab", "ab-A"), ("b", "b-B"), ("z", "z-O")] {
        let mut e = ex(input);
        block_on(c.process(&mut e)).unwrap();
        assert_eq!(expected, body(&e));
    }
    assert_eq!(1, c.not_filtered_count());
    assert_eq!(2, c.whens().len());

    let no_otherwise = ChoiceProcessor::new(vec![when("a", "-A")], None);
    let mut e = ex("z");
    block_on(no_otherwise.process(&mut e)).unwrap();
    assert_eq!("z", body(&e));
}

#[test]
fn choice_breaks_out_on_predicate_error() {
    let bad = FilterProcessor::new(
        Arc::new(predicate_fn(|_| Err(CamelError::runtime("boom")))),
        append("-X"),
    );
    let c = ChoiceProcessor::new(vec![bad, when("a", "-A")], Some(append("-O")));
    let mut e = ex("a");
    block_on(c.process(&mut e)).unwrap();
    assert_eq!("a", body(&e));
    assert!(e.is_failed());
}

#[test]
fn message_translators() {
    let p = Pipeline::new(vec![
        Arc::new(SetHeaderProcessor::new("foo", constant("123"))),
        Arc::new(SetHeaderProcessor::new("CamelSecret", constant("x"))),
        Arc::new(SetHeaderProcessor::new("CamelKeep", constant("y"))),
        Arc::new(SetPropertyProcessor::new("prop", constant("p"))),
        Arc::new(SetVariableProcessor::new("var", constant("v"))),
        Arc::new(SetBodyProcessor::new(Arc::new(expression_fn(
            |e: &Exchange| Ok(Value::from(format!("Hello {}", body(e)))),
        )))),
        Arc::new(RemoveHeadersProcessor::new("Camel*", &["CamelKeep"])),
    ]);
    let mut e = ex("World");
    block_on(p.process(&mut e)).unwrap();
    assert_eq!("Hello World", body(&e));
    assert_eq!(
        Some("123".to_string()),
        e.message().header_as::<String>("foo").unwrap()
    );
    assert!(e.message().header("CamelSecret").is_none());
    assert!(e.message().header("CamelKeep").is_some());
    assert_eq!(Some(&Value::from("p")), e.property("prop"));
    assert_eq!(Some(&Value::from("v")), e.variable("var"));
    block_on(RemoveHeaderProcessor::new("FOO").process(&mut e)).unwrap();
    assert!(e.message().header("foo").is_none());
}

#[test]
fn throw_exception() {
    let p = Pipeline::new(vec![
        Arc::new(ThrowExceptionProcessor::new(CamelError::new(
            ErrorKind::IllegalArgument,
            "Forced",
        ))),
        append("!"),
    ]);
    let mut e = ex("x");
    block_on(p.process(&mut e)).unwrap();
    assert!(e.exception().unwrap().is(ErrorKind::IllegalArgument));
    assert_eq!("x", body(&e));
}

#[test]
fn log_processor() {
    assert_eq!(None, to_log_level(LoggingLevel::OFF));
    assert_eq!(Some(log::Level::Info), to_log_level(LoggingLevel::INFO));
    let p = LogProcessor::new(constant("hello"), "route1", LoggingLevel::INFO);
    let mut e = ex("x");
    block_on(p.process(&mut e)).unwrap();
    assert_eq!("x", body(&e));
}

/// An endpoint recording the bodies and patterns sent to it.
struct RecordingEndpoint {
    received: Mutex<Vec<(String, ExchangePattern)>>,
}

impl Service for RecordingEndpoint {}

impl Endpoint for RecordingEndpoint {
    fn endpoint_uri(&self) -> &str {
        "recording:test"
    }

    fn camel_context(&self) -> Arc<dyn CamelContext> {
        StandaloneContext::shared()
    }

    fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
        Ok(Arc::new(RecordingProducer(self)))
    }

    fn create_consumer(
        self: Arc<Self>,
        _processor: SharedProcessor,
    ) -> CamelResult<SharedConsumer> {
        Err(CamelError::runtime("not supported"))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct RecordingProducer(Arc<RecordingEndpoint>);

impl Service for RecordingProducer {}

impl Processor for RecordingProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        self.0
            .received
            .lock()
            .unwrap()
            .push((body(exchange), exchange.pattern()));
        exchange.message_mut().set_body("reply");
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Producer for RecordingProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.0.clone()
    }
}

#[test]
fn send_processor() {
    let endpoint = Arc::new(RecordingEndpoint {
        received: Mutex::new(vec![]),
    });
    let send = SendProcessor::new(endpoint.clone(), Some(ExchangePattern::InOut));
    assert_eq!("recording:test", send.endpoint().endpoint_uri());
    let mut e = ex("hello");
    block_on(send.process(&mut e)).unwrap();
    block_on(send.process(&mut e)).unwrap();
    assert_eq!("reply", body(&e));
    // the pattern is restored after the send
    assert_eq!(ExchangePattern::InOnly, e.pattern());
    let received = endpoint.received.lock().unwrap();
    assert_eq!(
        vec![
            ("hello".to_string(), ExchangePattern::InOut),
            ("reply".to_string(), ExchangePattern::InOut)
        ],
        *received
    );
}
