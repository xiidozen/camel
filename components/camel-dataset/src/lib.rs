//! # camel-dataset
//!
//! The dataset components (Java `DataSetComponent` and `DataSetTestComponent`) for load and
//! soak testing:
//!
//! - `dataset:name` refers to a [`DataSet`] bean of the registry (see [`data_set_value`]): its
//!   consumer sends the messages of the dataset (each with the `CamelDataSetIndex` header), and
//!   its producer is a mock endpoint expecting them back (in order, checked as they arrive).
//! - `dataset-test:uri` is a mock endpoint expecting the messages polled from the endpoint of the
//!   uri (such as a file, or a seda queue), optionally split in lines.
//!
//! This crate is the Rust port of the Java `components/camel-dataset` module.
//!
//! # `dataset` options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `dataSetIndex` | `lenient` | Controls the behaviour of the CamelDataSetIndex header: `off` (not set nor checked), `lenient` (checked when present), `strict` (set and required). |
//! | `initialDelay` | `1000` | Time period in millis to wait before starting sending messages. |
//! | `preloadSize` | `0` | Sets how many messages should be preloaded (sent) before the route completes its initialization. |
//! | `produceDelay` | `3` | Allows a delay to be specified which causes a delay when a message is sent by the consumer (to simulate slow processing). |
//! | `consumeDelay` | `0` | Allows a delay to be specified which causes a delay when a message is consumed by the producer (to simulate slow processing). |
//! | `minRate` | `0` | Wait until the DataSet contains at least this number of messages (accepted). |
//!
//! # `dataset-test` options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `anyOrder` | `false` | Whether the expected messages should arrive in the same order or can be in any order. |
//! | `split` | `false` | If enabled the messages loaded from the test endpoint will be split using new line delimiters so each line is an expected message. |
//! | `delimiter` | `\n|\r` | The split delimiter to use when split is enabled (a regular expression). |
//! | `timeout` | `2000` | The timeout to use when polling for message bodies from the URI. |

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod endpoint;
mod test_endpoint;

pub use endpoint::{DataSetComponent, DataSetConfig, DataSetExtension, DataSetIndex};
pub use test_endpoint::{DataSetTestComponent, DataSetTestExtension};

use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_util::value::{ObjectRef, Value};
use std::sync::Arc;

/// A dataset (Java `DataSet`): the messages sent by the consumer of a `dataset` endpoint and
/// expected by its producer.
pub trait DataSet: Send + Sync {
    /// Populates the message of the exchange with the message of the index.
    ///
    /// # Errors
    ///
    /// The message cannot be created.
    fn populate_message(&self, exchange: &mut Exchange, message_index: i64) -> CamelResult<()>;

    /// The number of messages.
    fn size(&self) -> i64;

    /// Asserts that the actual exchange is the expected one (Java `assertMessageExpected`):
    /// by default the bodies are equal (see [`assert_body_expected`]).
    ///
    /// # Errors
    ///
    /// The messages differ.
    fn assert_message_expected(
        &self,
        expected: &Exchange,
        actual: &Exchange,
        message_index: i64,
    ) -> CamelResult<()> {
        let _ = message_index;
        assert_body_expected(expected, actual)
    }

    /// The number of messages between the throughput reports (by default a fifth of the
    /// size).
    fn report_count(&self) -> i64 {
        let n = self.size() / 5;
        if n == 0 {
            self.size()
        } else {
            n
        }
    }
}

/// A shared [`DataSet`].
pub type SharedDataSet = Arc<dyn DataSet>;

/// The dataset as a value, to bind it in the registry.
pub fn data_set_value(data_set: SharedDataSet) -> Value {
    Value::Object(ObjectRef::new(data_set))
}

/// The dataset held by a value of the registry (see [`data_set_value`]).
pub fn data_set_of(value: &Value) -> Option<SharedDataSet> {
    match value {
        Value::Object(o) => o.downcast_ref::<SharedDataSet>().cloned(),
        _ => None,
    }
}

