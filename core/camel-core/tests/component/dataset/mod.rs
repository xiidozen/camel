//! Ports of the dataset component tests.

use crate::support::ContextTestSupport;
use camel_core::api::exchange_constants::DATASET_INDEX;
use camel_core::prelude::*;
use camel_dataset::{
    data_set_value, DataSet, DataSetExtension, ListDataSet, SharedDataSet, SimpleDataSet,
};
use std::sync::Arc;

mod data_set_test;

/// A context with the dataset bound as `foo` and the routes, not started.
async fn context_with(
    data_set: SharedDataSet,
    routes: impl Fn(&mut Routes) + Send + Sync + 'static,
) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    t.context
        .registry()
        .unwrap()
        .bind("foo", data_set_value(data_set))
        .unwrap();
    t.context
        .add_routes(move |r: &mut Routes| routes(r))
        .await
        .unwrap();
    t
}

/// `DataSetTest`: from a dataset to another dataset endpoint.
#[tokio::test]
async fn test_data_set() {
    let t = context_with(Arc::new(SimpleDataSet::new(20)), |r: &mut Routes| {
        r.add(from("direct:foo").to("dataset:foo?minRate=50"));
        r.add(from("dataset:foo?initialDelay=0&minRate=50").to("direct:foo"));
    })
    .await;
    t.context.start().await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(20, t.mock("dataset:foo?minRate=50").received_counter());
}

/// `DataSetSedaTest` and `DataSetPreloadTest`: through a seda queue, the route started later.
#[tokio::test]
async fn test_data_set_seda_and_preload() {
    for (size, uri) in [
        (200, "dataset:foo?initialDelay=0&produceDelay=1"),
        (20, "dataset:foo?initialDelay=0&preloadSize=5"),
    ] {
        let t = context_with(Arc::new(SimpleDataSet::new(size)), move |r: &mut Routes| {
            r.add(from(uri).to("seda:test").auto_startup(false));
            r.add(from("seda:test").to(uri));
        })
        .await;
        t.context.start().await.unwrap();
        let endpoint = t.mock(uri);
        endpoint.expected_message_count(size as i32);
        t.context.start_all_routes().await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
        if uri.contains("preloadSize") {
            assert_eq!(
                5,
                DataSetExtension::of(&endpoint)
                    .unwrap()
                    .config()
                    .preload_size
            );
        }
    }
}

/// `CustomDataSetTest`: a dataset checking the index of the message (an xpath in Java, the
/// text here).
#[tokio::test]
async fn test_custom_data_set() {
    struct Custom;
    impl DataSet for Custom {
        fn populate_message(&self, e: &mut Exchange, i: i64) -> CamelResult<()> {
            e.message_mut()
                .set_body(format!("<message index='{i}'>someBody{i}</message>"));
            Ok(())
        }
        fn size(&self) -> i64 {
            10
        }
        fn assert_message_expected(
            &self,
            _: &Exchange,
            actual: &Exchange,
            index: i64,
        ) -> CamelResult<()> {
            let body = actual.message().body().to_string();
            if body.starts_with(&format!("<message index='{index}'>")) {
                Ok(())
            } else {
                Err(CamelError::runtime(format!(
                    "Actual: {body} is not the message {index}"
                )))
            }
        }
    }
    let t = context_with(Arc::new(Custom), |r: &mut Routes| {
        r.add(from("dataset:foo?initialDelay=0").to("direct:foo"));
        r.add(from("direct:foo").to("dataset:foo?initialDelay=0"));
    })
    .await;
    t.context.start().await.unwrap();
    t.assert_mock_endpoints_satisfied().await;
}

/// `ListDataSetConsumerTest`: the list in turn, also beyond its size.
#[tokio::test]
async fn test_list_data_set_consumer() {
    for size in [None, Some(10)] {
        let mut ds = ListDataSet::new(vec![Value::from("<hello>world!</hello>")]);
        if let Some(s) = size {
            ds = ds.with_size(s);
        }
        let n = ds.size();
        let t = context_with(Arc::new(ds), |r: &mut Routes| {
            r.add(from("dataset://foo").to("mock://result"));
        })
        .await;
        t.context.start().await.unwrap();
        let result = t.mock("mock://result");
        result.expected_minimum_message_count(n as i32);
        result.set_result_wait_time(5000);
        result.assert_is_satisfied().await;
        result.message_matches(0, header(DATASET_INDEX).is_not_null());
    }
}

