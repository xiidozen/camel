//! # camel-csv
//!
//! The CSV data format (Java `CsvDataFormat`, on Apache Commons CSV): `marshal` prints a list
//! of records (each a list or a map of values; a single map is one record) as CSV text, and
//! `unmarshal` parses CSV text into a list of records (lists of texts, or maps by the names of
//! the header with `useMaps`). The [`format`](mod@format) module is the port of the Commons CSV
//! `CSVFormat` (with its predefined formats) and the [`parser`] module the port of its parser.
//! This crate is the Rust port of the Java `components/camel-csv` module.
//!
//! # Options
//!
//! | Option | Default | Description |
//! |---|---|---|
//! | `format` | `DEFAULT` | The reference format to use, it will be updated with the other format options, the default value is CSVFormat.DEFAULT. |
//! | `commentMarkerDisabled` | `false` | Disables the comment marker of the reference format. |
//! | `commentMarker` | | Sets the comment marker of the reference format. |
//! | `delimiter` | | Sets the delimiter to use. The default value is , (comma). |
//! | `escapeDisabled` | `false` | Use for disabling using escape character. |
//! | `escape` | | Sets the escape character to use. |
//! | `headerDisabled` | `false` | Use for disabling headers. |
//! | `header` | | To configure the CSV headers (comma separated). |
//! | `allowMissingColumnNames` | | Whether to allow missing column names. |
//! | `ignoreEmptyLines` | | Whether to ignore empty lines. |
//! | `ignoreSurroundingSpaces` | | Whether to ignore surrounding spaces. |
//! | `nullStringDisabled` | `false` | Used to disable null strings. |
//! | `nullString` | | Sets the null string. |
//! | `quoteDisabled` | `false` | Used to disable quotes. |
//! | `quote` | | Sets the quote to use which by default is double-quote character. |
//! | `recordSeparatorDisabled` | `false` | Used for disabling record separator. |
//! | `recordSeparator` | | Sets the record separator (aka new line) which by default is new line characters (CRLF). |
//! | `skipHeaderRecord` | | Whether to skip the header record in the output. |
//! | `quoteMode` | | Sets the quote mode (`ALL`, `ALL_NON_NULL`, `MINIMAL`, `NON_NUMERIC`, `NONE`). |
//! | `ignoreHeaderCase` | | Sets whether or not to ignore case when accessing header names. |
//! | `trim` | | Sets whether or not to trim leading and trailing blanks. |
//! | `trailingDelimiter` | | Sets whether or not to add a trailing delimiter. |
//! | `marshallerFactoryRef` | | Sets the implementation of the CsvMarshallerFactory interface which is able to customize marshalling/unmarshalling behavior by extending CsvMarshaller or creating it from scratch. |
//! | `lazyLoad` | `false` | Whether the unmarshalling should produce an iterator that reads the lines on the fly or if all the lines must be read at once (the records are read at once in Rust). |
//! | `useMaps` | `false` | Whether the unmarshalling should produce maps (HashMap) for the lines values instead of lists. It requires to have header (either defined or collected). |
//! | `useOrderedMaps` | `false` | Whether the unmarshalling should produce ordered maps (LinkedHashMap) for the lines values instead of lists. It requires to have header (either defined or collected). |
//! | `recordConverterRef` | | Refers to a custom CsvRecordConverter to lookup from the registry to use. |
//! | `captureHeaderRecord` | `false` | Whether the unmarshalling should capture the header record and store it in the message header. |
//!
//! # Examples
//!
//! ```
//! use camel_csv::format::CsvFormat;
//! use camel_csv::CsvDataFormat;
//!
//! let df = CsvDataFormat::new().with_delimiter('|');
//! assert_eq!("|", df.active_format().unwrap().delimiter);
//! assert_eq!(CsvFormat::mysql(), CsvDataFormat::new().with_format("MYSQL").active_format().unwrap());
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod format;
pub mod parser;

use crate::format::{CsvFormat, QuoteMode};
use crate::parser::{CsvParser, CsvRecord};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::CHARSET_NAME;
use camel_api::spi::data_format::{input_bytes, DataFormat, DataFormatFactory, SharedDataFormat};
use camel_base_engine::context::DefaultCamelContext;
use camel_util::charset::Charset;
use camel_util::value::{ObjectRef, Value, ValueMap};
use std::sync::{Arc, OnceLock};

