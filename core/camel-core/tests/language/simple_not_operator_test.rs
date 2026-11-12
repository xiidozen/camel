//! Port of `SimpleNotOperatorTest` (the cases negating Java AST nodes directly are not
//! ported: the Rust parser has no public node API).

use crate::simple_test::{evaluate, exchange};
use camel_core::prelude::*;

fn predicate(e: &Exchange, text: &str) -> CamelResult<bool> {
    let simple = e.context().resolve_language("simple")?;
    simple.create_predicate(text)?.matches(e)
}

fn map_body() -> (crate::support::ContextTestSupport, Exchange) {
    let (t, mut e) = exchange();
    let mut m = ValueMap::new();
    m.insert("a".into(), Value::from(1));
    e.message_mut().set_body(Value::Map(m));
    (t, e)
}

#[test]
fn test_negates_a_function() {
    let (_t, e) = map_body();
    assert!(predicate(&e, "!${body.isEmpty()}").unwrap());
    assert!(!predicate(&e, "!${body.containsKey('a')}").unwrap());
    // with logical operators
    assert!(predicate(&e, "!${body.isEmpty()} && ${body} != null").unwrap());
    assert!(predicate(&e, "${body} == null || !${body.isEmpty()}").unwrap());
    assert!(predicate(&e, "${body != null && !${body.isEmpty()}}").unwrap());
    // inside the braces
    assert!(predicate(&e, "${!body.isEmpty()}").unwrap());
    assert!(!predicate(&e, "${!body.containsKey('a')}").unwrap());
    assert!(predicate(&e, "${body != null && !body.isEmpty()}").unwrap());
    assert!(predicate(&e, "${!body.isEmpty() && body != null}").unwrap());
    // negates only its own function
    assert!(!predicate(&e, "${!body.isEmpty() && body == null}").unwrap());
    assert!(!predicate(&e, "!${body.isEmpty()} && ${body} == null").unwrap());
    // the opposite of the predicate
    assert!(!predicate(&e, "!${body}").unwrap());
    assert!(predicate(&e, "!${header.missing}").unwrap());
}

#[test]
fn test_cannot_negate_a_comparison() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("y");
    let err = predicate(&e, "!${body} == 'x'").unwrap_err();
    let m = err.message().unwrap_or_default().to_string();
    assert!(err.is(ErrorKind::ExpressionIllegalSyntax), "{err}");
    assert!(m.contains("! cannot be compared"), "{m}");
    assert!(m.contains("${...} != value"), "{m}");
    assert!(!m.contains("!=!="), "{m}");
    assert!(predicate(&e, "${body} == !${body}")
        .unwrap_err()
        .is(ErrorKind::ExpressionIllegalSyntax));
    assert!(predicate(&e, "${body} != 'x'").unwrap());
}

#[test]
fn test_says_how_to_write_a_negation_it_cannot_read() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("x");
    let err = predicate(&e, "${body} != null && !someFlag").unwrap_err();
    let m = err.message().unwrap_or_default().to_string();
    assert!(
        m.contains("! negates a function, which is written as ${ }"),
        "{m}"
    );
    assert!(m.contains("!${someFlag}"), "{m}");
    assert!(predicate(&e, "${body} !contains 'zz'").unwrap());
}

#[test]
fn test_empty_body_and_negated_operators() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::Map(ValueMap::new()));
    assert!(!predicate(&e, "!${body.isEmpty()}").unwrap());
    e.message_mut().set_body("Hello");
    for p in [
        "${body} != 'x'",
        "${body} !contains 'zz'",
        "${body} !startsWith 'zz'",
        "${body} !endsWith 'zz'",
        "${body} !in 'a,b'",
    ] {
        assert!(predicate(&e, p).unwrap(), "{p}");
    }
}

#[test]
fn test_an_exclamation_mark_in_text_is_not_an_operator() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("World");
    for (text, expected) in [
        ("Hello ${body}! how are you", "Hello World! how are you"),
        ("!aaa! is a weird text", "!aaa! is a weird text"),
        ("Alert: !${body}", "Alert: !World"),
        ("Order ${body}!!!", "Order World!!!"),
    ] {
        assert_eq!(Value::from(expected), evaluate(&e, text), "{text}");
    }
    e.message_mut().set_body("Hello!");
    assert!(predicate(&e, "${body} == 'Hello!'").unwrap());
    assert!(predicate(&e, "${body} contains '!'").unwrap());
}
