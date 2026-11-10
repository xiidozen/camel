//! Tests of the mock endpoint, ported from the Java camel-core `MockEndpointTest`.
//!
//! The Java clause builder `message(i).header("foo").isEqualTo(123)` is written
//! `mock.message_matches(i, header("foo").is_equal_to(123))`; the simple language predicate of
//! `testExpectedMessagesMatches` and the bean of `testBodyTransformed` are written as closures.

use camel_api::enums::ExchangePattern;
use camel_api::error::{CamelError, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{
    expression_fn, predicate_fn, processor_fn, SharedExpression, SharedProcessor,
};
use camel_base_engine::context::DefaultCamelContext;
use camel_base_engine::producer_template::ProducerTemplate;
use camel_base_engine::route::RouteSpec;
use camel_core_processor::pipeline::Pipeline;
use camel_core_processor::send::SendProcessor;
use camel_core_processor::transform::SetBodyProcessor;
use camel_direct::DirectComponent;
use camel_mock::{MockComponent, MockEndpoint};
use camel_support::builder::{body as body_value, constant, exchange_pattern, header};
use camel_util::value::Value;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

struct Test {
    context: Arc<DefaultCamelContext>,
    template: ProducerTemplate,
    mock: MockEndpoint,
}

impl Test {
    /// The routes of the Java test: `direct:a` to `mock:result`, and `direct:b` appending
    /// " World" to the body before `mock:result`.
    async fn new() -> Test {
        let context = DefaultCamelContext::new();
        context
            .add_component("direct", Arc::new(DirectComponent::new()))
            .unwrap();
        context
            .add_component("mock", Arc::new(MockComponent::new()))
            .unwrap();
        let to_mock = || -> SharedProcessor {
            Arc::new(SendProcessor::new(
                context.endpoint_as::<MockEndpoint>("mock:result").unwrap(),
                None,
            ))
        };
        context
            .add_route(RouteSpec::new("direct:a", to_mock()))
            .await
            .unwrap();
        let append: SharedProcessor = Arc::new(SetBodyProcessor::new(Arc::new(expression_fn(
            |e: &Exchange| Ok(Value::from(format!("{} World", e.message().body()))),
        ))));
        context
            .add_route(RouteSpec::new(
                "direct:b",
                Arc::new(Pipeline::new(vec![append, to_mock()])),
            ))
            .await
            .unwrap();
        context.start().await.unwrap();
        let template = context.create_producer_template();
        let mock = MockEndpoint::resolve(context.as_ref(), "mock:result").unwrap();
        Test {
            context,
            template,
            mock,
        }
    }

    async fn send_messages(&self, counters: &[i64]) {
        for c in counters {
            self.template
                .send_body_and_header("direct:a", message(*c), "counter", *c)
                .await
                .unwrap();
        }
    }

    async fn send_header(&self, name: &str, value: impl Into<Value>) {
        self.template
            .send_body_and_header("direct:a", "body", name, value)
            .await
            .unwrap();
    }

    async fn send_with(&self, f: impl Fn(&mut Exchange)) {
        let mut e = self
            .template
            .create_exchange("direct:a", ExchangePattern::InOnly)
            .unwrap();
        f(&mut e);
        self.template.send("direct:a", e).await.unwrap();
    }

    async fn assert_err(&self) -> String {
        self.mock
            .try_assert_is_satisfied()
            .await
            .unwrap_err()
            .message()
            .to_string()
    }

    fn reset_mocks(&self) {
        MockEndpoint::reset_mocks(self.context.endpoints());
    }
}

fn message(counter: i64) -> String {
    format!("<message>{counter}</message>")
}

fn messages(counters: &[i64]) -> Vec<String> {
    counters.iter().map(|c| message(*c)).collect()
}

/// Java `header("counter").convertTo(Number.class)`.
fn counter() -> SharedExpression {
    Arc::new(expression_fn(|e: &Exchange| {
        Ok(e.message().header("counter").cloned().unwrap_or_default())
    }))
}

fn body() -> SharedExpression {
    Arc::new(expression_fn(|e: &Exchange| Ok(e.message().body().clone())))
}

#[tokio::test]
async fn test_ascending_messages_pass() {
    let t = Test::new().await;
    t.mock.expects_ascending(counter());
    t.send_messages(&[11, 12, 13, 14, 15]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_ascending_messages_fail() {
    let t = Test::new().await;
    t.mock.expects_ascending(counter());
    t.send_messages(&[11, 12, 13, 15, 14]).await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_descending_messages_pass() {
    let t = Test::new().await;
    t.mock.expects_descending(counter());
    t.send_messages(&[15, 14, 13, 12, 11]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_descending_messages_fail() {
    let t = Test::new().await;
    t.mock.expects_descending(counter());
    t.send_messages(&[15, 14, 13, 11, 12]).await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_expects_bodies_in_order() {
    let t = Test::new().await;
    t.mock
        .expected_bodies_received(messages(&[11, 12, 13, 14, 15]));
    t.send_messages(&[11, 12, 13, 14, 15]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expects_bodies_in_any_order() {
    let t = Test::new().await;
    t.mock
        .expected_bodies_received_in_any_order(messages(&[11, 12, 13, 14, 15]));
    t.send_messages(&[15, 12, 14, 13, 11]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expects_bodies_in_any_order_with_duplicates() {
    let t = Test::new().await;
    t.mock
        .expected_bodies_received_in_any_order(messages(&[11, 15, 12, 12, 13, 14, 15, 15]));
    t.send_messages(&[15, 15, 12, 14, 13, 12, 15, 11]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expects_headers_in_any_order() {
    let t = Test::new().await;
    t.mock
        .expected_header_values_received_in_any_order("counter", [11, 12, 13, 14, 15]);
    t.send_messages(&[15, 12, 14, 13, 11]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expects_headers_in_any_order_fail() {
    let t = Test::new().await;
    t.mock
        .expected_header_values_received_in_any_order("counter", [11, 12, 7, 14, 15]);
    t.send_messages(&[15, 12, 14, 13, 11]).await;
    assert_eq!(
        "mock://result Expected 5 headers with key[counter], received 4 headers. Expected header values: [7]",
        t.assert_err().await
    );
}

#[tokio::test]
async fn test_expects_properties_in_any_order() {
    let t = Test::new().await;
    t.mock
        .expected_property_values_received_in_any_order("foo", [123, 456]);
    t.send_with(|e| e.set_property("foo", 456)).await;
    t.send_with(|e| e.set_property("foo", 123)).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expects_properties_in_any_order_fail() {
    let t = Test::new().await;
    t.mock
        .expected_property_values_received_in_any_order("foo", [123, 456]);
    t.send_with(|e| e.set_property("foo", 123)).await;
    t.send_with(|e| e.set_property("foo", 789)).await;
    assert_eq!(
        "mock://result Expected 2 properties with key[foo], received 1 properties. Expected property values: [456]",
        t.assert_err().await
    );
}

#[tokio::test]
async fn test_no_duplicate_messages_pass() {
    let t = Test::new().await;
    t.mock.expects_no_duplicates(counter());
    t.send_messages(&[11, 12, 13, 14, 15]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_duplicate_messages_fail() {
    let t = Test::new().await;
    t.mock.expects_no_duplicates(counter());
    t.send_messages(&[11, 12, 13, 14, 12]).await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_expectations_after_messages_arrive_pass() {
    let t = Test::new().await;
    t.send_messages(&[11, 12, 13, 14, 12]).await;
    t.mock.expected_message_count(5);
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expectations_after_messages_arrive_fail() {
    let t = Test::new().await;
    t.send_messages(&[11, 12, 13, 14, 12]).await;
    t.mock.expected_message_count(6);
    t.mock.set_result_wait_time(500);
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_reset() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.send_messages(&[11, 12]).await;
    t.mock.assert_is_satisfied().await;
    t.mock.reset();
    t.mock.expected_message_count(3);
    t.send_messages(&[11, 12, 13]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_expectation_of_header() {
    let t = Test::new().await;
    t.mock.reset();
    t.mock.expected_header_received("header", "value");
    t.send_header("header", "value").await;
    t.mock.assert_is_satisfied().await;

    t.mock.reset();
    t.mock.expected_header_received("header", "value1");
    t.send_header("header", "value").await;
    t.mock.assert_is_not_satisfied().await;

    t.mock.reset();
    t.mock.expected_header_received("header1", "value");
    t.send_header("header", "value").await;
    t.mock.assert_is_not_satisfied().await;

    t.mock.reset();
    t.mock.expected_header_received("header1", "value1");
    t.send_header("header", "value").await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_expectation_of_header_with_number() {
    let t = Test::new().await;
    t.mock.reset();
    t.mock.expected_header_received("number", 123);
    t.send_header("number", 123).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_ascending() {
    let t = Test::new().await;
    t.mock.expects_ascending(body());
    t.mock.expects_ascending(counter());
    t.send_messages(&[1, 2, 3, 4, 5]).await;
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_ascending_failed() {
    let t = Test::new().await;
    t.mock.expects_ascending(body());
    t.mock.expects_ascending(counter());
    t.send_messages(&[1, 2, 5, 3, 4]).await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_descending() {
    let t = Test::new().await;
    t.mock.expects_descending(body());
    t.mock.expects_descending(counter());
    t.send_messages(&[5, 4, 3, 2, 1]).await;
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_descending_failed() {
    let t = Test::new().await;
    t.mock.expects_descending(body());
    t.mock.expects_descending(counter());
    t.send_messages(&[5, 4, 2, 3, 1]).await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_no_duplicates() {
    let t = Test::new().await;
    t.mock.expects_no_duplicates(body());
    t.mock.expects_no_duplicates(counter());
    t.send_messages(&[1, 2, 3, 4, 5]).await;
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_no_duplicates_failed() {
    let t = Test::new().await;
    t.mock.expects_no_duplicates(body());
    t.mock.expects_no_duplicates(counter());
    t.send_messages(&[1, 2, 5, 2, 4]).await;
    t.mock.assert_is_not_satisfied().await;
}

// testBody: expectedBodyReceived().constant(..)
#[tokio::test]
async fn test_body() {
    let t = Test::new().await;
    t.mock.expected_bodies_received(["<message>1</message>"]);
    t.send_messages(&[1]).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_body_failed() {
    let t = Test::new().await;
    t.mock.expected_bodies_received(["<message>2</message>"]);
    t.send_messages(&[1]).await;
    assert_eq!(
        "mock://result Body of message: 0. Expected: <<message>2</message>> but was: <<message>1</message>>",
        t.assert_err().await
    );
}

#[tokio::test]
async fn test_body_transformed() {
    let t = Test::new().await;
    t.mock.expected_bodies_received(["Hello World"]);
    t.template.send_body("direct:b", "Hello").await.unwrap();
    t.mock.assert_is_satisfied().await;
}

fn assert_forced(e: CamelError) {
    assert!(e.is(ErrorKind::CamelExecution));
    let cause = e
        .source()
        .and_then(|c| c.downcast_ref::<CamelError>())
        .unwrap();
    assert!(cause.is(ErrorKind::IllegalArgument));
    assert_eq!(Some("Forced"), cause.message());
}

use std::error::Error;

#[tokio::test]
async fn test_simulate_error() {
    let t = Test::new().await;
    t.mock
        .when_any_exchange_received(Arc::new(processor_fn(|e: &mut Exchange| {
            e.set_exception(Some(CamelError::illegal_argument("Forced")));
            Ok(())
        })));
    assert_forced(
        t.template
            .send_body("direct:a", "Hello World")
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn test_simulate_error_by_throwing_exception() {
    let t = Test::new().await;
    t.mock
        .when_any_exchange_received(Arc::new(processor_fn(|_| {
            Err(CamelError::illegal_argument("Forced"))
        })));
    assert_forced(
        t.template
            .send_body("direct:a", "Hello World")
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn test_simulate_error_with_index() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.when_exchange_received(
        2,
        Arc::new(processor_fn(|e: &mut Exchange| {
            e.set_exception(Some(CamelError::illegal_argument("Forced")));
            Ok(())
        })),
    );
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    assert_forced(
        t.template
            .send_body("direct:a", "Hello World")
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn test_simulate_error_with_index_by_throwing_exception() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.when_exchange_received(
        2,
        Arc::new(processor_fn(|_| {
            Err(CamelError::illegal_argument("Forced"))
        })),
    );
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    assert_forced(
        t.template
            .send_body("direct:a", "Bye World")
            .await
            .unwrap_err(),
    );
}

#[tokio::test]
async fn test_minimum_count() {
    let t = Test::new().await;
    t.mock.expected_minimum_message_count(2);
    t.send_messages(&[3, 4, 5]).await;
    t.mock.assert_is_satisfied().await;
    assert_eq!(2, t.mock.expected_minimum_count());
}

#[tokio::test]
async fn test_resolve() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.set_result_wait_time(100);
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.mock.assert_is_not_satisfied().await;
    assert_eq!(100, t.mock.result_wait_time());
}

#[tokio::test]
async fn test_resolve_timeout() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.set_result_wait_time(100);
    t.mock.assert_is_not_satisfied_with(500).await;
    assert_eq!(2, t.mock.expected_count());
    assert_eq!(100, t.mock.result_wait_time());
}

#[tokio::test]
async fn test_sleep_for_empty_test() {
    let t = Test::new().await;
    t.mock.expected_message_count(0);
    t.mock.set_sleep_for_empty_test(100);
    t.mock.assert_is_satisfied().await;
    assert_eq!(0, t.mock.expected_count());
    assert_eq!(100, t.mock.sleep_for_empty_test());
}

#[tokio::test]
async fn test_sleep_for_empty_test_assert() {
    let t = Test::new().await;
    t.mock.expected_message_count(0);
    t.mock.assert_is_satisfied_with(100).await;
    assert_eq!(0, t.mock.expected_count());
    assert_eq!(0, t.mock.sleep_for_empty_test());
    assert_eq!(0, t.mock.result_wait_time());
}

#[tokio::test]
async fn test_reporter() {
    let t = Test::new().await;
    let reported = Arc::new(AtomicBool::new(false));
    let r = reported.clone();
    t.mock.expected_message_count(1);
    t.mock.set_reporter(Arc::new(processor_fn(move |_| {
        r.store(true, Ordering::SeqCst);
        Ok(())
    })));
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
    assert!(t.mock.has_reporter());
    assert!(reported.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_no_arg_ctr() {
    let t = Test::new().await;
    let mock = MockEndpoint::new("mock:bar", "bar", Arc::downgrade(&t.context.as_shared()));
    let e: Arc<dyn camel_api::endpoint::Endpoint> = Arc::new(mock.clone());
    assert!(e
        .create_consumer(Arc::new(processor_fn(|_| Ok(()))))
        .is_err());
    assert_eq!(0, mock.failures().len());
}

#[tokio::test]
async fn test_header_missing() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_header_received("foo", 123);
    t.mock.expected_header_received("bar", "cheese");
    t.template
        .send_body_and_header("direct:a", "Hello World", "foo", 123)
        .await
        .unwrap();
    assert_eq!(
        "mock://result No header with name bar found for message: 0",
        t.assert_err().await
    );
}

#[tokio::test]
async fn test_header_no_message_sent() {
    let t = Test::new().await;
    t.mock.expected_header_received("foo", 123);
    t.mock.set_result_wait_time(5);
    assert_eq!(
        "mock://result Received message count 0, expected at least 1",
        t.assert_err().await
    );
}

#[tokio::test]
async fn test_header_invalid_value() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_header_received("bar", "cheese");
    t.template
        .send_body_and_header("direct:a", "Hello World", "bar", "beer")
        .await
        .unwrap();
    assert_eq!(
        "mock://result Header with name bar for message: 0. Expected: <cheese> but was: <beer>",
        t.assert_err().await
    );
}

#[tokio::test]
async fn test_property_missing() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_property_received("foo", 123);
    t.mock.expected_property_received("bar", "cheese");
    t.send_with(|e| {
        e.message_mut().set_body("Hello World");
        e.set_property("foo", 123);
    })
    .await;
    assert_eq!(
        "mock://result No property with name bar found for message: 0",
        t.assert_err().await
    );
}

// testPropertyExpectedNull: message(0).exchangeProperty("foo").isNull()
#[tokio::test]
async fn test_property_expected_null() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.message_matches(
        0,
        Arc::new(predicate_fn(|e: &Exchange| {
            Ok(e.property("foo").is_none_or(Value::is_null))
        })),
    );
    t.send_with(|e| e.set_property("foo", 123)).await;
    t.mock.assert_is_not_satisfied().await;
    t.reset_mocks();
    t.mock.expected_message_count(1);
    t.mock.message_matches(
        0,
        Arc::new(predicate_fn(|e: &Exchange| {
            Ok(e.property("foo").is_none_or(Value::is_null))
        })),
    );
    t.send_with(|e| e.set_property("foo", Value::Null)).await;
    t.mock.assert_is_satisfied().await;
    t.reset_mocks();
    t.send_with(|_| {}).await;
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_property_invalid_value() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_property_received("bar", "cheese");
    t.send_with(|e| {
        e.message_mut().set_body("Hello World");
        e.set_property("bar", "beer");
    })
    .await;
    assert_eq!(
        "mock://result Property with name bar for message: 0. Expected: <cheese> but was: <beer>",
        t.assert_err().await
    );
}

fn pattern_is(p: ExchangePattern) -> Arc<dyn camel_api::processor::Predicate> {
    Arc::new(predicate_fn(move |e: &Exchange| Ok(e.pattern() == p)))
}

// testExchangePattern: message(i).exchangePattern().isEqualTo(..)
#[tokio::test]
async fn test_exchange_pattern() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, pattern_is(ExchangePattern::InOnly));
    t.mock
        .message_matches(1, pattern_is(ExchangePattern::InOut));
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.template
        .request_body("direct:a", "Bye World")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_not_exchange_pattern() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, pattern_is(ExchangePattern::InOnly));
    t.mock
        .message_matches(1, pattern_is(ExchangePattern::InOnly));
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.template
        .request_body("direct:a", "Bye World")
        .await
        .unwrap();
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_expected_exchange_pattern() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.expected_exchange_pattern(ExchangePattern::InOnly);
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.template.send_body("direct:a", "Bye World").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
    t.reset_mocks();
    t.mock.expected_message_count(1);
    t.mock.expected_exchange_pattern(ExchangePattern::InOut);
    t.template
        .request_body("direct:a", "Bye World")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

fn headers(pairs: &[(&str, Value)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[tokio::test]
async fn test_set_multiple_expected_headers() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_header_received("foo", 123);
    t.mock.expected_header_received("bar", "beer");
    let map = headers(&[("foo", Value::from(123)), ("bar", Value::from("beer"))]);
    t.template
        .send_body_and_headers("direct:a", "Hello World", map)
        .await
        .unwrap();
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_set_multiple_expected_headers2() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.expected_header_received("foo", 123);
    t.mock.expected_header_received("bar", "beer");
    let map = headers(&[("foo", Value::from(123)), ("bar", Value::from("beer"))]);
    t.template
        .send_body_and_headers("direct:a", "Hello World", map.clone())
        .await
        .unwrap();
    t.template
        .send_body_and_headers("direct:a", "Hello World", map)
        .await
        .unwrap();
    t.mock.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_set_multiple_expected_headers3() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_header_received("foo", 123);
    t.mock.expected_header_received("bar", Value::Null);
    t.template
        .send_body_and_headers(
            "direct:a",
            "Hello World",
            headers(&[("foo", Value::from(123))]),
        )
        .await
        .unwrap();
    t.mock.assert_is_satisfied().await;
}

// testSetMultipleExpectedHeaders4: the InputStream header is bytes here
#[tokio::test]
async fn test_set_multiple_expected_headers4() {
    let t = Test::new().await;
    t.mock.expected_message_count(1);
    t.mock.expected_header_received("foo", 123);
    t.mock.expected_header_received("bar", "Test");
    let map = headers(&[
        ("foo", Value::from(123)),
        ("bar", Value::Bytes(b"Test".to_vec())),
    ]);
    t.template
        .send_body_and_headers("direct:a", "Hello World", map)
        .await
        .unwrap();
    t.mock.assert_is_satisfied().await;
}

async fn multiple_headers_should_fail(count: i32, sends: Vec<Vec<(String, Value)>>) {
    let t = Test::new().await;
    t.mock.expected_message_count(count);
    t.mock.expected_header_received("foo", 123);
    t.mock.expected_header_received("bar", "beer");
    for map in sends {
        t.template
            .send_body_and_headers("direct:a", "Hello World", map)
            .await
            .unwrap();
    }
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_set_multiple_expected_headers_should_fail() {
    multiple_headers_should_fail(
        1,
        vec![headers(&[
            ("foo", Value::from(456)),
            ("bar", Value::from("beer")),
        ])],
    )
    .await;
}

#[tokio::test]
async fn test_set_multiple_expected_headers_should_fail2() {
    multiple_headers_should_fail(
        1,
        vec![headers(&[
            ("foo", Value::from(123)),
            ("bar", Value::from("wine")),
        ])],
    )
    .await;
}

#[tokio::test]
async fn test_set_multiple_expected_headers_should_fail3() {
    multiple_headers_should_fail(
        2,
        vec![
            headers(&[("foo", Value::from(123)), ("bar", Value::from("beer"))]),
            headers(&[("foo", Value::from(123)), ("bar", Value::from("wine"))]),
        ],
    )
    .await;
}

async fn multiple_properties(
    count: i32,
    sends: Vec<(i64, Option<&'static str>)>,
    satisfied: bool,
    bar: Value,
) {
    let t = Test::new().await;
    t.mock.expected_message_count(count);
    t.mock.expected_property_received("foo", 123);
    t.mock.expected_property_received("bar", bar);
    for (foo_value, bar) in sends {
        t.send_with(|e| {
            e.set_property("foo", foo_value);
            if let Some(b) = bar {
                e.set_property("bar", b);
            }
        })
        .await;
    }
    if satisfied {
        t.mock.assert_is_satisfied().await;
    } else {
        t.mock.assert_is_not_satisfied().await;
    }
}

#[tokio::test]
async fn test_set_multiple_expected_properties() {
    multiple_properties(1, vec![(123, Some("beer"))], true, Value::from("beer")).await;
}

#[tokio::test]
async fn test_set_multiple_expected_properties2() {
    multiple_properties(
        2,
        vec![(123, Some("beer")), (123, Some("beer"))],
        true,
        Value::from("beer"),
    )
    .await;
}

#[tokio::test]
async fn test_set_multiple_expected_properties3() {
    multiple_properties(1, vec![(123, None)], true, Value::Null).await;
}

#[tokio::test]
async fn test_set_multiple_expected_properties_should_fail() {
    multiple_properties(1, vec![(456, Some("beer"))], false, Value::from("beer")).await;
}

#[tokio::test]
async fn test_set_multiple_expected_properties_should_fail2() {
    multiple_properties(1, vec![(123, Some("wine"))], false, Value::from("beer")).await;
}

#[tokio::test]
async fn test_set_multiple_expected_properties_should_fail3() {
    multiple_properties(
        2,
        vec![(123, Some("beer")), (123, Some("wine"))],
        false,
        Value::from("beer"),
    )
    .await;
}

#[tokio::test]
async fn test_expected_body_type_coerce() {
    let t = Test::new().await;
    t.mock.expected_bodies_received([987]);
    t.template.send_body("direct:a", "0987").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_reset_default_processor() {
    let t = Test::new().await;
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    t.mock
        .when_any_exchange_received(Arc::new(processor_fn(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })));
    t.mock.expected_message_count(1);
    t.send_messages(&[1]).await;
    t.mock.assert_is_satisfied().await;
    assert_eq!(1, counter.load(Ordering::SeqCst));
    t.reset_mocks();
    t.mock.expected_message_count(1);
    t.send_messages(&[1]).await;
    t.mock.assert_is_satisfied().await;
    assert_eq!(1, counter.load(Ordering::SeqCst));
}

async fn retained(first: i32, last: i32, count: i64) -> (Test, Vec<String>) {
    let t = Test::new().await;
    if first != 0 {
        t.mock.set_retain_first(first);
    }
    if last != 0 {
        t.mock.set_retain_last(last);
    }
    t.mock.expected_message_count(count as i32);
    let all: Vec<i64> = (0..count).collect();
    t.send_messages(&all).await;
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
    assert_eq!(count as usize, t.mock.received_counter());
    let bodies = t
        .mock
        .received_exchanges()
        .iter()
        .map(|e| e.in_message().body().to_string())
        .collect();
    (t, bodies)
}

#[tokio::test]
async fn test_retain_first() {
    let (_, bodies) = retained(5, 0, 10).await;
    assert_eq!(messages(&[0, 1, 2, 3, 4]), bodies);
}

#[tokio::test]
async fn test_retain_last() {
    let (_, bodies) = retained(0, 5, 10).await;
    assert_eq!(messages(&[5, 6, 7, 8, 9]), bodies);
}

#[tokio::test]
async fn test_retain_first_and_last() {
    let (_, bodies) = retained(5, 5, 20).await;
    assert_eq!(messages(&[0, 1, 2, 3, 4, 15, 16, 17, 18, 19]), bodies);
}

#[tokio::test]
async fn test_retain_first_and_last_overlap() {
    let (_, bodies) = retained(5, 5, 8).await;
    assert_eq!(messages(&[0, 1, 2, 3, 4, 5, 6, 7]), bodies);
}

#[tokio::test]
async fn test_retain_first_and_last_no_gap() {
    let (_, bodies) = retained(5, 5, 10).await;
    assert_eq!(messages(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), bodies);
}

#[tokio::test]
async fn test_retain_first_and_last_single_gap() {
    let (_, bodies) = retained(5, 5, 11).await;
    assert_eq!(messages(&[0, 1, 2, 3, 4, 6, 7, 8, 9, 10]), bodies);
}

// testExpectedMessagesMatches with a closure predicate instead of the simple language
#[tokio::test]
async fn test_expected_messages_matches() {
    let t = Test::new().await;
    let is_abc = || -> Arc<dyn camel_api::processor::Predicate> {
        Arc::new(predicate_fn(|e: &Exchange| {
            Ok(e.message().body().as_str() == Some("abc"))
        }))
    };
    t.mock.expected_messages_matches(vec![is_abc()]);
    t.template.send_body("direct:a", "abc").await.unwrap();
    t.mock.assert_is_satisfied().await;
    t.mock.reset();
    t.mock.expected_messages_matches(vec![is_abc()]);
    t.template.send_body("direct:a", "def").await.unwrap();
    t.mock.assert_is_not_satisfied().await;
}

async fn clause_error(t: &Test) -> String {
    t.mock
        .try_assert_is_satisfied()
        .await
        .unwrap_err()
        .message()
        .to_string()
}

#[tokio::test]
async fn test_message_index_is_equal_to() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.message_matches(0, header("foo").is_equal_to(123));
    t.mock.message_matches(1, header("bar").is_equal_to(444));
    t.template
        .send_body_and_header("direct:a", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:a", "Hello World", "bar", 234)
        .await
        .unwrap();
    let e = clause_error(&t).await;
    let s = "Assertion error at index 1 on mock mock://result with predicate: header(bar) == 444 evaluated as: 234 == 444";
    assert!(e.starts_with(s), "{e}");
}

#[tokio::test]
async fn test_predicate_evaluation_is_null() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.message_matches(0, header("foo").is_not_null());
    t.mock.message_matches(1, header("bar").is_null());
    t.template
        .send_body_and_header("direct:a", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:a", "Hello World", "bar", 234)
        .await
        .unwrap();
    let e = clause_error(&t).await;
    let s = "Assertion error at index 1 on mock mock://result with predicate: header(bar) is null evaluated as: 234 is null";
    assert!(e.starts_with(s), "{e}");
}

#[tokio::test]
async fn test_predicate_evaluation_is_instance_of() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock.message_matches(0, header("foo").is_not_null());
    t.mock
        .message_matches(1, header("bar").is_instance_of("String"));
    t.template
        .send_body_and_header("direct:a", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:a", "Hello World", "bar", 234)
        .await
        .unwrap();
    let e = clause_error(&t).await;
    let s = "Assertion error at index 1 on mock mock://result with predicate: header(bar) instanceof java.lang.String";
    assert!(e.starts_with(s), "{e}");
}

#[tokio::test]
async fn test_exchange_pattern_clause() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, exchange_pattern().is_equal_to("InOnly"));
    t.mock
        .message_matches(1, exchange_pattern().is_equal_to("InOut"));
    t.template
        .send_body("direct:a", "Hello World")
        .await
        .unwrap();
    t.template
        .request_body("direct:a", "Bye World")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

async fn two_messages(t: &Test) {
    t.template
        .send_body_and_header("direct:a", "Hello World", "foo", 123)
        .await
        .unwrap();
    t.template
        .send_body_and_header("direct:a", "Bye World", "bar", 234)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_body_predicate() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, body_value().is_equal_to(constant("Hello World")));
    t.mock
        .message_matches(1, body_value().is_equal_to(constant("Bye World")));
    two_messages(&t).await;
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_not_body_predicate() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, body_value().is_equal_to(constant("Hello World")));
    t.mock
        .message_matches(1, body_value().is_equal_to(constant("Hi World")));
    two_messages(&t).await;
    t.mock.assert_is_not_satisfied().await;
}

#[tokio::test]
async fn test_header_predicate() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, header("foo").is_equal_to(constant(123)));
    t.mock
        .message_matches(1, header("bar").is_equal_to(constant(234)));
    two_messages(&t).await;
    MockEndpoint::assert_is_satisfied_all(t.context.endpoints()).await;
}

#[tokio::test]
async fn test_not_header_predicate() {
    let t = Test::new().await;
    t.mock.expected_message_count(2);
    t.mock
        .message_matches(0, header("foo").is_equal_to(constant(123)));
    t.mock
        .message_matches(1, header("bar").is_equal_to(constant(666)));
    two_messages(&t).await;
    t.mock.assert_is_not_satisfied().await;
}
