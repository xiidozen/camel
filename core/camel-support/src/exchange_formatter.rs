//! The default exchange formatter (Java `org.apache.camel.support.processor.DefaultExchangeFormatter`)
//! and the body extraction for logging (Java `MessageHelper.extractValueForLogging`).

use camel_api::exchange::Exchange;
use camel_api::exchange_constants::EXCEPTION_CAUGHT;
use camel_api::spi::exchange_formatter::ExchangeFormatter;
use camel_util::value::{Value, ValueMap};
use std::fmt::Write;

const SEPARATOR: &str = "###REPLACE_ME###";

/// The line separator (Java `System.lineSeparator()`).
#[cfg(windows)]
pub const LS: &str = "\r\n";
/// The line separator (Java `System.lineSeparator()`).
#[cfg(not(windows))]
pub const LS: &str = "\n";

/// The text of a value for logging (Java `MessageHelper.extractValueForLogging`): `[Body is
/// null]` for no value, and texts longer than `max_chars` are clipped (`max_chars` 0 for no
/// limit, negative to not log the value).
///
/// # Examples
///
/// ```
/// use camel_support::exchange_formatter::extract_value_for_logging;
/// use camel_util::value::Value;
/// assert_eq!("[Body is null]", extract_value_for_logging(&Value::Null, 100));
/// assert_eq!("Hello... [Body clipped after 5 chars, total length is 11]", extract_value_for_logging(&Value::from("Hello World"), 5));
/// ```
pub fn extract_value_for_logging(value: &Value, max_chars: i32) -> String {
    if max_chars < 0 {
        return "[Body is not logged]".to_string();
    }
    if value.is_null() {
        return "[Body is null]".to_string();
    }
    let body = value.to_string();
    let len = body.chars().count();
    if max_chars > 0 && len > max_chars as usize {
        let clipped: String = body.chars().take(max_chars as usize).collect();
        return format!(
            "{clipped}... [Body clipped after {max_chars} chars, total length is {len}]"
        );
    }
    body
}

/// The output styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputStyle {
    /// `, Label: value`
    #[default]
    Default,
    /// `\tLabel: value`
    Tab,
    /// Fixed width labels.
    Fixed,
}

/// The default exchange formatter: `Exchange[ExchangePattern: InOnly, BodyType: String, Body:
/// Hello]`, with options for what to show.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct DefaultExchangeFormatter {
    pub show_route_id: bool,
    pub show_route_group: bool,
    pub show_exchange_id: bool,
    pub show_exchange_pattern: bool,
    pub show_properties: bool,
    pub show_all_properties: bool,
    pub show_variables: bool,
    pub show_headers: bool,
    pub skip_body_line_separator: bool,
    pub show_body: bool,
    pub show_body_type: bool,
    pub show_exception: bool,
    pub show_caught_exception: bool,
    pub show_stack_trace: bool,
    pub show_all: bool,
    pub multiline: bool,
    /// Limits the number of characters logged per line (default 10000).
    pub max_chars: i32,
    pub style: OutputStyle,
    /// Only the body.
    pub plain: bool,
}

impl Default for DefaultExchangeFormatter {
    fn default() -> Self {
        DefaultExchangeFormatter {
            show_route_id: false,
            show_route_group: false,
            show_exchange_id: false,
            show_exchange_pattern: false,
            show_properties: false,
            show_all_properties: false,
            show_variables: false,
            show_headers: false,
            skip_body_line_separator: true,
            show_body: true,
            show_body_type: true,
            show_exception: false,
            show_caught_exception: false,
            show_stack_trace: false,
            show_all: false,
            multiline: false,
            max_chars: 10000,
            style: OutputStyle::Default,
            plain: false,
        }
    }
}

/// The map sorted case-insensitively, as Java `TreeMap(CASE_INSENSITIVE_ORDER)` prints it.
fn sort_map<'a>(entries: impl Iterator<Item = (&'a str, &'a Value)>) -> String {
    let mut v: Vec<(&str, &Value)> = entries.collect();
    v.sort_by_key(|(k, _)| k.to_lowercase());
    let parts: Vec<String> = v.iter().map(|(k, val)| format!("{k}={val}")).collect();
    format!("{{{}}}", parts.join(", "))
}

fn map_entries(m: &ValueMap) -> impl Iterator<Item = (&str, &Value)> {
    m.iter().map(|(k, v)| (k.as_str(), v))
}

/// The body type as Java prints it (`String`, `Long`, `byte[]`, ..., `null`).
pub fn body_type_name(value: &Value) -> String {
    if value.is_null() {
        return "null".to_string();
    }
    value.type_name().to_string()
}

impl DefaultExchangeFormatter {
    fn style(&self, sb: &mut String, label: &str) {
        match self.style {
            OutputStyle::Default if self.multiline => {
                let _ = write!(sb, "  {label}: ");
            }
            OutputStyle::Default => {
                let _ = write!(sb, ", {label}: ");
            }
            OutputStyle::Tab => {
                let _ = write!(sb, "\t{label}: ");
            }
            OutputStyle::Fixed => {
                let _ = write!(sb, "\t{label:<20}");
            }
        }
    }

    fn sep(&self, sb: &mut String) {
        if self.multiline {
            sb.push_str(SEPARATOR);
        }
    }

    fn body_as_string(&self, exchange: &Exchange) -> String {
        extract_value_for_logging(exchange.in_message().body(), self.max_chars)
    }
}