/// `DataSetTestSedaTest` and `DataSetTestAnyOrderTest`: the expected bodies of a seda queue.
#[tokio::test]
async fn test_data_set_test_seda() {
    for any_order in [false, true] {
        let t = ContextTestSupport::new();
        t.context.start().await.unwrap();
        let queued: &[&str] = if any_order {
            &["Bye World", "Hello World"]
        } else {
            &["Hello World"]
        };
        for b in queued {
            t.template.send_body("seda:testme", *b).await.unwrap();
        }
        let uri = if any_order {
            "dataset-test:seda:testme?anyOrder=true&timeout=0"
        } else {
            "dataset-test:seda:testme?timeout=0"
        };
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from("direct:start").to(uri));
            })
            .await
            .unwrap();
        t.context.start_all_routes().await.unwrap();
        let sent: &[&str] = if any_order {
            &["Hello World", "Bye World"]
        } else {
            &["Hello World"]
        };
        for b in sent {
            t.template.send_body("direct:start", *b).await.unwrap();
        }
        t.assert_mock_endpoints_satisfied().await;
        assert_eq!(sent.len(), t.mock(uri).received_counter());
    }
}

/// `DataSetTestFileTest` and `DataSetTestFileSplitTest` (disabled in Java): the expected bodies
/// of a file, whole or split in lines.
#[tokio::test]
async fn test_data_set_test_file() {
    for split in [false, true] {
        let dir = crate::support::TestDirectory::new("dataset-test-file");
        let t = ContextTestSupport::new();
        t.context.start().await.unwrap();
        let content = if split {
            "Hello World\nBye World\nHi World"
        } else {
            "Hello World"
        };
        t.template
            .send_body_and_header(
                &dir.file_uri(""),
                content,
                camel_core::api::exchange_constants::FILE_NAME,
                "data.txt",
            )
            .await
            .unwrap();
        let uri = if split {
            format!(
                "dataset-test:{}?noop=true&split=true&timeout=1000",
                dir.file_uri("")
            )
        } else {
            format!("dataset-test:{}?noop=true&timeout=1500", dir.file_uri(""))
        };
        let u = uri.clone();
        t.context
            .add_routes(move |r: &mut Routes| {
                r.add(from("direct:start").to(&u));
            })
            .await
            .unwrap();
        t.context.start_all_routes().await.unwrap();
        let sent: &[&str] = if split {
            &["Hello World", "Bye World", "Hi World"]
        } else {
            &["Hello World"]
        };
        for b in sent {
            t.template.send_body("direct:start", *b).await.unwrap();
        }
        t.assert_mock_endpoints_satisfied().await;
        assert_eq!(sent.len(), t.mock(&uri).received_counter(), "split={split}");
    }
}

/// The `CamelDataSetIndex` header as a number.
fn index_expression() -> camel_core::api::processor::SharedExpression {
    Arc::new(camel_core::api::processor::expression_fn(|e: &Exchange| {
        Ok(e.message()
            .header(DATASET_INDEX)
            .and_then(|v| v.to_string().parse::<i64>().ok())
            .map(Value::Int)
            .unwrap_or_default())
    }))
}

/// `DataSetConsumerTest`.
#[tokio::test]
async fn test_data_set_consumer() {
    let uri = "dataset://foo?initialDelay=0";
    // a consumer only endpoint expects nothing; with a producer the size of the dataset
    for with_producer in [false, true] {
        let t = context_with(Arc::new(SimpleDataSet::new(5)), move |r: &mut Routes| {
            let route = from(uri);
            let route = if with_producer { route.to(uri) } else { route };
            r.add(route.to("mock://result"));
        })
        .await;
        let result = t.mock("mock://result");
        result.expected_message_count(5);
        result.expects_ascending(index_expression());
        t.context.start().await.unwrap();
        assert_eq!(
            if with_producer { 5 } else { -1 },
            t.mock(uri).expected_count()
        );
        t.assert_mock_endpoints_satisfied().await;
    }
    // the dataSetIndex modes
    for (mode, header_set) in [
        ("", true),
        ("&dataSetIndex=off", false),
        ("&dataSetIndex=lenient", true),
        ("&dataSetIndex=strict", true),
    ] {
        let u = format!("{uri}{mode}");
        let t = context_with(Arc::new(SimpleDataSet::new(5)), move |r: &mut Routes| {
            r.add(from(&u).to("mock://result"));
        })
        .await;
        let result = t.mock("mock://result");
        result.expected_message_count(5);
        if header_set {
            result.all_messages_match(header(DATASET_INDEX).is_not_null());
            result.expects_ascending(index_expression());
        } else {
            result.all_messages_match(header(DATASET_INDEX).is_null());
        }
        t.context.start().await.unwrap();
        t.assert_mock_endpoints_satisfied().await;
    }
}

