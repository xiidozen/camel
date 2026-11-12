//! The simple language (Java `org.apache.camel.language.simple`): text templates with
//! functions such as `Hello ${body} from ${header.name}`, and predicates such as
//! `${header.foo} == 'bar' && ${body} contains 'World'`.

pub mod ast;
pub mod bean_functions;
pub mod collection_functions;
pub mod date_functions;
pub mod expression_parser;
pub mod file_functions;
pub mod functions;
pub mod init_block;
pub mod language;
pub mod math_functions;
pub mod misc_functions;
pub mod ognl;
pub mod other_functions;
pub mod other_operators;
pub mod predicate_parser;
pub mod properties_functions;
pub mod string_functions;
pub mod syntax_hints;
pub mod tokenizer;

use camel_api::error::{CamelError, ErrorKind};

/// The Java class name of the syntax errors of the simple language.
pub const SIMPLE_ILLEGAL_SYNTAX: &str =
    "org.apache.camel.language.simple.types.SimpleIllegalSyntaxException";

/// A syntax error at a position (Java `SimpleParserException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserError {
    /// The message.
    pub message: String,
    /// The position in the expression (`None` when unknown).
    pub index: Option<usize>,
}

impl ParserError {
    /// A syntax error at the position.
    pub fn new(message: impl Into<String>, index: usize) -> Self {
        ParserError {
            message: message.into(),
            index: Some(index),
        }
    }
}

/// The error of an invalid simple expression (Java `SimpleIllegalSyntaxException`): the
/// message, then for a known position ` at location N`, the expression and a `*` under the
/// position.
pub fn illegal_syntax(expression: &str, error: &ParserError) -> CamelError {
    let mut sb = error.message.clone();
    if let Some(index) = error.index {
        sb.push_str(&format!(
            " at location {index}\n{expression}\n{}*\n",
            " ".repeat(index)
        ));
    }
    CamelError::new(ErrorKind::ExpressionIllegalSyntax, sb)
        .with_subtype(SIMPLE_ILLEGAL_SYNTAX)
        .with_detail(expression)
}

/// The result of parsing.
pub type ParseResult<T> = Result<T, ParserError>;
