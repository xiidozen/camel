//! Port of `SimpleParserEdgeCasesTest` (CAMEL-24964: edge cases of the tokenizer and the
//! predicate parser).

use super::simple_test::{assert_expression, assert_predicate, exchange};
use camel_core::prelude::CamelContext;

#[test]
fn test_single_quote_inside_double_quotes_in_function() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("a'b'c");
    assert_predicate(&e, "${body.replace(\"'\", \"\")} == 'abc'", true);
    assert_predicate(&e, "${body.replace(\"'\", \"\")} == 'x'", false);
}

#[test]
fn test_brace_inside_double_quotes() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("a}b");
    e.message_mut().set_header("foo", "y");
    assert_predicate(&e, "${body} contains \"}\" && ${header.foo} == 'y'", true);
    assert_predicate(&e, "${body} contains \"}\" && ${header.foo} == 'z'", false);
}

#[test]
fn test_chain_followed_by_number_or_null() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("abcdef");
    assert_predicate(&e, "${body} ~> ${length()} > 5", true);
    assert_predicate(&e, "${body} ~> ${length()} > 10", false);
    assert_predicate(&e, "${body} ~> ${length()} == null", false);
    assert_predicate(&e, "${body} ~> ${length()} > -5", true);
}

#[test]
fn test_chain_needs_spaces() {
    let (_t, mut e) = exchange();
    assert_expression(&e, "Move A~>B", "Move A~>B");
    e.message_mut().set_body("a~>b");
    assert_expression(&e, "${body.replace('~>', '-')}", "a-b");
}

#[test]
fn test_operator_inside_quotes_in_braces() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("a > b");
    assert_predicate(&e, "${body == 'a > b'}", true);
    e.message_mut().set_body("x == y");
    assert_predicate(&e, "${body startsWith 'x == y'}", true);
}

#[test]
fn test_unquoted_text_with_digit_is_lenient() {
    let (_t, mut e) = exchange();
    // kept working as routes may compare with unquoted text such as v2
    e.message_mut().set_header("version", "v2");
    assert_predicate(&e, "${header.version} == v2", true);
}

#[test]
fn test_colon_without_spaces_is_not_a_ternary() {
    let (t, e) = exchange();
    // Java MyService
    let svc = camel_bean::MethodBean::new("MyService").method("echo", |inv| Ok(inv.parameter(0)));
    t.context
        .registry()
        .unwrap()
        .bind("svc", svc.into_value())
        .unwrap();
    assert_expression(&e, "${bean:svc?method=echo('10:30')}", "10:30");
}
