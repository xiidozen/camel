//! Message helpers (Java `MessageHelper`): the dump of the message history.

use camel_api::context::context_plugin;
use camel_api::exchange::Exchange;
use camel_api::message_history::{body_size, body_type, message_history, MessageHistoryFactory};
use camel_api::spi::exchange_formatter::ExchangeFormatter;
use camel_util::string_helper::limit_length;
use camel_util::uri_support::sanitize_uri;

const LINE: &str = "----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------";

/// `%-{width}.{width}s`: left aligned, padded and truncated to the width.
fn left(text: &str, width: usize) -> String {
    let t: String = text.chars().take(width).collect();
    format!("{t:<width$}")
}

/// `%{width}.{width}s`: right aligned, padded and truncated to the width.
fn right(text: &str, width: usize) -> String {
    let t: String = text.chars().take(width).collect();
    format!("{t:>width$}")
}

/// A row of the table (Java `MESSAGE_HISTORY_OUTPUT`:
/// `%-40.40s %-30.30s %-50.50s %12.12s %-32.32s %9.9s`).
fn row(source: &str, id: &str, label: &str, elapsed: i64, body_type: &str, size: &str) -> String {
    format!(
        "{} {} {} {} {} {}",
        left(source, 40),
        left(id, 30),
        left(label, 50),
        right(&elapsed.to_string(), 12),
        left(body_type, 32),
        right(size, 9)
    )
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Dumps the message history of the exchange as a table (Java
/// `MessageHelper.dumpMessageHistoryStacktrace`): the route the exchange came from (with the
/// body as it is now), then each step of the history (with the body type and size when it was
/// reached), then the exchange formatted by the formatter, if any.
pub fn dump_message_history_stacktrace(
    exchange: &Exchange,
    formatter: Option<&dyn ExchangeFormatter>,
    log_stack_trace: bool,
) -> String {
    let route_id = exchange.from_route_id().unwrap_or_default().to_string();
    let label = exchange
        .from_endpoint()
        .map(|uri| format!("from[{}]", sanitize_uri(&limit_length(uri, 100))))
        .unwrap_or_default();
    let elapsed = now_millis() - exchange.created();
    let history = message_history(exchange);
    let enabled = history.is_some();
    // the source locations of the routes are not known in Rust
    let mut sb = String::with_capacity(2048);
    sb.push_str("\nMessage History");
    if !enabled {
        sb.push_str(" (source location and message history is disabled)");
    } else {
        sb.push_str(" (source location is disabled)");
    }
    sb.push('\n');
    sb.push_str(LINE);
    sb.push('\n');
    sb.push_str(&format!(
        "{:<40} {:<30} {:<50} {:<12} {:<32} {:<9}",
        "Source", "ID", "Processor", "Elapsed (ms)", "Body type", "Size"
    ));
    sb.push('\n');
    let body = exchange.message().body();
    let size_enabled = context_plugin::<MessageHistoryFactory>(exchange.context().as_ref())
        .is_some_and(|f| f.is_size_enabled());
    let size = if size_enabled && !body.is_null() {
        let s = body_size(body);
        if s >= 0 {
            s.to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    sb.push_str(&row(
        "",
        &format!("{route_id}/{route_id}"),
        &label,
        elapsed,
        &body_type(body),
        &size,
    ));
    sb.push('\n');
    for h in history.unwrap_or_default() {
        let label = sanitize_uri(&limit_length(&h.label, 100));
        let size = if h.body_size >= 0 {
            h.body_size.to_string()
        } else {
            String::new()
        };
        sb.push_str(&row(
            "",
            &format!("{}/{}", h.route_id, h.node_id),
            &label,
            h.elapsed,
            h.body_type.as_deref().unwrap_or_default(),
            &size,
        ));
        sb.push('\n');
    }
    if let Some(f) = formatter {
        sb.push_str("\nExchange\n");
        sb.push_str(LINE);
        sb.push('\n');
        sb.push_str(&f.format(exchange));
        sb.push('\n');
    }
    if log_stack_trace {
        sb.push_str("\nStacktrace\n");
        sb.push_str(&LINE[..135]);
    }
    sb
}
