//! The `marshal` and `unmarshal` EIPs (Java `MarshalProcessor` and `UnmarshalProcessor`):
//! the body (or a variable) through a data format, the result becoming the body (or a
//! variable).

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_api::exchange::Exchange;
use camel_api::processor::{BoxFuture, Processor};
use camel_api::spi::data_format::SharedDataFormat;
use camel_util::value::Value;

/// The options shared by the marshal and unmarshal processors.
#[derive(Debug, Clone, Default)]
pub struct DataFormatOptions {
    /// The variable used instead of the body.
    pub variable_send: Option<String>,
    /// The variable receiving the result instead of the body.
    pub variable_receive: Option<String>,
    /// Whether a null body is left as is.
    pub allow_null_body: bool,
}

/// The input of the data format: the body, or the variable sent.
fn input(exchange: &Exchange, options: &DataFormatOptions) -> Value {
    match &options.variable_send {
        Some(v) => exchange.variable(v).cloned().unwrap_or_default(),
        None => exchange.message().body().clone(),
    }
}

/// The marshal EIP (Java `MarshalProcessor`).
pub struct MarshalProcessor {
    data_format: SharedDataFormat,
    options: DataFormatOptions,
}

impl MarshalProcessor {
    /// Creates the processor.
    pub fn new(data_format: SharedDataFormat, options: DataFormatOptions) -> Self {
        MarshalProcessor {
            data_format,
            options,
        }
    }

    fn run(&self, exchange: &mut Exchange) {
        let body = input(exchange, &self.options);
        if self.options.allow_null_body && body.is_null() {
            if let Some(v) = &self.options.variable_receive {
                exchange.set_variable(v, Value::Null);
            }
            return;
        }
        let mut out = Vec::new();
        match self.data_format.marshal(exchange, &body, &mut out) {
            Ok(()) => match &self.options.variable_receive {
                Some(v) => exchange.set_variable(v, Value::Bytes(out)),
                None => exchange.message_mut().set_body(Value::Bytes(out)),
            },
            Err(e) => exchange.set_exception(Some(e)),
        }
    }
}

impl Processor for MarshalProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.data_format.start()))
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.data_format.stop()))
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        self.run(exchange);
        Box::pin(std::future::ready(Ok(())))
    }
}

/// The unmarshal EIP (Java `UnmarshalProcessor`).
pub struct UnmarshalProcessor {
    data_format: SharedDataFormat,
    options: DataFormatOptions,
}

impl UnmarshalProcessor {
    /// Creates the processor.
    pub fn new(data_format: SharedDataFormat, options: DataFormatOptions) -> Self {
        UnmarshalProcessor {
            data_format,
            options,
        }
    }

    /// The message of a failure on a null input (Java `nullBodyException`).
    fn null_body_error(&self, e: CamelError) -> CamelError {
        if !e.is(ErrorKind::NoTypeConversionAvailable) {
            return e;
        }
        let what = match &self.options.variable_send {
            Some(v) => format!("the variable {v}"),
            None => "the message body".to_string(),
        };
        CamelError::with_cause(
            ErrorKind::NoTypeConversionAvailable,
            format!(
                "Cannot unmarshal using {}: {what} is null, so there is nothing to unmarshal. Set {what} first, or set allowNullBody=true to skip a null body.",
                self.data_format.name()
            ),
            e,
        )
    }

    fn run(&self, exchange: &mut Exchange) {
        let body = input(exchange, &self.options);
        if self.options.allow_null_body && body.is_null() {
            match &self.options.variable_receive {
                Some(v) => exchange.set_variable(v, Value::Null),
                None => exchange.message_mut().set_body(Value::Null),
            }
            return;
        }
        match self.data_format.unmarshal(exchange, &body) {
            Ok(result) => match &self.options.variable_receive {
                Some(v) => exchange.set_variable(v, result),
                None => exchange.message_mut().set_body(result),
            },
            Err(e) => {
                let e = if body.is_null() {
                    self.null_body_error(e)
                } else {
                    e
                };
                exchange.set_exception(Some(e));
            }
        }
    }
}

impl Processor for UnmarshalProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.data_format.start()))
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(self.data_format.stop()))
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        self.run(exchange);
        Box::pin(std::future::ready(Ok(())))
    }
}