/// Whether the values are equal, the actual one converted like the expected one (Java
/// `getMandatoryBody(expectedBody.getClass())`).
pub fn values_equal(expected: &Value, actual: &Value) -> bool {
    if expected == actual {
        return true;
    }
    match (expected, actual) {
        (Value::Null, _) | (_, Value::Null) => false,
        (Value::Int(e), a) => a.to_string().trim().parse::<i64>().ok() == Some(*e),
        (Value::Text(e), Value::Bytes(b)) => e.as_bytes() == b.as_slice(),
        (Value::Text(e), a) => *e == a.to_string(),
        _ => expected.to_string() == actual.to_string(),
    }
}

/// Asserts that the bodies are equal (Java `DataSetSupport.assertMessageExpected`).
///
/// # Errors
///
/// An assertion failure.
pub fn assert_body_expected(expected: &Exchange, actual: &Exchange) -> CamelResult<()> {
    let e = expected.message().body();
    if e.is_null() {
        return Ok(());
    }
    assert_equals("message body", e, actual.message().body(), actual)
}

/// Asserts that the values are equal (Java `DataSetEndpoint.assertEquals`).
///
/// # Errors
///
/// An assertion failure.
pub fn assert_equals(
    description: &str,
    expected: &Value,
    actual: &Value,
    exchange: &Exchange,
) -> CamelResult<()> {
    if values_equal(expected, actual) {
        return Ok(());
    }
    let headers: Vec<String> = exchange
        .message()
        .headers()
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    // a Java AssertionError
    Err(CamelError::runtime(format!(
        "{description} does not match. Expected: {expected} but was: {actual} on {exchange} with headers: {{{}}}",
        headers.join(", ")
    )))
}

/// A transformer of the populated messages (Java `DataSetSupport.outputTransformer`).
pub type OutputTransformer = Arc<dyn Fn(&mut Exchange) -> CamelResult<()> + Send + Sync>;

/// The common options of the datasets (Java `DataSetSupport`): the size, the report count,
/// the default headers and an output transformer.
#[derive(Clone)]
pub struct DataSetSupport {
    size: i64,
    report_count: i64,
    default_headers: Vec<(String, Value)>,
    output_transformer: Option<OutputTransformer>,
}

impl DataSetSupport {
    /// The options with the size.
    pub fn new(size: i64) -> Self {
        DataSetSupport {
            size,
            report_count: -1,
            default_headers: Vec::new(),
            output_transformer: None,
        }
    }

    /// The size.
    pub fn size(&self) -> i64 {
        self.size
    }

    /// The report count (by default a fifth of the size).
    pub fn report_count(&self) -> i64 {
        if self.report_count > 0 {
            return self.report_count;
        }
        let n = self.size / 5;
        if n == 0 {
            self.size
        } else {
            n
        }
    }

    /// Populates the message with the body, the default headers and the transformer (Java
    /// `DataSetSupport.populateMessage`).
    ///
    /// # Errors
    ///
    /// The error of the transformer.
    pub fn populate(&self, exchange: &mut Exchange, body: Value) -> CamelResult<()> {
        let m = exchange.message_mut();
        m.set_body(body);
        m.set_headers(Default::default());
        for (k, v) in &self.default_headers {
            m.set_header(k, v.clone());
        }
        if let Some(t) = &self.output_transformer {
            t(exchange)?;
        }
        Ok(())
    }
}

macro_rules! support_setters {
    ($t:ty) => {
        impl $t {
            /// With the number of messages.
            pub fn with_size(mut self, size: i64) -> Self {
                self.support.size = size;
                self
            }

            /// With the number of messages between the throughput reports.
            pub fn with_report_count(mut self, count: i64) -> Self {
                self.support.report_count = count;
                self
            }

            /// With a default header of the messages.
            pub fn with_default_header(mut self, name: &str, value: impl Into<Value>) -> Self {
                self.support
                    .default_headers
                    .push((name.to_string(), value.into()));
                self
            }

            /// With the transformer of the populated messages.
            pub fn with_output_transformer(mut self, transformer: OutputTransformer) -> Self {
                self.support.output_transformer = Some(transformer);
                self
            }

            /// The common options.
            pub fn support(&self) -> &DataSetSupport {
                &self.support
            }
        }
    };
}