const DS: &str = "dataset://foo";

/// The dataset endpoint of the uri is not satisfied, with the message.
async fn assert_not_satisfied(t: &ContextTestSupport, uri: &str, expected: &str) {
    let e = t
        .mock(uri)
        .try_assert_is_satisfied()
        .await
        .expect_err("should not be satisfied");
    assert!(
        e.to_string().contains(expected),
        "{e} should contain {expected}"
    );
}

/// The index header of the message of the index (`None`: no header).
type IndexOf = fn(i64) -> Option<i64>;

/// `DataSetProducerTest`: sending to the dataset endpoint, with or without the index header.
#[tokio::test]
async fn test_data_set_producer_explicit() {
    let ds = SimpleDataSet::new(20);
    let body = ds.default_body().clone();
    // (uri suffix, header for the index i: None for no header)
    let cases: Vec<(&str, IndexOf)> = vec![
        ("", |i| Some(i)),
        ("", |_| None),
        ("?dataSetIndex=lenient", |i| (i % 2 == 0).then_some(i)),
        ("?dataSetIndex=strict", |i| Some(i)),
        ("?dataSetIndex=off", |i| Some(20 - i)),
        ("?dataSetIndex=off", |_| None),
    ];
    for (suffix, index) in cases {
        let t = context_with(Arc::new(ds.clone()), |_: &mut Routes| {}).await;
        t.context.start().await.unwrap();
        let uri = format!("{DS}{suffix}");
        for i in 0..20 {
            match index(i) {
                Some(n) => t
                    .template
                    .send_body_and_header(&uri, body.clone(), DATASET_INDEX, n)
                    .await
                    .unwrap(),
                None => t.template.send_body(&uri, body.clone()).await.unwrap(),
            };
        }
        t.assert_mock_endpoints_satisfied().await;
    }
    // strict requires the header
    let t = context_with(Arc::new(ds.clone()), |_: &mut Routes| {}).await;
    t.context.start().await.unwrap();
    let uri = format!("{DS}?dataSetIndex=strict");
    for _ in 0..20 {
        t.template.send_body(&uri, body.clone()).await.unwrap();
    }
    assert_not_satisfied(
        &t,
        &uri,
        &format!("Caught exception on {uri} due to: No '{DATASET_INDEX}' header available of type: java.lang.Long"),
    )
    .await;
}

/// `DataSetProducerTest`: through a route, the index header set (or checked) by the endpoint.
#[tokio::test]
async fn test_data_set_producer_route() {
    let ds = SimpleDataSet::new(20);
    let body = ds.default_body().clone();
    for (suffix, with_header) in [
        ("", false),
        ("?dataSetIndex=off", true),
        ("?dataSetIndex=lenient", true),
        ("?dataSetIndex=strict", true),
    ] {
        let uri = format!("{DS}{suffix}");
        let u = uri.clone();
        let t = context_with(Arc::new(ds.clone()), move |r: &mut Routes| {
            r.add(from("direct://source").to(&u).to("mock://result"));
        })
        .await;
        t.context.start().await.unwrap();
        let result = t.mock("mock://result");
        result.expected_message_count(20);
        result.all_messages_match(header(DATASET_INDEX).is_not_null());
        result.expects_ascending(index_expression());
        for i in 0..20 {
            if with_header {
                t.template
                    .send_body_and_header("direct://source", body.clone(), DATASET_INDEX, i)
                    .await
                    .unwrap();
            } else {
                t.template
                    .send_body("direct://source", body.clone())
                    .await
                    .unwrap();
            }
        }
        t.assert_mock_endpoints_satisfied().await;
    }
}

