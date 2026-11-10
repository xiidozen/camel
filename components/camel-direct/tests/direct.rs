//! Tests of the direct component, ported from the Java camel-core tests in
//! `org.apache.camel.component.direct` (DirectRouteTest, DirectNoConsumerTest,
//! DirectNoMultipleConsumersTest, DirectProducerBlockingTest,
//! SendToNonExistingDirectEndpointTest, TwoCamelContextDirectEndpointTest).
//!
//! The `mock:` endpoints of the Java tests are replaced by a recording processor until the
//! mock component is ported.

use camel_api::context::CamelContext;
use camel_api::error::{CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{processor_fn, SharedProcessor};
use camel_base_engine::context::DefaultCamelContext;
use camel_base_engine::route::RouteSpec;
use camel_core_processor::pipeline::Pipeline;
use camel_core_processor::send::SendProcessor;
use camel_direct::{DirectComponent, DirectEndpoint, DIRECT_CONSUMER_NOT_AVAILABLE};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn context(block: bool) -> Arc<DefaultCamelContext> {
    let context = DefaultCamelContext::new();
    let direct = DirectComponent::new();
    direct.set_block(block);
    context.add_component("direct", Arc::new(direct)).unwrap();
    context
}

fn to(context: &DefaultCamelContext, uri: &str) -> SharedProcessor {
    Arc::new(SendProcessor::new(context.endpoint(uri).unwrap(), None))
}

fn pipeline(steps: Vec<SharedProcessor>) -> SharedProcessor {
    Arc::new(Pipeline::new(steps))
}

/// Records the bodies it receives (stands in for a mock endpoint).
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn processor(&self) -> SharedProcessor {
        let bodies = self.0.clone();
        Arc::new(processor_fn(move |e: &mut Exchange| {
            bodies
                .lock()
                .unwrap()
                .push(e.message().body_as::<String>()?.unwrap_or_default());
            Ok(())
        }))
    }

    fn bodies(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn assert_no_consumer<T: std::fmt::Debug>(result: CamelResult<T>) {
    let e = result.unwrap_err();
    assert!(e.is(ErrorKind::CamelExecution), "{e:?}");
    let cause = e
        .source()
        .and_then(|s| s.downcast_ref::<camel_api::error::CamelError>())
        .expect("a CamelError cause");
    assert!(
        cause.is_subtype("DirectConsumerNotAvailableException"),
        "{cause:?}"
    );
    assert_eq!(Some(DIRECT_CONSUMER_NOT_AVAILABLE), cause.subtype());
    assert!(cause.is(ErrorKind::CamelExchange));
    assert!(cause.exchange_id().is_some());
}

use std::error::Error;

// DirectRouteTest
#[tokio::test]
async fn test_direct_route() {
    let context = context(true);
    let invoked = Recorder::default();
    context
        .add_route(RouteSpec::new(
            "direct:test.a",
            to(&context, "direct:test.b"),
        ))
        .await
        .unwrap();
    context
        .add_route(RouteSpec::new("direct:test.b", invoked.processor()))
        .await
        .unwrap();
    context.start().await.unwrap();

    let endpoint = context.endpoint("direct:test.a").unwrap();
    let mut exchange = endpoint.create_exchange();
    exchange.message_mut().set_header("cheese", 123);
    let producer = endpoint.create_producer().unwrap();
    producer.process(&mut exchange).await.unwrap();
    assert_eq!(1, invoked.bodies().len(), "Did not receive the message!");
    assert_eq!(Some("route1"), exchange.from_route_id());
    context.stop().await.unwrap();
}

// DirectNoConsumerTest
#[tokio::test]
async fn test_in_only() {
    let context = context(false);
    context
        .add_route(RouteSpec::new("direct:start", to(&context, "direct:foo")))
        .await
        .unwrap();
    context.start().await.unwrap();
    assert_no_consumer(
        context
            .create_producer_template()
            .send_body("direct:start", "Hello World")
            .await,
    );
}

#[tokio::test]
async fn test_in_out() {
    let context = context(false);
    context
        .add_route(RouteSpec::new("direct:start", to(&context, "direct:foo")))
        .await
        .unwrap();
    context.start().await.unwrap();
    assert_no_consumer(
        context
            .create_producer_template()
            .request_body("direct:start", "Hello World")
            .await,
    );
}

#[tokio::test]
async fn test_fail_if_no_consumer_false() {
    let context = context(false);
    context
        .add_route(RouteSpec::new(
            "direct:start",
            to(&context, "direct:foo?failIfNoConsumers=false"),
        ))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
        .create_producer_template()
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
}

#[tokio::test]
async fn test_fail_if_no_consumers_after_consumers_leave() {
    let context = context(false);
    let mock = Recorder::default();
    context
        .add_route(RouteSpec::new("direct:foo", mock.processor()).with_id("stopThisRoute"))
        .await
        .unwrap();
    context.start().await.unwrap();
    let template = context.create_producer_template();
    template
        .send_body("direct:foo", "Hello World")
        .await
        .unwrap();
    assert_eq!(vec!["Hello World"], mock.bodies());

    context.stop_route("stopThisRoute").await.unwrap();
    assert_no_consumer(template.send_body("direct:foo", "Hello World").await);
}

#[tokio::test]
async fn test_fail_if_no_consumers_with_valid_consumer() {
    let context = context(false);
    let mock = Recorder::default();
    context
        .add_route(RouteSpec::new("direct:in", to(&context, "direct:foo")))
        .await
        .unwrap();
    context
        .add_route(RouteSpec::new("direct:foo", mock.processor()))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
        .create_producer_template()
        .send_body("direct:in", "Hello World")
        .await
        .unwrap();
    assert_eq!(vec!["Hello World"], mock.bodies());
}

#[tokio::test]
async fn test_fail_if_no_consumers_false_with_pipeline() {
    let context = context(false);
    let mock = Recorder::default();
    let steps = pipeline(vec![
        to(&context, "direct:foo?failIfNoConsumers=false"),
        to(&context, "direct:bar"),
    ]);
    context
        .add_route(RouteSpec::new("direct:in", steps))
        .await
        .unwrap();
    context
        .add_route(RouteSpec::new("direct:bar", mock.processor()))
        .await
        .unwrap();
    context.start().await.unwrap();
    context
        .create_producer_template()
        .send_body("direct:in", "Hello World")
        .await
        .unwrap();
    assert_eq!(vec!["Hello World"], mock.bodies());
}

#[tokio::test]
async fn test_config_on_a_consumer() {
    let context = context(false);
    context
        .add_route(RouteSpec::new(
            "direct:foo?failIfNoConsumers=false",
            Recorder::default().processor(),
        ))
        .await
        .unwrap();
    context.start().await.unwrap();
    let endpoint = context
        .endpoint("direct:foo?failIfNoConsumers=false")
        .unwrap();
    assert!(!endpoint
        .as_any()
        .downcast_ref::<DirectEndpoint>()
        .unwrap()
        .is_fail_if_no_consumers());
}

// DirectNoMultipleConsumersTest
#[tokio::test]
async fn test_no_multiple_consumers() {
    let context = context(true);
    let mock = Recorder::default();
    context
        .add_route(RouteSpec::new("direct:in", mock.processor()))
        .await
        .unwrap();
    context
        .add_route(RouteSpec::new("direct:in", mock.processor()))
        .await
        .unwrap();
    let e = context.start().await.unwrap_err();
    assert!(e.is(ErrorKind::FailedToStartRoute), "{e:?}");
    assert_eq!(
        Some("Failed to start route: route2 because: Cannot add a 2nd consumer to the same endpoint: direct://in. DirectEndpoint only allows one consumer."),
        e.message()
    );
    context.stop().await.unwrap();
}

// DirectProducerBlockingTest
async fn blocking_context() -> (Arc<DefaultCamelContext>, Recorder) {
    let context = context(true);
    let mock = Recorder::default();
    context
        .add_route(RouteSpec::new("direct:suspended", mock.processor()).with_id("foo"))
        .await
        .unwrap();
    context.start().await.unwrap();
    (context, mock)
}

#[tokio::test]
async fn test_producer_blocks_for_suspended_consumer() {
    let (context, _mock) = blocking_context().await;
    context.suspend_route("foo").await.unwrap();
    let watch = Instant::now();
    assert_no_consumer(
        context
            .create_producer_template()
            .send_body("direct:suspended?block=true&timeout=500", "hello world")
            .await,
    );
    assert!(watch.elapsed() > Duration::from_millis(490));
    assert!(watch.elapsed() < Duration::from_millis(5000));
}

#[tokio::test]
async fn test_producer_blocks_with_no_consumers() {
    let (context, _mock) = blocking_context().await;
    context.suspend_route("foo").await.unwrap();
    let watch = Instant::now();
    assert_no_consumer(
        context
            .create_producer_template()
            .send_body("direct:start?block=true&timeout=500", "hello world")
            .await,
    );
    assert!(watch.elapsed() > Duration::from_millis(490));
    assert!(watch.elapsed() < Duration::from_millis(5000));
}

#[tokio::test]
async fn test_producer_blocks_resume() {
    let (context, mock) = blocking_context().await;
    context.suspend_route("foo").await.unwrap();
    let resumer = context.clone();
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        resumer.resume_route("foo").await.unwrap();
    });
    context
        .create_producer_template()
        .send_body("direct:suspended?block=true&timeout=1000", "hello world")
        .await
        .unwrap();
    assert_eq!(1, mock.bodies().len());
    task.await.unwrap();
}

