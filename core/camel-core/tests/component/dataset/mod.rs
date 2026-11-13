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