/// The name of the data format.
pub const NAME: &str = "csv";

/// The header with the names of the header record (Java `CsvConstants.HEADER_RECORD`).
pub const HEADER_RECORD: &str = "CamelCsvHeaderRecord";

/// Converts a parsed record into the value of the unmarshalled list (Java
/// `CsvRecordConverter`).
pub trait CsvRecordConverter: Send + Sync {
    /// The value of the record.
    fn convert_record(&self, record: &CsvRecord) -> Value;
}

impl<F> CsvRecordConverter for F
where
    F: Fn(&CsvRecord) -> Value + Send + Sync,
{
    fn convert_record(&self, record: &CsvRecord) -> Value {
        self(record)
    }
}

/// A shared record converter.
pub type SharedRecordConverter = Arc<dyn CsvRecordConverter>;

/// The record converter as a value, to bind it in the registry (for `recordConverterRef`).
pub fn record_converter_value(converter: SharedRecordConverter) -> Value {
    Value::Object(ObjectRef::new(converter))
}

/// The record converters of the data format (Java `CsvRecordConverters`).
pub mod converters {
    use super::*;

    /// The values of the record as a list of texts (null values as null).
    pub fn list(record: &CsvRecord) -> Value {
        Value::List(
            record
                .values()
                .iter()
                .map(|v| v.as_deref().map_or(Value::Null, Value::from))
                .collect(),
        )
    }

    /// The values of the record by the names of the header (Java `CSVRecord.toMap`).
    pub fn map(record: &CsvRecord) -> Value {
        let mut m = ValueMap::new();
        for (k, v) in record.to_map() {
            m.insert(k, v.map_or(Value::Null, Value::from));
        }
        Value::Map(m)
    }
}

/// Prints the bodies to marshal (Java `CsvMarshaller`).
pub trait CsvMarshaller: Send + Sync {
    /// Prints the body into the output.
    ///
    /// # Errors
    ///
    /// A body that cannot be printed.
    fn marshal(&self, exchange: &mut Exchange, body: &Value, out: &mut Vec<u8>) -> CamelResult<()>;
}

/// Creates the marshaller of a data format (Java `CsvMarshallerFactory`).
pub trait CsvMarshallerFactory: Send + Sync {
    /// The marshaller of the active format of the data format.
    fn create(&self, format: &CsvFormat, data_format: &CsvDataFormat) -> Arc<dyn CsvMarshaller>;
}

impl<F> CsvMarshallerFactory for F
where
    F: Fn(&CsvFormat, &CsvDataFormat) -> Arc<dyn CsvMarshaller> + Send + Sync,
{
    fn create(&self, format: &CsvFormat, data_format: &CsvDataFormat) -> Arc<dyn CsvMarshaller> {
        self(format, data_format)
    }
}

/// A shared marshaller factory.
pub type SharedMarshallerFactory = Arc<dyn CsvMarshallerFactory>;

/// The marshaller factory as a value, to bind it in the registry (for
/// `marshallerFactoryRef`).
pub fn marshaller_factory_value(factory: SharedMarshallerFactory) -> Value {
    Value::Object(ObjectRef::new(factory))
}

/// The charset of the exchange (the `CamelCharsetName` property, UTF-8 by default; Java
/// `ExchangeHelper.getCharsetName`).
pub fn charset_of(exchange: &Exchange) -> CamelResult<Charset> {
    match exchange.property(CHARSET_NAME) {
        Some(v) if !v.is_null() => Charset::for_name(&v.to_string())
            .map_err(|e| CamelError::illegal_argument(e.to_string())),
        _ => Ok(Charset::Utf8),
    }
}

/// The records of a body to marshal (Java `ObjectHelper.createIterator`): the items of a
/// list, the comma separated parts of a text, or the value itself.
pub fn records_of(body: &Value) -> Vec<Value> {
    match body {
        Value::List(l) => l.clone(),
        Value::Null => Vec::new(),
        Value::Text(t) => t.split(',').map(Value::from).collect(),
        other => vec![other.clone()],
    }
}

