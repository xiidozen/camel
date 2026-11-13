//! Data formats (Java `DataFormat` and `DataFormatFactory`): marshal a message body to bytes
//! (such as an object to JSON) and unmarshal bytes back (the `marshal` and `unmarshal` EIPs and
//! the `dataformat` component).

use crate::context::CamelContext;
use crate::error::{CamelError, CamelResult};
use crate::exchange::Exchange;
use camel_util::value::{ObjectRef, Value};
use std::sync::{Arc, Weak};

/// A data format (Java `DataFormat`).
pub trait DataFormat: Send + Sync {
    /// The name of the data format, such as `base64` (Java `DataFormatName`); used in the
    /// messages.
    fn name(&self) -> &str;

    /// Marshals the object (the message body, or the variable sent) to the output (Java
    /// `marshal(exchange, graph, stream)`).
    ///
    /// # Errors
    ///
    /// The object cannot be marshalled.
    fn marshal(&self, exchange: &mut Exchange, graph: &Value, out: &mut Vec<u8>)
        -> CamelResult<()>;

    /// Unmarshals the body (Java `unmarshal(exchange, body)`): the result becomes the message
    /// body; the data format may also change the message, such as adding headers. See
    /// [`input_bytes`] to read the body.
    ///
    /// # Errors
    ///
    /// The body cannot be unmarshalled (a `NoTypeConversionAvailableException` for a body that
    /// is not bytes or text).
    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value>;

    /// Gives the context to the data format when it is used by a route (Java
    /// `CamelContextAware`); does nothing by default.
    fn set_camel_context(&self, context: Weak<dyn CamelContext>) {
        let _ = context;
    }

    /// Starts the data format with the processor using it (Java `Service.start`); does nothing
    /// by default.
    ///
    /// # Errors
    ///
    /// The data format cannot start.
    fn start(&self) -> CamelResult<()> {
        Ok(())
    }

    /// Stops the data format with the processor using it (Java `Service.stop`); does nothing
    /// by default.
    ///
    /// # Errors
    ///
    /// The data format cannot stop.
    fn stop(&self) -> CamelResult<()> {
        Ok(())
    }
}

/// A shared [`DataFormat`].
pub type SharedDataFormat = Arc<dyn DataFormat>;

/// Creates a data format with its options (the properties of the data format definition, such
/// as `lineLength=76`) on each use (Java `DataFormatFactory` plus the configurer of the
/// options).
pub type DataFormatFactory =
    Arc<dyn Fn(&[(String, String)]) -> CamelResult<SharedDataFormat> + Send + Sync>;

/// The data format as a value, to bind it in the registry (for `custom("name")`).
pub fn data_format_value(data_format: SharedDataFormat) -> Value {
    Value::Object(ObjectRef::new(data_format))
}

/// The data format held by a value of the registry (see [`data_format_value`]).
pub fn data_format_of(value: &Value) -> Option<SharedDataFormat> {
    match value {
        Value::Object(o) => o.downcast_ref::<SharedDataFormat>().cloned(),
        _ => None,
    }
}

/// The suffix of the name of a data format factory bean (Java
/// `ResolverHelper.DATA_FORMAT_FACTORY_FALLBACK_SUFFIX`).
pub const DATA_FORMAT_FACTORY_FALLBACK_SUFFIX: &str = "-dataformat-factory";

/// The data format factory as a value, to bind it in the registry under the name of the data
/// format or the name with the [`DATA_FORMAT_FACTORY_FALLBACK_SUFFIX`] (Java
/// `DataFormatFactory` beans); the context then creates the data format with it.
pub fn data_format_factory_value(factory: DataFormatFactory) -> Value {
    Value::Object(ObjectRef::new(factory))
}

/// The data format factory held by a value of the registry (see
/// [`data_format_factory_value`]).
pub fn data_format_factory_of(value: &Value) -> Option<DataFormatFactory> {
    match value {
        Value::Object(o) => o.downcast_ref::<DataFormatFactory>().cloned(),
        _ => None,
    }
}

/// The bytes of a body to unmarshal (Java converting the body to an `InputStream`): bytes,
/// text, numbers and booleans as their text.
///
/// # Errors
///
/// A `NoTypeConversionAvailableException` for a null body or another kind of value.
///
/// # Examples
///
/// ```
/// use camel_api::spi::data_format::input_bytes;
/// use camel_util::value::Value;
///
/// assert_eq!(b"abc".to_vec(), input_bytes(&Value::from("abc")).unwrap());
/// assert!(input_bytes(&Value::Null).is_err());
/// ```
pub fn input_bytes(body: &Value) -> CamelResult<Vec<u8>> {
    match body {
        Value::Bytes(b) => Ok(b.clone()),
        Value::Text(t) => Ok(t.as_bytes().to_vec()),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => Ok(body.to_string().into_bytes()),
        Value::Null => Err(CamelError::no_type_conversion_available(
            None,
            "java.io.InputStream",
        )),
        other => Err(CamelError::no_type_conversion_available(
            Some(other.type_name()),
            "java.io.InputStream",
        )),
    }
}

/// A data format from closures, for tests and simple formats.
pub struct FnDataFormat<M, U> {
    name: String,
    marshal: M,
    unmarshal: U,
}

impl<M, U> DataFormat for FnDataFormat<M, U>
where
    M: Fn(&mut Exchange, &Value, &mut Vec<u8>) -> CamelResult<()> + Send + Sync,
    U: Fn(&mut Exchange, &Value) -> CamelResult<Value> + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        (self.marshal)(exchange, graph, out)
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        (self.unmarshal)(exchange, body)
    }
}

/// Creates a data format from closures.
pub fn data_format_fn<M, U>(name: &str, marshal: M, unmarshal: U) -> FnDataFormat<M, U>
where
    M: Fn(&mut Exchange, &Value, &mut Vec<u8>) -> CamelResult<()> + Send + Sync,
    U: Fn(&mut Exchange, &Value) -> CamelResult<Value> + Send + Sync,
{
    FnDataFormat {
        name: name.to_string(),
        marshal,
        unmarshal,
    }
}
