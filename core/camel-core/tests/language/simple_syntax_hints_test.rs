//! Port of `SimpleSyntaxHintsTest`: the parser errors say what to write (CAMEL-24703).

use super::simple_test::{evaluate, exchange};
use camel_core::prelude::*;
use camel_core::util::value::ValueMap;

fn predicate_error(e: &Exchange, text: &str) -> String {
    let simple = e.context().resolve_language("simple").unwrap();
    match simple.create_predicate(text) {
        Ok(_) => panic!("Should fail: {text}"),
        Err(err) => {
            assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err:?}");
            err.to_string()
        }
    }
}

fn expression_error(e: &Exchange, text: &str) -> String {
    let simple = e.context().resolve_language("simple").unwrap();
    match simple.create_expression(text) {
        Ok(_) => panic!("Should fail: {text}"),
        Err(err) => {
            assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err:?}");
            err.to_string()
        }
    }
}

/// The error of evaluating the expression.
fn evaluation_error(e: &Exchange, text: &str) -> String {
    let simple = e.context().resolve_language("simple").unwrap();
    let exp = simple.create_expression(text).unwrap();
    exp.evaluate(e).unwrap_err().to_string()
}

fn assert_contains(text: &str, part: &str) {
    assert!(text.contains(part), "{text:?} should contain {part:?}");
}

#[test]
fn test_semantic_function_hints() {
    // the last Java assertion ("requires camel-semantic") waits for the query functions
    let (_t, e) = exchange();
    assert_contains(&expression_error(&e, "${sematic('q')}"), "${semantic('q')}");
    assert_contains(&expression_error(&e, "${semantic:q}"), "${semantic('q')}");
    assert_contains(
        &expression_error(&e, "${semantic}"),
        "${semantic('evaluationName')}",
    );
}

#[test]
fn test_function_without_dollar_braces() {
    let (_t, e) = exchange();
    let msg = predicate_error(&e, "body == 'Hello World'");
    assert_contains(&msg, "text outside ${...} is a literal");
    assert_contains(&msg, "did you mean ${body} == 'Hello World'?");
    assert_contains(
        &predicate_error(&e, "header.foo == 'bar'"),
        "did you mean ${header.foo} == 'bar'?",
    );
}

#[test]
fn test_operator_inside_function() {
    // CAMEL-24921: the braces may hold a predicate, which is then what they answer
    let (t, mut e) = exchange();
    let simple = t.context.resolve_language("simple").unwrap();
    e.message_mut().set_body("x");
    assert!(simple
        .create_predicate("${body == 'x'}")
        .unwrap()
        .matches(&e)
        .unwrap());
    e.message_mut().set_body("y");
    assert!(!simple
        .create_predicate("${body == 'x'}")
        .unwrap()
        .matches(&e)
        .unwrap());
    // and what is inside must still be a predicate the parser understands, reported against
    // the wrapped text
    let msg = predicate_error(&e, "${body == }");
    assert_contains(&msg, "Unexpected token ==");
    assert_contains(&msg, "${body} ==");
}

#[test]
fn test_unquoted_value() {
    let (_t, e) = exchange();
    let msg = predicate_error(&e, "${body} == x");
    assert_contains(&msg, "does not accept x on the right hand side");
    assert_contains(&msg, "a quoted literal 'x'");
    assert!(!msg.contains("token null"));
    assert_contains(
        &predicate_error(&e, "${body} contains x"),
        "does not accept x on the right hand side",
    );
}

#[test]
fn test_unknown_operators() {
    let (_t, e) = exchange();
    assert_contains(
        &predicate_error(&e, "${body} = 'x'"),
        "Unknown operator =: did you mean ==?",
    );
    assert_contains(
        &predicate_error(&e, "${body} == 'x' and ${header.y} == 1"),
        "use && for and",
    );
    assert_contains(
        &predicate_error(&e, "${body} == 'x' || "),
        "needs a predicate on the right hand side",
    );
}

#[test]
fn test_quotes() {
    let (_t, e) = exchange();
    assert_contains(&predicate_error(&e, "${body} == 'it''s'"), "double quotes");
}