/// The default marshaller (Java `CsvMarshaller.create`): the values of the maps by the
/// columns of the header (fixed columns), or all their values (dynamic columns).
#[derive(Debug, Clone)]
pub struct DefaultCsvMarshaller {
    format: CsvFormat,
    fixed_columns: Option<Vec<String>>,
}

impl DefaultCsvMarshaller {
    /// The marshaller of the format.
    pub fn new(format: &CsvFormat) -> Self {
        let mut format = format.clone();
        let headers = format.header.clone();
        if format.skip_header_record {
            format.header = None;
        }
        DefaultCsvMarshaller {
            format,
            fixed_columns: headers.filter(|h| !h.is_empty()),
        }
    }

    /// The format, without its header when it is skipped.
    pub fn format(&self) -> &CsvFormat {
        &self.format
    }

    /// The values of a map record.
    pub fn map_record_values(&self, map: &ValueMap) -> Vec<Value> {
        match &self.fixed_columns {
            Some(columns) => columns
                .iter()
                .map(|c| map.get(c).cloned().unwrap_or(Value::Null))
                .collect(),
            None => map.values().cloned().collect(),
        }
    }

    /// The values of a record: the values of a map, the items of a list, or the value alone.
    pub fn record_values(&self, record: &Value) -> Vec<Value> {
        match record {
            Value::Map(m) => self.map_record_values(m),
            Value::List(l) => l.clone(),
            other => vec![other.clone()],
        }
    }

    /// The text of the body: the header, then the records.
    pub fn print(&self, body: &Value) -> String {
        let mut text = String::new();
        self.format.print_header(&mut text);
        match body {
            Value::Map(m) => self
                .format
                .print_record(&mut text, &self.map_record_values(m)),
            other => {
                for r in records_of(other) {
                    self.format.print_record(&mut text, &self.record_values(&r));
                }
            }
        }
        text
    }
}

impl CsvMarshaller for DefaultCsvMarshaller {
    fn marshal(&self, exchange: &mut Exchange, body: &Value, out: &mut Vec<u8>) -> CamelResult<()> {
        let text = self.print(body);
        out.extend(charset_of(exchange)?.encode(&text));
        Ok(())
    }
}

/// The CSV data format (Java `CsvDataFormat`).
#[derive(Default)]
pub struct CsvDataFormat {
    csv_format: CsvFormat,
    format_name: Option<String>,
    comment_marker_disabled: bool,
    comment_marker: Option<char>,
    delimiter: Option<String>,
    escape_disabled: bool,
    escape: Option<char>,
    header_disabled: bool,
    header: Option<Vec<String>>,
    allow_missing_column_names: Option<bool>,
    ignore_empty_lines: Option<bool>,
    ignore_surrounding_spaces: Option<bool>,
    null_string_disabled: bool,
    null_string: Option<String>,
    quote_disabled: bool,
    quote: Option<char>,
    quote_mode: Option<QuoteMode>,
    record_separator_disabled: bool,
    record_separator: Option<String>,
    skip_header_record: Option<bool>,
    trim: Option<bool>,
    ignore_header_case: Option<bool>,
    trailing_delimiter: Option<bool>,
    capture_header_record: bool,
    lazy_load: bool,
    use_maps: bool,
    use_ordered_maps: bool,
    record_converter: Option<SharedRecordConverter>,
    record_converter_ref: Option<String>,
    marshaller_factory: Option<SharedMarshallerFactory>,
    marshaller_factory_ref: Option<String>,
    marshaller: OnceLock<Arc<dyn CsvMarshaller>>,
}

/// The conversion of a builder argument into the field of the data format.
trait Field<T> {
    fn field(self) -> T;
}

impl Field<bool> for bool {
    fn field(self) -> bool {
        self
    }
}

impl<T> Field<Option<T>> for T {
    fn field(self) -> Option<T> {
        Some(self)
    }
}

impl Field<Option<String>> for &str {
    fn field(self) -> Option<String> {
        Some(self.to_string())
    }
}

