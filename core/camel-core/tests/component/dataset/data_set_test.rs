//! Ports of `ListDataSetProducerTest`, `FileDataSetConsumerTest`,
//! `FileDataSetConsumerWithSplitTest`, `FileDataSetProducerTest` and
//! `FileDataSetProducerWithSplitTest`.

use super::context_with;
use crate::support::TestDirectory;
use camel_core::api::exchange_constants::DATASET_INDEX;
use camel_core::prelude::*;
use camel_dataset::{DataSet, FileDataSet, ListDataSet};
use std::sync::Arc;

const PAYLOAD: &str =
    "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9\nLine 10\n";

/// Sends the bodies (with their index) through `direct://source` to the dataset endpoint,
/// expecting the count.
async fn produce(ds: ListDataSet, bodies: Vec<String>) {
    let count = bodies.len();
    let t = context_with(Arc::new(ds), |r: &mut Routes| {
        r.add(from("direct://source").to("dataset://foo"));
    })
    .await;
    t.context.start().await.unwrap();
    t.mock("dataset://foo").expected_message_count(count as i32);
    for (i, b) in bodies.into_iter().enumerate() {
        t.template
            .send_body_and_header("direct://source", b, DATASET_INDEX, i as i64)
            .await
            .unwrap();
    }
    t.assert_mock_endpoints_satisfied().await;
}

#[tokio::test]
async fn test_list_data_set_producer() {
    let hello = "<hello>world!</hello>";
    // the template to the dataset endpoint
    let t = context_with(
        Arc::new(ListDataSet::new(vec![Value::from(hello)])),
        |r: &mut Routes| {
            r.add(from("direct://source").to("dataset://foo"));
        },
    )
    .await;
    t.context.start().await.unwrap();
    t.template
        .send_body_and_header("dataset://foo", hello, DATASET_INDEX, 0)
        .await
        .unwrap();
    t.assert_mock_endpoints_satisfied().await;
    // a size greater than the list
    produce(
        ListDataSet::new(vec![Value::from(hello)]).with_size(10),
        vec![hello.to_string(); 10],
    )
    .await;
}

#[tokio::test]
async fn test_file_data_set_consumer() {
    for (delimiter, size) in [(None, 1), (Some("\n"), 10)] {
        let ds = FileDataSet::load("src/test/data/file-dataset-test.txt", delimiter).unwrap();
        assert_eq!(size, ds.size(), "Unexpected DataSet size");
        for ds in [ds.clone(), ds.with_size(20)] {
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
        }
    }
}

#[tokio::test]
async fn test_file_data_set_producer() {
    let dir = TestDirectory::new("file-dataset");
    dir.write("file-dataset-test.txt", PAYLOAD);
    let file = dir.test_file("file-dataset-test.txt");
    let ds = FileDataSet::load(&file, None).unwrap();
    assert_eq!(1, ds.size());
    produce(ds.clone(), vec![PAYLOAD.to_string()]).await;
    produce(ds.with_size(20), vec![PAYLOAD.to_string(); 20]).await;

    let ds = FileDataSet::load(&file, Some("\n")).unwrap();
    assert_eq!(10, ds.size());
    produce(ds.clone(), (1..=10).map(|i| format!("Line {i}")).collect()).await;
    produce(
        ds.with_size(20),
        (0..20).map(|i| format!("Line {}", 1 + i % 10)).collect(),
    )
    .await;
}