#[test]
fn test_missing_closing_brace() {
    let (_t, e) = exchange();
    assert_contains(&expression_error(&e, "${header.foo"), "missing } to close");
    assert_contains(
        &expression_error(&e, "${bodyAs(String)"),
        "missing } to close",
    );
}

#[test]
fn test_unknown_function_suggestions() {
    let (_t, e) = exchange();
    let msg = expression_error(&e, "${padding(3)}");
    assert_contains(&msg, "Unknown function: padding(3)");
    assert_contains(&msg, "did you mean ${pad(3)}?");
    for (text, hint) in [
        ("${property.foo}", "did you mean ${exchangeProperty.foo}?"),
        ("${Body}", "case sensitive: ${body}"),
        ("${ body }", "remove the spaces: ${body}"),
        ("${bodyy}", "did you mean ${body}?"),
        // CAMEL-24970: the suggestions name the simple functions
        ("${upper}", "did you mean ${uppercase()}?"),
        ("${avg(1,2)}", "did you mean ${average(1,2)}?"),
        ("${uppercse()}", "did you mean ${uppercase()}?"),
        ("${count}", "did you mean ${size()}?"),
    ] {
        assert_contains(&expression_error(&e, text), hint);
    }
}

#[test]
fn test_colon_argument_on_a_parenthesis_function() {
    // CAMEL-24845: jsonpath, jq, xpath and simpleJsonpath take their argument in parentheses
    let (_t, e) = exchange();
    let msg = expression_error(&e, "${jsonpath:$.status}");
    assert_contains(&msg, "Unknown function: jsonpath:$.status");
    assert_contains(
        &msg,
        "the argument goes in parentheses: did you mean ${jsonpath($.status)}?",
    );
    for (text, hint) in [
        ("${jq:.name}", "${jq(.name)}"),
        ("${xpath:/order/@id}", "${xpath(/order/@id)}"),
        ("${simpleJsonpath:$.status}", "${simpleJsonpath($.status)}"),
        // the json alias resolves to jsonpath, so its argument moves into parentheses as well
        ("${json:$.status}", "${jsonpath($.status)}"),
    ] {
        assert_contains(
            &expression_error(&e, text),
            &format!("the argument goes in parentheses: did you mean {hint}?"),
        );
    }
}

#[test]
fn test_bare_query_function_is_not_called_a_nested_language() {
    let (_t, e) = exchange();
    let msg = expression_error(&e, "${jsonpath}");
    assert_contains(
        &msg,
        "the argument goes in parentheses: did you mean ${jsonpath(exp)}?",
    );
    assert!(!msg.contains("is a language"));
    assert_contains(
        &expression_error(&e, "${jq}"),
        "the argument goes in parentheses: did you mean ${jq(exp)}?",
    );
}

#[test]
fn test_operator_after_ognl_method() {
    // CAMEL-24921: an OGNL call on the left of the operator is wrapped as the function it is
    let (t, mut e) = exchange();
    let simple = t.context.resolve_language("simple").unwrap();
    e.message_mut().set_body("hello");
    assert!(simple
        .create_predicate("${body.length() > 3}")
        .unwrap()
        .matches(&e)
        .unwrap());
    e.message_mut().set_body("hi");
    assert!(!simple
        .create_predicate("${body.length() > 3}")
        .unwrap()
        .matches(&e)
        .unwrap());
}

#[test]
fn test_ognl_runtime_messages() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("hello");
    let msg = evaluation_error(&e, "${body.lenght()}");
    assert_contains(&msg, "on the message body of type java.lang.String");
    assert_contains(&msg, "did you mean length()?");
    assert!(!msg.contains("on null"));
}

fn map(entries: &[(&str, Value)]) -> Value {
    let mut m = ValueMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v.clone());
    }
    Value::Map(m)
}

#[test]
fn test_ognl_dot_on_a_map_reads_the_key() {
    // CAMEL-24916: a map has no method type, so the key is what the dot can mean
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(map(&[("type", Value::from("order"))]));
    assert_eq!(Value::from("order"), evaluate(&e, "${body.type}"));
    assert_eq!(Value::from("order"), evaluate(&e, "${body[type]}"));
}