macro_rules! with {
    ($(#[$doc:meta] $name:ident, $field:ident: $t:ty;)*) => {
        $(
            #[$doc]
            pub fn $name(mut self, value: $t) -> Self {
                self.$field = value.field();
                self
            }
        )*
    };
}

impl CsvDataFormat {
    /// The data format of the Commons CSV `DEFAULT` format.
    pub fn new() -> Self {
        Self::default()
    }

    /// The data format of a format (Java `new CsvDataFormat(CSVFormat)`).
    pub fn of_format(format: CsvFormat) -> Self {
        CsvDataFormat {
            csv_format: format,
            ..Self::default()
        }
    }

    with! {
        /// With the name of the reference format (such as `EXCEL`).
        with_format, format_name: &str;
        /// With the comment marker.
        with_comment_marker, comment_marker: char;
        /// Whether the comment marker of the reference format is disabled.
        with_comment_marker_disabled, comment_marker_disabled: bool;
        /// With the escape character.
        with_escape, escape: char;
        /// Whether the escape character is disabled.
        with_escape_disabled, escape_disabled: bool;
        /// Whether the header is disabled.
        with_header_disabled, header_disabled: bool;
        /// Whether to allow missing column names.
        with_allow_missing_column_names, allow_missing_column_names: bool;
        /// Whether to ignore empty lines.
        with_ignore_empty_lines, ignore_empty_lines: bool;
        /// Whether to ignore surrounding spaces.
        with_ignore_surrounding_spaces, ignore_surrounding_spaces: bool;
        /// With the null string.
        with_null_string, null_string: &str;
        /// Whether the null string is disabled.
        with_null_string_disabled, null_string_disabled: bool;
        /// With the quote character.
        with_quote, quote: char;
        /// Whether quotes are disabled.
        with_quote_disabled, quote_disabled: bool;
        /// With the quote mode.
        with_quote_mode, quote_mode: QuoteMode;
        /// With the record separator.
        with_record_separator, record_separator: &str;
        /// Whether the record separator is disabled.
        with_record_separator_disabled, record_separator_disabled: bool;
        /// Whether the header record is skipped.
        with_skip_header_record, skip_header_record: bool;
        /// Whether the values are trimmed.
        with_trim, trim: bool;
        /// Whether the header names ignore the case.
        with_ignore_header_case, ignore_header_case: bool;
        /// Whether a delimiter ends the records.
        with_trailing_delimiter, trailing_delimiter: bool;
        /// Whether the header record is stored in the [`HEADER_RECORD`] header.
        with_capture_header_record, capture_header_record: bool;
        /// Whether the records are read lazily (they are read at once in Rust).
        with_lazy_load, lazy_load: bool;
        /// Whether the records are unmarshalled as maps.
        with_use_maps, use_maps: bool;
        /// Whether the records are unmarshalled as ordered maps.
        with_use_ordered_maps, use_ordered_maps: bool;
        /// With the name of the record converter bean.
        with_record_converter_ref, record_converter_ref: &str;
        /// With the name of the marshaller factory bean.
        with_marshaller_factory_ref, marshaller_factory_ref: &str;
    }

    /// With the delimiter.
    pub fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = Some(delimiter.to_string());
        self
    }

    /// With the names of the header.
    pub fn with_header<S: AsRef<str>>(mut self, header: &[S]) -> Self {
        self.header = Some(header.iter().map(|h| h.as_ref().to_string()).collect());
        self
    }

    /// With the record converter.
    pub fn with_record_converter(mut self, converter: impl CsvRecordConverter + 'static) -> Self {
        self.record_converter = Some(Arc::new(converter));
        self
    }

    /// With the marshaller factory.
    pub fn with_marshaller_factory(mut self, factory: impl CsvMarshallerFactory + 'static) -> Self {
        self.marshaller_factory = Some(Arc::new(factory));
        self
    }

    /// The reference format (the predefined format of the `format` option, or the format of
    /// the data format).
    ///
    /// # Errors
    ///
    /// An unknown format name.
    pub fn csv_format(&self) -> CamelResult<CsvFormat> {
        match &self.format_name {
            Some(n) => CsvFormat::predefined(n),
            None => Ok(self.csv_format.clone()),
        }
    }

    /// The header (comma separated, Java `getHeader`).
    pub fn header(&self) -> Option<String> {
        self.header.as_ref().map(|h| h.join(","))
    }

    /// The format with the options applied (Java `getActiveFormat`).
    ///
    /// # Errors
    ///
    /// An unknown format name or an invalid format.
    pub fn active_format(&self) -> CamelResult<CsvFormat> {
        let mut f = self.csv_format()?;
        if self.comment_marker_disabled {
            f.comment_marker = None;
        } else if self.comment_marker.is_some() {
            f.comment_marker = self.comment_marker;
        }
        if let Some(d) = &self.delimiter {
            f.delimiter = d.clone();
        }
        if self.escape_disabled {
            f.escape = None;
        } else if self.escape.is_some() {
            f.escape = self.escape;
        }
        if self.header_disabled {
            f.header = None;
        } else if self.header.is_some() {
            f.header = self.header.clone();
        }
        if let Some(v) = self.allow_missing_column_names {
            f.allow_missing_column_names = v;
        }
        if let Some(v) = self.ignore_empty_lines {
            f.ignore_empty_lines = v;
        }
        if let Some(v) = self.ignore_surrounding_spaces {
            f.ignore_surrounding_spaces = v;
        }
        if self.null_string_disabled {
            f.null_string = None;
        } else if self.null_string.is_some() {
            f.null_string = self.null_string.clone();
        }
        if self.quote_disabled {
            f.quote = None;
        } else if self.quote.is_some() {
            f.quote = self.quote;
        }
        if self.quote_mode.is_some() {
            f.quote_mode = self.quote_mode;
        }
        if self.record_separator_disabled {
            f.record_separator = None;
        } else if self.record_separator.is_some() {
            f.record_separator = self.record_separator.clone();
        }
        if let Some(v) = self.skip_header_record {
            f.skip_header_record = v;
        }
        if let Some(v) = self.trim {
            f.trim = v;
        }
        if let Some(v) = self.ignore_header_case {
            f.ignore_header_case = v;
        }
        if let Some(v) = self.trailing_delimiter {
            f.trailing_delimiter = v;
        }
        f.validate()?;
        Ok(f)
    }

    /// Whether the header record is captured.
    pub fn is_capture_header_record(&self) -> bool {
        self.capture_header_record
    }

    /// Whether the records are unmarshalled as maps.
    pub fn is_use_maps(&self) -> bool {
        self.use_maps || self.use_ordered_maps
    }

    /// Whether the records are read lazily.
    pub fn is_lazy_load(&self) -> bool {
        self.lazy_load
    }

    /// The data format of the options (see the options of the crate).
    ///
    /// # Errors
    ///
    /// An unknown option or an invalid value.
    pub fn of_options(options: &[(String, String)]) -> CamelResult<Self> {
        let mut df = Self::default();
        let flag = |v: &str| v.trim().eq_ignore_ascii_case("true");
        let character = |k: &str, v: &str| {
            let mut chars = v.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(CamelError::illegal_argument(format!(
                    "The {k} option must be one character: {v}"
                ))),
            }
        };
        for (k, v) in options {
            match k.as_str() {
                "format" => df.format_name = Some(v.clone()),
                "commentMarkerDisabled" => df.comment_marker_disabled = flag(v),
                "commentMarker" => df.comment_marker = Some(character(k, v)?),
                "delimiter" => df.delimiter = Some(v.clone()),
                "escapeDisabled" => df.escape_disabled = flag(v),
                "escape" => df.escape = Some(character(k, v)?),
                "headerDisabled" => df.header_disabled = flag(v),
                "header" => df.header = Some(v.split(',').map(|h| h.trim().to_string()).collect()),
                "allowMissingColumnNames" => df.allow_missing_column_names = Some(flag(v)),
                "ignoreEmptyLines" => df.ignore_empty_lines = Some(flag(v)),
                "ignoreSurroundingSpaces" => df.ignore_surrounding_spaces = Some(flag(v)),
                "nullStringDisabled" => df.null_string_disabled = flag(v),
                "nullString" => df.null_string = Some(v.clone()),
                "quoteDisabled" => df.quote_disabled = flag(v),
                "quote" => df.quote = Some(character(k, v)?),
                "quoteMode" => df.quote_mode = Some(QuoteMode::parse(v)?),
                "recordSeparatorDisabled" => df.record_separator_disabled = flag(v),
                "recordSeparator" => df.record_separator = Some(v.clone()),
                "skipHeaderRecord" => df.skip_header_record = Some(flag(v)),
                "trim" => df.trim = Some(flag(v)),
                "ignoreHeaderCase" => df.ignore_header_case = Some(flag(v)),
                "trailingDelimiter" => df.trailing_delimiter = Some(flag(v)),
                "captureHeaderRecord" => df.capture_header_record = flag(v),
                "lazyLoad" => df.lazy_load = flag(v),
                "useMaps" => df.use_maps = flag(v),
                "useOrderedMaps" => df.use_ordered_maps = flag(v),
                "recordConverterRef" => df.record_converter_ref = Some(v.clone()),
                "marshallerFactoryRef" => df.marshaller_factory_ref = Some(v.clone()),
                _ => {
                    return Err(CamelError::illegal_argument(format!(
                        "Unknown option {k} of the csv data format"
                    )))
                }
            }
        }
        // the format is checked at once
        df.active_format()?;
        Ok(df)
    }

    /// A bean of the registry of the exchange's context.
    fn lookup<T: Clone + 'static>(exchange: &Exchange, name: &str) -> CamelResult<T> {
        exchange
            .context()
            .registry()
            .and_then(|r| r.lookup_by_name(name))
            .and_then(|v| match v {
                Value::Object(o) => o.downcast_ref::<T>().cloned(),
                _ => None,
            })
            .ok_or_else(|| {
                CamelError::new(
                    camel_api::error::ErrorKind::NoSuchBean,
                    format!("No bean could be found in the registry for: {name}"),
                )
            })
    }

    fn marshaller(&self, exchange: &Exchange) -> CamelResult<Arc<dyn CsvMarshaller>> {
        if let Some(m) = self.marshaller.get() {
            return Ok(m.clone());
        }
        let format = self.active_format()?;
        let m = match (&self.marshaller_factory, &self.marshaller_factory_ref) {
            (Some(f), _) => f.create(&format, self),
            (None, Some(r)) => {
                Self::lookup::<SharedMarshallerFactory>(exchange, r)?.create(&format, self)
            }
            (None, None) => Arc::new(DefaultCsvMarshaller::new(&format)),
        };
        Ok(self.marshaller.get_or_init(|| m).clone())
    }

    fn converter(&self, exchange: &Exchange) -> CamelResult<SharedRecordConverter> {
        Ok(match (&self.record_converter, &self.record_converter_ref) {
            (Some(c), _) => c.clone(),
            (None, Some(r)) => Self::lookup::<SharedRecordConverter>(exchange, r)?,
            (None, None) if self.is_use_maps() => Arc::new(converters::map),
            (None, None) => Arc::new(converters::list),
        })
    }
}