/// `DataSetProducerTest`: an invalid index fails the dataset endpoint, but with
/// `dataSetIndex=off`.
#[tokio::test]
async fn test_data_set_producer_invalid_index() {
    let ds = SimpleDataSet::new(20);
    let body = ds.default_body().clone();
    for suffix in [
        "",
        "?dataSetIndex=off",
        "?dataSetIndex=lenient",
        "?dataSetIndex=strict",
    ] {
        let uri = format!("{DS}{suffix}");
        let u = uri.clone();
        let t = context_with(Arc::new(ds.clone()), move |r: &mut Routes| {
            r.add(from("direct://source").to(&u).to("mock://result"));
        })
        .await;
        t.context.start().await.unwrap();
        t.mock("mock://result").expected_message_count(20);
        for i in 0..20 {
            if i == 10 {
                t.template
                    .send_body_and_header("direct://source", body.clone(), DATASET_INDEX, i + 10)
                    .await
                    .unwrap();
            } else if suffix.contains("strict") {
                t.template
                    .send_body_and_header("direct://source", body.clone(), DATASET_INDEX, i)
                    .await
                    .unwrap();
            } else {
                t.template
                    .send_body("direct://source", body.clone())
                    .await
                    .unwrap();
            }
        }
        if suffix.contains("off") {
            t.assert_mock_endpoints_satisfied().await;
        } else {
            assert_not_satisfied(
                &t,
                &uri,
                &format!("Caught exception on {uri} due to: Header: {DATASET_INDEX} does not match. Expected: 10 but was: 20"),
            )
            .await;
        }
    }
}

