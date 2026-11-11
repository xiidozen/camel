//! Port of `SimplePredicateInBracesTest`: a predicate written inside `${ }` (CAMEL-24921).

use super::simple_test::{assert_expression, assert_predicate, exchange};
use camel_core::prelude::*;
use camel_core::util::value::ValueMap;

#[test]
fn test_predicate_inside_the_braces() {
    let (_t, mut e) = exchange();
    let mut item = ValueMap::new();
    item.insert("sku".to_string(), Value::from("CAMEL-MUG"));
    e.message_mut()
        .set_body(Value::List(vec![Value::Map(item)]));
    assert_predicate(&e, "${body != null && body.size() > 0}", true);
    assert_expression(&e, "${body != null && body.size() > 0}", "true");
    e.message_mut().set_body(Value::List(vec![]));
    assert_predicate(&e, "${body != null && body.size() > 0}", false);
}

#[test]
fn test_one_comparison() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("n", 5);
    assert_predicate(&e, "${header.n > 0}", true);
    assert_predicate(&e, "${header.n > 10}", false);
    assert_predicate(&e, "${header.n == 5}", true);
    e.message_mut().set_body("Hello");
    assert_predicate(&e, "${body contains 'ell'}", true);
    assert_predicate(&e, "${body.length() > 3}", true);
}

#[test]
fn test_both_forms_agree() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("n", 5);
    assert_predicate(&e, "${header.n > 0 && header.n < 10}", true);
    assert_predicate(&e, "${header.n} > 0 && ${header.n} < 10", true);
    assert_predicate(&e, "${header.n > 0 || header.n > 100}", true);
}

#[test]
fn test_a_value_in_text_is_still_a_template() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("n", 5);
    // outside the braces the expression is a template, and that has not changed
    assert_expression(&e, "${header.n} > 0", "5 > 0");
    // inside the braces it is the answer of the predicate, which a template can now hold
    assert_expression(
        &e,
        "Count ${header.n} is over three: ${header.n > 3}",
        "Count 5 is over three: true",
    );
}

#[test]
fn test_a_name_is_not_arithmetic() {
    // an operator counts only when whitespace surrounds it, so these are names as before
    let (_t, mut e) = exchange();
    e.message_mut().set_header("Content-Length", 42);
    assert_expression(&e, "${header.Content-Length}", "42");
    e.message_mut().set_header("aws-s3-bucket", "orders");
    assert_expression(&e, "${header.aws-s3-bucket}", "orders");
}

#[test]
fn test_an_operator_word_in_a_value_is_not_a_predicate() {
    // CAMEL-24963: the operator word is in a bracket key, which is text, not an operator (the
    // ${properties:..} cases of the Java test wait for the properties functions)
    let (_t, mut e) = exchange();
    e.message_mut().set_header("order in progress", "yes");
    assert_expression(&e, "${header[order in progress]}", "yes");
    // a closed bracket key can still be compared
    e.message_mut().set_header("n", 5);
    assert_predicate(&e, "${header[n] == 5}", true);
}

#[test]
fn test_the_ternary_still_wins() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("n", 5);
    assert_expression(&e, "${header.n > 0 ? 'positive' : 'negative'}", "positive");
    assert_expression(&e, "${header.n > 0 && header.n < 10 ? 'in' : 'out'}", "in");
}