#[test]
fn test_ognl_dot_on_a_map_without_that_key_says_to_use_a_key() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(map(&[("type", Value::from("order"))]));
    // CAMEL-25322: the dot reads a key, so the hint says the key is missing and which keys
    // there are
    assert_contains(
        &evaluation_error(&e, "${body.typo}"),
        "(the Map has no key typo; its keys are: type)",
    );
}

#[test]
fn test_ognl_dot_on_an_empty_map_says_it_is_empty() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(map(&[]));
    assert_contains(
        &evaluation_error(&e, "${body.orderId}"),
        "(the Map has no key orderId; it is empty)",
    );
}

#[test]
fn test_ognl_dot_on_a_map_with_a_null_value_answers_null() {
    // a key that is there and holds null is a value, not a missing key
    let (_t, mut e) = exchange();
    e.message_mut().set_body(map(&[("sku", Value::Null)]));
    assert_eq!(Value::Null, evaluate(&e, "${body.sku}"));
}

#[test]
fn test_a_method_of_a_map_still_wins() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(map(&[("size", Value::from("not the size"))]));
    // size() is a method of Map, so it still answers before the key
    assert_eq!("1", evaluate(&e, "${body.size}").to_string());
}

#[test]
fn test_ognl_dot_on_a_nested_map_reads_the_key() {
    let (_t, mut e) = exchange();
    let item = map(&[("sku", Value::from("CAMEL-MUG"))]);
    e.message_mut().set_body(map(&[("item", item)]));
    assert_eq!(Value::from("CAMEL-MUG"), evaluate(&e, "${body.item.sku}"));
}

#[test]
fn test_arithmetic_in_a_function_says_there_is_none() {
    let (_t, e) = exchange();
    let msg = expression_error(&e, "${exchangeCounter % 3}");
    assert_contains(&msg, "Unknown function: exchangeCounter % 3");
    assert_contains(&msg, "simple has no arithmetic operators");
}

#[test]
fn test_language_name_as_a_function_is_named() {
    let (_t, e) = exchange();
    let msg = expression_error(&e, "${simple}[${date:now:HH:mm}] size ${body.length}");
    assert_contains(&msg, "Unknown function: simple");
    assert_contains(&msg, "simple is the language, not a function");
    let msg = expression_error(&e, "${groovy(body.size())}");
    assert_contains(&msg, "groovy is a language, not a simple function");
    assert_contains(&msg, "groovy: \"...\"");
}

#[test]
fn test_ognl_method_without_parentheses() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("hello");
    assert_contains(
        &evaluation_error(&e, "${body.toUpperCase}"),
        "a method call needs parentheses: toUpperCase()",
    );
}

#[test]
fn test_missing_language_names_the_dependency() {
    // xquery is a built-in language whose crate (camel-saxon) is not a dependency (Java: on
    // the classpath)
    let (t, _e) = exchange();
    let msg = t
        .context
        .resolve_language("xquery")
        .err()
        .unwrap()
        .to_string();
    assert_contains(&msg, "No language could be found for: xquery");
    assert_contains(
        &msg,
        "the xquery language is in camel-saxon; add camel-saxon to the dependencies)",
    );
}

#[test]
fn test_unknown_language_says_did_you_mean() {
    let (t, _e) = exchange();
    let msg = t
        .context
        .resolve_language("simpel")
        .err()
        .unwrap()
        .to_string();
    assert_contains(&msg, "No language could be found for: simpel");
    assert_contains(
        &msg,
        "not a built-in Camel language; did you mean 'simple'?",
    );
    let msg = t
        .context
        .resolve_language("cheese")
        .err()
        .unwrap()
        .to_string();
    assert_contains(&msg, "No language could be found for: cheese");
    assert_contains(&msg, "(not a built-in Camel language)");
    assert!(!msg.contains("did you mean"));
}

#[test]
fn test_file_function_does_not_read_a_file() {
    let (_t, e) = exchange();
    let msg = expression_error(&e, "${file:src/main/resources/input.xml}");
    assert_contains(&msg, "Unknown file language syntax");
    assert_contains(&msg, "they do not read a file");
    assert_contains(&msg, "poll EIP");
}