/// `DataSetEndpointTest`: the options of the endpoint (by uri here, by setters in Java), the
/// reporter, and a dataset with an output transformer.
#[tokio::test]
async fn test_data_set_endpoint() {
    for uri in [
        "dataset://foo?initialDelay=0",
        "dataset://foo?consumeDelay=2&produceDelay=5&initialDelay=1",
    ] {
        let t = context_with(Arc::new(SimpleDataSet::new(2)), move |r: &mut Routes| {
            r.add(from(uri).to("direct:foo"));
            r.add(from("direct:foo").to(uri));
        })
        .await;
        let endpoint = t.mock(uri);
        let c = DataSetExtension::of(&endpoint).unwrap().config().clone();
        if uri.ends_with("initialDelay=0") {
            assert_eq!(
                (0, 0, 3),
                (c.preload_size, c.consume_delay, c.produce_delay)
            );
        } else {
            assert_eq!(
                (2, 5, 1),
                (c.consume_delay, c.produce_delay, c.initial_delay)
            );
        }
        t.context.start().await.unwrap();
        endpoint.assert_is_satisfied().await;
    }

    // the reporter
    let uri = "dataset://foo?initialDelay=0";
    let t = context_with(Arc::new(SimpleDataSet::new(10)), move |r: &mut Routes| {
        r.add(from(uri).to("direct:foo"));
        r.add(from("direct:foo").to(uri));
    })
    .await;
    let endpoint = t.mock(uri);
    let reported = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let rep = reported.clone();
    endpoint.set_reporter(Arc::new(camel_core::api::processor::processor_fn(
        move |_: &mut Exchange| {
            rep.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    )));
    t.context.start().await.unwrap();
    endpoint.assert_is_satisfied().await;
    assert!(reported.load(std::sync::atomic::Ordering::SeqCst));

    // SimpleDataSet
    let ds = SimpleDataSet::default()
        .with_size(2)
        .with_default_body("Hi");
    assert_eq!(&Value::from("Hi"), ds.default_body());

    // DataSetSupport with an output transformer
    struct MyDataSet(camel_dataset::DataSetSupport);
    impl DataSet for MyDataSet {
        fn populate_message(&self, e: &mut Exchange, i: i64) -> CamelResult<()> {
            self.0.populate(e, Value::from(format!("Message {i}")))
        }
        fn size(&self) -> i64 {
            self.0.size()
        }
    }
    let support = ListDataSet::new(vec![Value::Null; 4])
        .with_report_count(0)
        .with_output_transformer(Arc::new(|e: &mut Exchange| {
            let body = format!("Hi {}", e.message().body());
            e.message_mut().set_body(body);
            Ok(())
        }))
        .support()
        .clone();
    let t = context_with(Arc::new(MyDataSet(support)), move |r: &mut Routes| {
        r.add(from(uri).to("direct:foo"));
        r.add(from("direct:foo").to(uri));
    })
    .await;
    let endpoint = t.mock(uri);
    endpoint.all_messages_match(body().starts_with("Hi "));
    t.context.start().await.unwrap();
    endpoint.assert_is_satisfied().await;
    assert_eq!(4, endpoint.received_counter());
}

/// `RoutePerformanceTest`: 250 messages with a default header, through a choice.
#[tokio::test]
async fn test_route_performance() {
    let ds = SimpleDataSet::new(250).with_default_header("foo", 123);
    let t = context_with(Arc::new(ds), |r: &mut Routes| {
        r.add(from("dataset:foo?initialDelay=0&produceDelay=0").to("direct:start"));
        r.add(
            from("direct:start")
                .to("log:a?level=OFF")
                .to("log:b?level=OFF")
                .to("direct:c"),
        );
        r.add(from("direct:c").choice(|c| {
            c.when(header("foo").is_not_null(), |w| {
                w.to("mock:results").to("dataset:foo")
            })
            .otherwise(|o| o.to("mock:results").to("dataset:foo"))
        }));
    })
    .await;
    let endpoint = t.mock("mock:results");
    endpoint.expected_message_count(250);
    endpoint.expected_header_received("foo", 123);
    t.context.start().await.unwrap();
    endpoint.set_result_wait_time(30_000);
    t.assert_mock_endpoints_satisfied().await;
}

/// `DataSetTestEndpointTest`: the expected bodies polled (event driven) from an endpoint whose
/// consumer sends one message when started.
#[tokio::test]
async fn test_data_set_test_endpoint() {
    use camel_core::api::context::{CamelContext, StandaloneContext};
    use camel_core::api::endpoint::{
        Component, Consumer, Endpoint, SharedConsumer, SharedEndpoint, SharedProducer,
    };
    use camel_core::api::processor::{BoxFuture, SharedProcessor};
    use camel_core::api::service::Service;
    use std::sync::Weak;

    struct MyComponent;
    impl Service for MyComponent {}
    impl Component for MyComponent {
        fn create_endpoint(
            &self,
            context: &Arc<dyn CamelContext>,
            uri: &str,
            _: &str,
            _: &mut camel_util::uri_support::Parameters,
        ) -> CamelResult<SharedEndpoint> {
            Ok(Arc::new(MyEndpoint {
                uri: uri.to_string(),
                context: Arc::downgrade(context),
            }))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    struct MyEndpoint {
        uri: String,
        context: Weak<dyn CamelContext>,
    }
    impl Service for MyEndpoint {}
    impl Endpoint for MyEndpoint {
        fn endpoint_uri(&self) -> &str {
            &self.uri
        }
        fn camel_context(&self) -> Arc<dyn CamelContext> {
            self.context
                .upgrade()
                .unwrap_or_else(StandaloneContext::shared)
        }
        fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
            Err(CamelError::runtime("no producer"))
        }
        fn create_consumer(
            self: Arc<Self>,
            processor: SharedProcessor,
        ) -> CamelResult<SharedConsumer> {
            Ok(Arc::new(MyConsumer {
                endpoint: self,
                processor,
            }))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    struct MyConsumer {
        endpoint: Arc<MyEndpoint>,
        processor: SharedProcessor,
    }
    impl Service for MyConsumer {
        fn start(&self) -> BoxFuture<'_, CamelResult<()>> {
            Box::pin(async move {
                let mut e = self.endpoint.create_exchange();
                e.message_mut().set_body("Hello World");
                self.processor.process(&mut e).await
            })
        }
    }
    impl Consumer for MyConsumer {
        fn endpoint(&self) -> SharedEndpoint {
            self.endpoint.clone()
        }
        fn processor(&self) -> SharedProcessor {
            self.processor.clone()
        }
    }

    let t = ContextTestSupport::new();
    t.context
        .add_component("my", Arc::new(MyComponent))
        .unwrap();
    t.context
        .add_routes(|r: &mut Routes| {
            r.add(from("seda:foo").to("dataset-test:my:foo?timeout=0"));
        })
        .await
        .unwrap();
    t.context.start().await.unwrap();
    t.template
        .send_body("seda:foo", "Hello World")
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    assert_eq!(
        1,
        t.mock("dataset-test:my:foo?timeout=0").received_counter()
    );
}