impl ExchangeFormatter for DefaultExchangeFormatter {
    fn format(&self, exchange: &Exchange) -> String {
        if self.plain {
            return self.body_as_string(exchange);
        }
        let all = self.show_all;
        let mut sb = String::new();
        if all || self.show_exchange_id {
            self.sep(&mut sb);
            self.style(&mut sb, "Id");
            sb.push_str(exchange.exchange_id());
        }
        if all || self.show_route_group {
            // the group of the route is not known on the exchange yet
            self.sep(&mut sb);
            self.style(&mut sb, "RouteGroup");
        }
        if all || self.show_route_id {
            self.sep(&mut sb);
            self.style(&mut sb, "RouteId");
            sb.push_str(exchange.from_route_id().unwrap_or(""));
        }
        if all || self.show_exchange_pattern {
            self.sep(&mut sb);
            self.style(&mut sb, "ExchangePattern");
            sb.push_str(&exchange.pattern().to_string());
        }
        if all || self.show_all_properties {
            self.sep(&mut sb);
            self.style(&mut sb, "Properties");
            let props = exchange.all_properties();
            sb.push_str(&sort_map(map_entries(&props)));
        } else if self.show_properties {
            self.sep(&mut sb);
            self.style(&mut sb, "Properties");
            sb.push_str(&sort_map(map_entries(exchange.properties())));
        }
        if all || self.show_variables {
            self.sep(&mut sb);
            if exchange.has_variables() {
                self.style(&mut sb, "Variables");
                sb.push_str(&sort_map(map_entries(exchange.variables())));
            }
        }
        if all || self.show_headers {
            self.sep(&mut sb);
            self.style(&mut sb, "Headers");
            sb.push_str(&sort_map(exchange.in_message().headers().iter()));
        }
        if all || self.show_body_type {
            self.sep(&mut sb);
            self.style(&mut sb, "BodyType");
            sb.push_str(&body_type_name(exchange.in_message().body()));
        }
        if all || self.show_body {
            self.sep(&mut sb);
            let mut body = self.body_as_string(exchange);
            if self.skip_body_line_separator {
                body = body.replace(LS, "");
            }
            self.style(&mut sb, "Body");
            sb.push_str(&body);
        }
        if all || self.show_exception || self.show_caught_exception {
            let mut exception = exchange.exception().cloned();
            let mut caught = false;
            if (all || self.show_caught_exception) && exception.is_none() {
                exception = exchange
                    .property(EXCEPTION_CAUGHT)
                    .and_then(|v| v.downcast_ref::<camel_api::error::CamelError>())
                    .cloned();
                caught = true;
            }
            if let Some(e) = exception {
                self.sep(&mut sb);
                let (t, m) = if caught {
                    ("CaughtExceptionType", "CaughtExceptionMessage")
                } else {
                    ("ExceptionType", "ExceptionMessage")
                };
                self.style(&mut sb, t);
                sb.push_str(&camel_api::error::error_type_name(&e));
                self.style(&mut sb, m);
                sb.push_str(&e.to_string());
                if all || self.show_stack_trace {
                    self.style(&mut sb, "StackTrace");
                    let _ = write!(sb, "{e:?}");
                }
            }
        }
        if self.multiline || self.max_chars > 0 && sb.chars().count() > self.max_chars as usize {
            let mut answer = String::new();
            for s in sb.split(SEPARATOR) {
                if self.max_chars > 0 && s.chars().count() > self.max_chars as usize {
                    answer.extend(s.chars().take(self.max_chars as usize));
                    answer.push_str("...");
                } else {
                    answer.push_str(s);
                }
                if self.multiline {
                    answer.push_str(LS);
                }
            }
            sb = answer;
        }
        if !self.multiline && sb.starts_with(", ") {
            sb.replace_range(0..2, "");
        }
        format!("Exchange[{sb}]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_api::context::StandaloneContext;

    fn ex() -> Exchange {
        let mut e = Exchange::with_body(StandaloneContext::shared(), "Hello World");
        e.message_mut().set_header("foo", 123);
        e.message_mut().set_header("Bar", "x");
        e
    }

    #[test]
    fn formats() {
        let e = ex();
        let f = DefaultExchangeFormatter::default();
        assert_eq!(
            "Exchange[BodyType: String, Body: Hello World]",
            f.format(&e)
        );
        let f = DefaultExchangeFormatter {
            show_headers: true,
            show_exchange_pattern: true,
            ..Default::default()
        };
        assert_eq!("Exchange[ExchangePattern: InOnly, Headers: {Bar=x, foo=123}, BodyType: String, Body: Hello World]", f.format(&e));
        let f = DefaultExchangeFormatter {
            plain: true,
            ..Default::default()
        };
        assert_eq!("Hello World", f.format(&e));
        let f = DefaultExchangeFormatter {
            multiline: true,
            show_body_type: false,
            ..Default::default()
        };
        assert_eq!(
            format!("Exchange[{LS}  Body: Hello World{LS}]"),
            f.format(&e)
        );
        let f = DefaultExchangeFormatter {
            max_chars: 5,
            ..Default::default()
        };
        // as Java: the whole line is clipped too
        assert_eq!("Exchange[Bod...]", f.format(&e));
        let mut failed = ex();
        failed.set_exception(Some(camel_api::error::CamelError::illegal_argument(
            "Forced",
        )));
        let f = DefaultExchangeFormatter {
            show_exception: true,
            show_body_type: false,
            show_body: false,
            ..Default::default()
        };
        assert_eq!(
            "Exchange[ExceptionType: java.lang.IllegalArgumentException, ExceptionMessage: Forced]",
            f.format(&failed)
        );
        let f = DefaultExchangeFormatter {
            style: OutputStyle::Tab,
            ..Default::default()
        };
        assert_eq!(
            "Exchange[\tBodyType: String\tBody: Hello World]",
            f.format(&e)
        );
    }
}