/// A dataset of the same body (Java `SimpleDataSet`), `<hello>world!</hello>` by default, 10
/// messages by default.
#[derive(Clone)]
pub struct SimpleDataSet {
    support: DataSetSupport,
    default_body: Value,
}

impl Default for SimpleDataSet {
    fn default() -> Self {
        Self::new(10)
    }
}

impl SimpleDataSet {
    /// The dataset of the size.
    pub fn new(size: i64) -> Self {
        SimpleDataSet {
            support: DataSetSupport::new(size),
            default_body: Value::from("<hello>world!</hello>"),
        }
    }

    /// With the body of the messages.
    pub fn with_default_body(mut self, body: impl Into<Value>) -> Self {
        self.default_body = body.into();
        self
    }

    /// The body of the messages.
    pub fn default_body(&self) -> &Value {
        &self.default_body
    }
}

support_setters!(SimpleDataSet);

impl DataSet for SimpleDataSet {
    fn populate_message(&self, exchange: &mut Exchange, _message_index: i64) -> CamelResult<()> {
        self.support.populate(exchange, self.default_body.clone())
    }

    fn size(&self) -> i64 {
        self.support.size
    }

    fn report_count(&self) -> i64 {
        self.support.report_count()
    }
}

/// A dataset of a list of bodies, used in turn (Java `ListDataSet`); the size is the number of
/// bodies.
#[derive(Clone)]
pub struct ListDataSet {
    support: DataSetSupport,
    default_bodies: Vec<Value>,
}

impl ListDataSet {
    /// The dataset of the bodies.
    pub fn new(bodies: Vec<Value>) -> Self {
        ListDataSet {
            support: DataSetSupport::new(bodies.len() as i64),
            default_bodies: bodies,
        }
    }

    /// The bodies.
    pub fn default_bodies(&self) -> &[Value] {
        &self.default_bodies
    }
}

support_setters!(ListDataSet);

impl DataSet for ListDataSet {
    fn populate_message(&self, exchange: &mut Exchange, message_index: i64) -> CamelResult<()> {
        if self.default_bodies.is_empty() {
            return Err(CamelError::illegal_state("The ListDataSet has no bodies"));
        }
        let i = (message_index.max(0) as usize) % self.default_bodies.len();
        self.support
            .populate(exchange, self.default_bodies[i].clone())
    }

    fn size(&self) -> i64 {
        self.support.size
    }

    fn report_count(&self) -> i64 {
        self.support.report_count()
    }
}

/// The dataset of the content of a file (Java `FileDataSet`): one body, or one body per part
/// between the delimiters (a regular expression, empty parts skipped).
pub struct FileDataSet;

impl FileDataSet {
    /// Loads the file.
    ///
    /// # Errors
    ///
    /// The file cannot be read, or the delimiter is not a valid regular expression.
    pub fn load(
        path: impl AsRef<std::path::Path>,
        delimiter: Option<&str>,
    ) -> CamelResult<ListDataSet> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| {
            CamelError::new(
                camel_api::error::ErrorKind::Io,
                format!("Cannot read file: {}: {e}", path.display()),
            )
        })?;
        let bodies = match delimiter {
            Some(d) => {
                let re = regex::Regex::new(d).map_err(|e| {
                    CamelError::illegal_argument(format!("Invalid delimiter: {d}: {e}"))
                })?;
                re.split(&text)
                    .filter(|p| !p.is_empty())
                    .map(Value::from)
                    .collect()
            }
            None => vec![Value::from(text)],
        };
        Ok(ListDataSet::new(bodies))
    }
}
