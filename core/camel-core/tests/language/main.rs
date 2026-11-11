//! Tests ported from the Java camel-core package `org.apache.camel.language.simple` (one module
//! per Java test class).

#[path = "../support/mod.rs"]
mod support;

mod date_function_factory_test;
mod simple_operator_test;
mod simple_parser_edge_cases_test;
mod simple_parser_expression_test;
mod simple_parser_predicate_test;
mod simple_predicate_in_braces_test;
mod simple_string_math_date_functions_test;
mod simple_syntax_hints_test;
mod simple_test;