impl DataFormat for CsvDataFormat {
    fn name(&self) -> &str {
        NAME
    }

    fn marshal(
        &self,
        exchange: &mut Exchange,
        graph: &Value,
        out: &mut Vec<u8>,
    ) -> CamelResult<()> {
        let m = self.marshaller(exchange)?;
        m.marshal(exchange, graph, out)
    }

    fn unmarshal(&self, exchange: &mut Exchange, body: &Value) -> CamelResult<Value> {
        let text = match body {
            Value::Text(t) => t.clone(),
            other => charset_of(exchange)?.decode(&input_bytes(other)?),
        };
        let mut format = self.active_format()?;
        if (self.capture_header_record || self.is_use_maps() || format.skip_header_record)
            && format.header.is_none()
        {
            format.header = Some(Vec::new());
        }
        let mut parser = CsvParser::new(&format, &text)?;
        if self.capture_header_record {
            let names = parser.header_names().unwrap_or_default();
            let names: Vec<Value> = names.iter().map(|n| Value::from(n.as_str())).collect();
            exchange
                .message_mut()
                .set_header(HEADER_RECORD, Value::List(names));
        }
        let converter = self.converter(exchange)?;
        let records = parser.records()?;
        Ok(Value::List(
            records
                .iter()
                .map(|r| converter.convert_record(r))
                .collect(),
        ))
    }
}

/// The factory of the data format, to register on a context.
pub fn factory() -> DataFormatFactory {
    Arc::new(|options| Ok(Arc::new(CsvDataFormat::of_options(options)?) as SharedDataFormat))
}

/// Registers the data format on the context (what Java finds on the classpath).
pub fn register(context: &DefaultCamelContext) {
    context.register_data_format_factory(NAME, factory());
}