// SendToNonExistingDirectEndpointTest
#[tokio::test]
async fn test_send_to_non_existing_direct() {
    let context = context(false);
    context.start().await.unwrap();
    assert_no_consumer(
        context
            .create_producer_template()
            .send_body("direct:foo", "Hello World")
            .await,
    );
}

// TwoCamelContextDirectEndpointTest
#[tokio::test]
async fn test_two_camel_context_direct_endpoint() {
    let camel1 = context(true);
    let mock1 = Recorder::default();
    camel1
        .add_route(RouteSpec::new("direct:start", to(&camel1, "direct:foo")))
        .await
        .unwrap();
    camel1
        .add_route(RouteSpec::new("direct:foo", mock1.processor()))
        .await
        .unwrap();
    camel1.start().await.unwrap();
    let camel2 = context(true);
    let mock2 = Recorder::default();
    camel2
        .add_route(RouteSpec::new("direct:start", to(&camel2, "direct:foo")))
        .await
        .unwrap();
    camel2
        .add_route(RouteSpec::new("direct:foo", mock2.processor()))
        .await
        .unwrap();
    camel2.start().await.unwrap();

    assert!(!Arc::ptr_eq(
        &camel1.endpoint("direct:start").unwrap(),
        &camel2.endpoint("direct:start").unwrap()
    ));
    assert!(!Arc::ptr_eq(
        &camel1.endpoint("direct:foo").unwrap(),
        &camel2.endpoint("direct:foo").unwrap()
    ));

    camel1
        .create_producer_template()
        .send_body("direct:start", "Hello World")
        .await
        .unwrap();
    camel2
        .create_producer_template()
        .send_body("direct:start", "Bye World")
        .await
        .unwrap();
    assert_eq!(vec!["Hello World"], mock1.bodies());
    assert_eq!(vec!["Bye World"], mock2.bodies());
    camel1.stop().await.unwrap();
    camel2.stop().await.unwrap();
}

#[tokio::test]
async fn test_unknown_option() {
    let context = context(false);
    let e = context.endpoint("direct:foo?bar=1").err().unwrap();
    assert!(e.is(ErrorKind::ResolveEndpointFailed));
    assert!(
        e.message()
            .unwrap()
            .contains("Unknown parameters=[{bar=1}]"),
        "{e:?}"
    );
    // the same endpoint for equivalent uris
    let a = context.endpoint("direct:foo").unwrap();
    let b = context.endpoint("direct://foo").unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!("direct://foo", a.endpoint_uri());
}
