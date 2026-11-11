//! The file functions of the simple language (Java `FileExpressionBuilder`): `${file:name}`,
//! `${file:onlyname}`, `${file:ext}`, `${file:parent}`, `${file:size}`, ... They describe the
//! file being consumed from the `CamelFile*` headers the file consumers set; they do not read
//! a file.

use super::{ParseResult, ParserError};
use camel_api::convert::FromValue;
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{FILE_LAST_MODIFIED, FILE_LENGTH, FILE_NAME, FILE_NAME_ONLY};
use camel_api::processor::SharedExpression;
use camel_support::builder::named_expression;
use camel_util::file_util;
use camel_util::value::Value;

fn header_text(e: &Exchange, name: &str) -> Option<String> {
    e.message()
        .header(name)
        .and_then(|v| String::from_value(v).ok().flatten())
}

fn file_name(e: &Exchange) -> Option<String> {
    header_text(e, FILE_NAME)
}

/// `${file:onlyname}`: the `CamelFileNameOnly` header, else the file name without its path.
fn only_name(e: &Exchange) -> Option<String> {
    header_text(e, FILE_NAME_ONLY)
        .or_else(|| file_name(e).map(|n| file_util::strip_path(&n).to_string()))
}

fn text(v: Option<String>) -> Value {
    v.map(Value::from).unwrap_or_default()
}

fn header_long(e: &Exchange, name: &str) -> Value {
    e.message()
        .header(name)
        .and_then(|v| i64::from_value(v).ok().flatten())
        .map(Value::Int)
        .unwrap_or_default()
}

/// The expression of the `${file:..}` function of the remainder (the text after `file:`).
///
/// # Errors
///
/// A syntax error for an unknown remainder, which says that the file functions do not read a
/// file.
pub fn file_function(remainder: &str, index: usize) -> ParseResult<SharedExpression> {
    let d = format!("file:{remainder}");
    let exp = match remainder {
        "name" => named_expression(d, |e| Ok(text(file_name(e)))),
        "name.noext" => named_expression(d, |e| {
            Ok(text(file_name(e).map(|n| file_util::strip_ext(&n).to_string())))
        }),
        "name.noext.single" => named_expression(d, |e| {
            Ok(text(
                file_name(e).map(|n| file_util::strip_ext_mode(&n, true).to_string()),
            ))
        }),
        "name.ext" | "ext" => named_expression("file:ext", |e| {
            Ok(text(
                file_name(e).and_then(|n| file_util::only_ext(&n).map(str::to_string)),
            ))
        }),
        "name.ext.single" => named_expression("file:ext.single", |e| {
            Ok(text(file_name(e).and_then(|n| {
                file_util::only_ext_mode(&n, true).map(str::to_string)
            })))
        }),
        "onlyname" => named_expression(d, |e| Ok(text(only_name(e)))),
        "onlyname.noext" => named_expression(d, |e| {
            Ok(text(only_name(e).map(|n| file_util::strip_ext(&n).to_string())))
        }),
        "onlyname.noext.single" => named_expression(d, |e| {
            Ok(text(
                only_name(e).map(|n| file_util::strip_ext_mode(&n, true).to_string()),
            ))
        }),
        "parent" => named_expression(d, |e| Ok(text(header_text(e, "CamelFileParent")))),
        "path" => named_expression(d, |e| Ok(text(header_text(e, "CamelFilePath")))),
        "absolute" => named_expression(d, |e| {
            Ok(e.message()
                .header("CamelFileAbsolute")
                .and_then(|v| bool::from_value(v).ok().flatten())
                .map(Value::Bool)
                .unwrap_or_default())
        }),
        "absolute.path" => {
            named_expression(d, |e| Ok(text(header_text(e, "CamelFileAbsolutePath"))))
        }
        "length" | "size" => named_expression("file:length", |e| Ok(header_long(e, FILE_LENGTH))),
        "modified" => named_expression(d, |e| Ok(header_long(e, FILE_LAST_MODIFIED))),
        _ => {
            return Err(ParserError::new(
                format!("Unknown file language syntax: {remainder} (the file: functions describe the file being consumed: ${{file:name}}, ${{file:size}}, ${{file:parent}}, ${{file:absolute.path}}; they do not read a file. To read a file into the body use the poll EIP with a file: endpoint)"),
                index,
            ))
        }
    };
    Ok(exp)
}
