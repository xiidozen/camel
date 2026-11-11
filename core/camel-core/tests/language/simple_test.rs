//! Port of `SimpleTest` (in progress: the tests of the ported functions; the others are still
//! in the Java test, see the backlog).

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::util::value::ValueMap;

/// The Java `ExchangeTestSupport` exchange: headers foo=abc, bar=123, a body, a property and a
/// variable.
pub(crate) fn exchange() -> (ContextTestSupport, Exchange) {
    let t = ContextTestSupport::new();
    let mut e = Exchange::new(t.context.as_shared());
    e.message_mut().set_header("foo", "abc");
    e.message_mut().set_header("bar", 123);
    e.message_mut().set_body("<hello id='m123'>world!</hello>");
    e.set_property("foobar", "cba");
    e.set_variable("cheese", "gauda");
    (t, e)
}

pub(crate) fn evaluate(e: &Exchange, text: &str) -> Value {
    let simple = e.context().resolve_language("simple").unwrap();
    simple.create_expression(text).unwrap().evaluate(e).unwrap()
}

/// Java `assertExpression`: the value converted to the type of the expected value.
pub(crate) fn assert_expression(e: &Exchange, text: &str, expected: impl Into<Value>) {
    let expected = expected.into();
    let v = evaluate(e, text);
    let v = match &expected {
        Value::Null => v,
        Value::Text(_) => <String as camel_core::api::convert::FromValue>::from_value(&v)
            .unwrap()
            .map(Value::Text)
            .unwrap_or_default(),
        Value::Int(_) => <i64 as camel_core::api::convert::FromValue>::from_value(&v)
            .unwrap()
            .map(Value::Int)
            .unwrap_or_default(),
        Value::Bool(_) => <bool as camel_core::api::convert::FromValue>::from_value(&v)
            .unwrap()
            .map(Value::Bool)
            .unwrap_or_default(),
        _ => v,
    };
    assert_eq!(expected, v, "Expression: {text}");
}

/// Java `assertPredicate`.
pub(crate) fn assert_predicate(e: &Exchange, text: &str, expected: bool) {
    let simple = e.context().resolve_language("simple").unwrap();
    let p = simple.create_predicate(text).unwrap();
    assert_eq!(expected, p.matches(e).unwrap(), "Predicate: {text}");
}

fn string(e: &Exchange, text: &str) -> Option<String> {
    let v = evaluate(e, text);
    if v.is_null() {
        None
    } else {
        Some(v.to_string())
    }
}

#[test]
fn test_trim_simple_expressions() {
    let (_t, e) = exchange();
    assert_expression(&e, " \t${exchangeId}\n".trim(), e.exchange_id().to_string());
    assert_expression(&e, "\n${id}\r".trim(), e.exchange_id().to_string());
    assert_expression(&e, "\t\r ${body}".trim(), "<hello id='m123'>world!</hello>");
    assert_expression(
        &e,
        "\n${in.body}\r".trim(),
        "<hello id='m123'>world!</hello>",
    );
}

#[test]
fn test_simple_hostname() {
    let (_t, e) = exchange();
    let name = camel_core::util::host_utils::get_local_host_name_safe().unwrap_or_default();
    assert_expression(&e, "${hostname}", name.clone());
    assert_expression(&e, "The host is ${hostname}", format!("The host is {name}"));
}

#[test]
fn test_escape() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Something");
    assert_expression(&e, "\\foo", "\\foo");
    assert_expression(&e, "\\n${body}", "\nSomething");
    assert_expression(&e, "\\t${body}", "\tSomething");
    assert_expression(&e, "\\r${body}", "\rSomething");
    assert_expression(&e, "\\n\\r${body}", "\n\rSomething");
    assert_expression(&e, "\\n${body}\\n", "\nSomething\n");
    assert_expression(&e, "\\t${body}\\t", "\tSomething\t");
    assert_expression(&e, "\\r${body}\\r", "\rSomething\r");
    assert_expression(&e, "\\n\\r${body}\\n\\r", "\n\rSomething\n\r");
    assert_expression(&e, "$${body}", "$Something");
}

#[test]
fn test_escape_end_function() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Something");
    assert_expression(&e, "{hello\\}", "{hello}");
    assert_expression(&e, "${body}{hello\\}", "Something{hello}");
}

#[test]
fn test_replace_all_expression() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello a how are you");
    assert_expression(&e, "${replace(a,b)}", "Hello b how bre you");
    e.message_mut().set_body("{\"foo\": \"cheese\"}");
    assert_expression(&e, "${replace(&quot;,&apos;)}", "{'foo': 'cheese'}");
    e.message_mut().set_body("{'foo': 'cheese'}");
    assert_expression(&e, "${replace(&apos;,&quot;)}", "{\"foo\": \"cheese\"}");
    e.message_mut().set_body("{\"foo\": \"cheese\"}");
    assert_expression(&e, "${replace(&quot;,&empty;)}", "{foo: cheese}");
    e.message_mut().set_body("Hello");
    e.message_mut().set_header("foo", "{\"foo\": \"cheese\"}");
    assert_expression(
        &e,
        "${replace(&quot;,&apos;,${header.foo})}",
        "{'foo': 'cheese'}",
    );
}

#[test]
fn test_substring_expression() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("ABCDEFGHIJK");
    for (exp, expected) in [
        ("${substring(0)}", "ABCDEFGHIJK"),
        ("${substring(1)}", "BCDEFGHIJK"),
        ("${substring(3)}", "DEFGHIJK"),
        ("${substring(99)}", ""),
        ("${substring(-1)}", "ABCDEFGHIJ"),
        ("${substring(-3)}", "ABCDEFGH"),
        ("${substring(-99)}", ""),
        ("${substring(1,-1)}", "BCDEFGHIJ"),
        ("${substring(3,-3)}", "DEFGH"),
        ("${substring(1,-3)}", "BCDEFGH"),
        ("${substring(3,-1)}", "DEFGHIJ"),
        ("${substring(0,-1)}", "ABCDEFGHIJ"),
        ("${substring(1,0)}", "BCDEFGHIJK"),
        ("${substring(99,-99)}", ""),
        ("${substring(0,-99)}", ""),
        ("${substring(99,0)}", ""),
        ("${substring(0,0)}", "ABCDEFGHIJK"),
        ("${substring(3,-2)}", "DEFGHI"),
        ("${substring(3,2)}", "DEFGHI"),
    ] {
        assert_expression(&e, exp, expected);
    }
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${substring(6)}", "World");
    assert_expression(&e, "${substring(-6)}", "Hello");
    assert_expression(&e, "${substring(1,-1)}", "ello Worl");
    e.message_mut().set_header("foo", "1234567890");
    for (exp, expected) in [
        ("${substring(0,0,${header.foo})}", "1234567890"),
        ("${substring(1,0,${header.foo})}", "234567890"),
        ("${substring(3,0,${header.foo})}", "4567890"),
        ("${substring(99,0,${header.foo})}", ""),
        ("${substring(0,-1,${header.foo})}", "123456789"),
        ("${substring(0,-3,${header.foo})}", "1234567"),
        ("${substring(0,-99,${header.foo})}", ""),
        ("${substring(1,-1,${header.foo})}", "23456789"),
        ("${substring(3,-3,${header.foo})}", "4567"),
    ] {
        assert_expression(&e, exp, expected);
    }
}

// testSize: Java arrays are lists in Rust; a File body comes with the file component
#[test]
fn test_size() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::List(vec![
        Value::from(4),
        Value::from(7),
        Value::from(9),
    ]));
    assert_expression(&e, "${size()}", 3);
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${size()}", 1);
    e.message_mut().set_body(Value::Null);
    assert_expression(&e, "${size()}", 0);
    let mut m = ValueMap::new();
    for (k, v) in [("A", 1), ("B", 2), ("C", 3)] {
        m.insert(k.into(), Value::from(v));
    }
    e.message_mut().set_body(Value::Map(m));
    assert_expression(&e, "${size()}", 3);
}

// testLength: a list has the length of its text (as a Java List); byte arrays their size
#[test]
fn test_length() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::Bytes(vec![4, 7, 9]));
    assert_expression(&e, "${length()}", 3);
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${length()}", 11);
    e.message_mut().set_body(Value::List(
        ["A", "BB", "CCC", "DDDD"]
            .into_iter()
            .map(Value::from)
            .collect(),
    ));
    assert_expression(&e, "${length()}", 18);
    let mut m = ValueMap::new();
    for (k, v) in [("A", 1), ("BB", 22), ("CC", 333)] {
        m.insert(k.into(), Value::from(v));
    }
    e.message_mut().set_body(Value::Map(m));
    assert_expression(&e, "${length()}", 20);
}

#[test]
fn test_capitalize() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("hello world how are you");
    assert_expression(&e, "${capitalize()}", "Hello World How Are You");
    assert_expression(&e, "${capitalize(${body})}", "Hello World How Are You");
    e.message_mut().set_header("beer", "carlsberg is a Beer");
    assert_expression(&e, "${capitalize(${header.beer})}", "Carlsberg Is A Beer");
}

#[test]
fn test_pad() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("foo");
    assert_expression(&e, "${pad(${body},5)}", "foo  ");
    assert_expression(&e, "${pad(${body},-5)}", "  foo");
    assert_expression(&e, "${pad(${body},5,#)}", "foo##");
    assert_expression(&e, "${pad(${body},-5,#)}", "##foo");
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${pad(${body},5)}", "Hello World");
}

#[test]
fn test_quote() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${quote()}", "\"Hello World\"");
    assert_expression(&e, "${quote(${body})}", "\"Hello World\"");
    assert_expression(&e, "${quote('Hi')}", "\"Hi\"");
    assert_expression(&e, "${quote(''Hi'')}", "\"Hi\"");
}

#[test]
fn test_safe_quote() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${safeQuote()}", "\"Hello World\"");
    assert_expression(&e, "${safeQuote(${body})}", "\"Hello World\"");
    assert_expression(&e, "${safeQuote('Hi')}", "\"Hi\"");
    assert_expression(&e, "${safeQuote(''Hi'')}", "\"Hi\"");
    e.message_mut().set_body(123);
    assert_expression(&e, "${safeQuote()}", "123");
    assert_expression(&e, "${safeQuote(${body})}", "123");
    e.message_mut().set_body(true);
    assert_expression(&e, "${safeQuote()}", "true");
    assert_expression(&e, "${safeQuote(${body})}", "true");
    let mut m = ValueMap::new();
    m.insert("A".into(), Value::from("1"));
    m.insert("B".into(), Value::from("2"));
    e.message_mut().set_body(Value::Map(m));
    assert_expression(&e, "${safeQuote()}", "\"{A=1, B=2}\"");
}

#[test]
fn test_unquote() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("\"Hello World\"");
    assert_expression(&e, "${unquote()}", "Hello World");
    assert_expression(&e, "${unquote(${body})}", "Hello World");
    assert_expression(&e, "${unquote('\"Hi\"')}", "Hi");
    assert_expression(&e, "${unquote('Hi')}", "Hi");
}

#[test]
fn test_trim() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("   Hello World ");
    assert_expression(&e, "${trim()}", "Hello World");
    assert_expression(&e, "${trim(${body})}", "Hello World");
    assert_expression(&e, "${trim(' Hi  ')}", "Hi");
    e.message_mut().set_header("beer", "  Carlsberg");
    assert_expression(&e, "${trim(${header.beer})}", "Carlsberg");
}

#[test]
fn test_substring_before() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${substringBefore('World')}", "Hello ");
    assert_expression(&e, "${substringBefore(' World')}", "Hello");
    assert_expression(&e, "${trim(${substringBefore('World')})}", "Hello");
    assert_expression(&e, "${substringBefore(${body},'World')}", "Hello ");
    assert_eq!(None, string(&e, "${substringBefore('Unknown')}"));
    e.message_mut().set_header("place", "World");
    assert_expression(&e, "${substringBefore(${body},${header.place})}", "Hello ");
}

#[test]
fn test_substring_after() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${substringAfter('Hello')}", " World");
    assert_expression(&e, "${substringAfter('Hello ')}", "World");
    assert_expression(&e, "${substringAfter(${body},'Hello')}", " World");
    assert_expression(&e, "${trim(${substringAfter(${body},'Hello')})}", "World");
    assert_eq!(None, string(&e, "${substringAfter('Unknown')}"));
    e.message_mut().set_header("place", "Hello");
    assert_expression(&e, "${substringAfter(${body},${header.place})}", " World");
}

#[test]
fn test_substring_between() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello big great World");
    assert_expression(&e, "${substringBetween('Hello','World')}", " big great ");
    assert_expression(&e, "${substringBetween('Hello ',' World')}", "big great");
    assert_expression(&e, "${substringBetween(${body},'big ',' World')}", "great");
    assert_expression(
        &e,
        "${trim(${substringBetween(${body},'big','World')})}",
        "great",
    );
    assert_eq!(None, string(&e, "${substringBetween('Hello','Unknown')}"));
    e.message_mut().set_header("place", "Hello");
    e.message_mut().set_header("place2", "great");
    assert_expression(
        &e,
        "${substringBetween(${body},${header.place},${header.place2})}",
        " big ",
    );
}

#[test]
fn test_concat() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello");
    assert_expression(&e, "${concat(' World')}", "Hello World");
    assert_expression(&e, "${concat(${body}, ' World')}", "Hello World");
    assert_expression(&e, "${concat('a','b')}", "ab");
    assert_expression(&e, "${concat(${body}, 'World', '_')}", "Hello_World");
    assert_expression(&e, "${concat('World ', ${body})}", "World Hello");
    e.message_mut().set_header("beer", "Carlsberg");
    assert_expression(&e, "${concat(${header.beer})}", "HelloCarlsberg");
    assert_expression(
        &e,
        "${concat(${body}, ${header.beer}, ' ')}",
        "Hello Carlsberg",
    );
}

#[test]
fn test_uppercase() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${uppercase()}", "HELLO WORLD");
    assert_expression(&e, "${uppercase(${body})}", "HELLO WORLD");
    assert_expression(&e, "${uppercase('Hi')}", "HI");
    e.message_mut().set_header("beer", "Carlsberg");
    assert_expression(&e, "${uppercase(${header.beer})}", "CARLSBERG");
}

#[test]
fn test_lowercase() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${lowercase()}", "hello world");
    assert_expression(&e, "${lowercase(${body})}", "hello world");
    assert_expression(&e, "${lowercase('Hi')}", "hi");
    e.message_mut().set_header("beer", "Carlsberg");
    assert_expression(&e, "${lowercase(${header.beer})}", "carlsberg");
}

#[test]
fn test_val() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(123);
    assert_expression(&e, "${val(abc)}", "abc");
    assert_expression(&e, "${val(${body})}", "123");
    assert_eq!(Value::Int(123), evaluate(&e, "${val(${body})}"));
}

#[test]
fn test_normalize_whitespace() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("   Hello  big   World      ");
    assert_expression(&e, "${normalizeWhitespace()}", "Hello big World");
    assert_expression(&e, "${normalizeWhitespace(${body})}", "Hello big World");
    assert_expression(
        &e,
        "${normalizeWhitespace(' Hi   from    me  ')}",
        "Hi from me",
    );
    e.message_mut()
        .set_header("beer", "  Carlsberg    is a    beer ");
    assert_expression(
        &e,
        "${normalizeWhitespace(${header.beer})}",
        "Carlsberg is a beer",
    );
}

fn list(values: &[i64]) -> Value {
    Value::List(values.iter().map(|v| Value::Int(*v)).collect())
}

fn text_list(values: &[&str]) -> Value {
    Value::List(values.iter().map(|v| Value::from(*v)).collect())
}

/// Runs `(expression, expected)` cases on the body.
fn cases(e: &mut Exchange, body: impl Into<Value>, cases: &[(&str, i64)]) {
    e.message_mut().set_body(body);
    for (exp, expected) in cases {
        assert_expression(e, exp, *expected);
    }
}

#[test]
fn test_sum() {
    let (_t, mut e) = exchange();
    cases(
        &mut e,
        "4",
        &[
            ("${sum(1,2,3)}", 6),
            ("${sum(${body},1)}", 5),
            ("${sum(${body},-1)}", 3),
            ("${sum(${body},0)}", 4),
            ("${sum(${body},${body},-1)}", 7),
            ("${sum(1,2,3,4,5,6,7,8,9)}", 45),
        ],
    );
    cases(&mut e, list(&[4, 7, 9]), &[("${sum(${body})}", 20)]);
    cases(&mut e, "4,7,8", &[("${sum(${body})}", 19)]);
    cases(
        &mut e,
        text_list(&["4", "7", "7"]),
        &[("${sum(${body},-8)}", 10)],
    );
}

#[test]
fn test_average() {
    let (_t, mut e) = exchange();
    cases(
        &mut e,
        "4",
        &[
            ("${average(1,2,3)}", 2),
            ("${average(${body},1)}", 2),
            ("${average(${body},-1)}", 1),
            ("${average(${body},0)}", 2),
            ("${average(${body},${body},-1)}", 2),
            ("${average(1,2,3,4,5,6,7,8,9)}", 5),
        ],
    );
    cases(&mut e, list(&[4, 7, 9]), &[("${average(${body})}", 6)]);
    cases(&mut e, "4,7,8", &[("${average(${body})}", 6)]);
    cases(
        &mut e,
        text_list(&["4", "7", "7"]),
        &[("${average(${body},-8)}", 2)],
    );
}

#[test]
fn test_min() {
    let (_t, mut e) = exchange();
    cases(
        &mut e,
        "4",
        &[
            ("${min(1,2,6,4)}", 1),
            ("${min(${body},2)}", 2),
            ("${min(${body},-1)}", -1),
            ("${min(${body},0)}", 0),
            ("${min(${body},${body},-1)}", -1),
            ("${min(1,2,3,4,5,6,7,8,9)}", 1),
        ],
    );
    cases(&mut e, list(&[4, 9, 7]), &[("${min(${body})}", 4)]);
    cases(
        &mut e,
        text_list(&["4", "7", "7"]),
        &[("${min(${body},-8,11,6)}", -8)],
    );
}

#[test]
fn test_max() {
    let (_t, mut e) = exchange();
    cases(
        &mut e,
        "4",
        &[
            ("${max(1,2,6,4)}", 6),
            ("${max(${body},2)}", 4),
            ("${max(${body},-1)}", 4),
            ("${max(${body},0)}", 4),
            ("${max(${body},${body},-1)}", 4),
            ("${max(1,2,3,4,5,6,7,8,9)}", 9),
        ],
    );
    cases(&mut e, list(&[4, 9, 7]), &[("${max(${body})}", 9)]);
    cases(
        &mut e,
        text_list(&["4", "7", "7"]),
        &[("${max(${body},-8,11,6)}", 11)],
    );
}

#[test]
fn test_floor() {
    let (_t, mut e) = exchange();
    cases(
        &mut e,
        "5.3",
        &[
            ("${floor()}", 5),
            ("${floor(${body})}", 5),
            ("${floor(6)}", 6),
            ("${floor(6.0)}", 6),
            ("${floor(6.8)}", 6),
            ("${floor(-12.9)}", -13),
            ("${floor(0.0)}", 0),
        ],
    );
    e.message_mut().set_header("myNum", "234.56");
    assert_expression(&e, "${floor(${header.myNum})}", "234");
}

#[test]
fn test_ceil() {
    let (_t, mut e) = exchange();
    cases(
        &mut e,
        "5.3",
        &[
            ("${ceil()}", 6),
            ("${ceil(${body})}", 6),
            ("${ceil(6)}", 6),
            ("${ceil(6.0)}", 6),
            ("${ceil(6.1)}", 7),
            ("${ceil(-12.9)}", -12),
            ("${ceil(0.0)}", 0),
        ],
    );
    e.message_mut().set_header("myNum", "234.56");
    assert_expression(&e, "${ceil(${header.myNum})}", "235");
}

#[test]
fn test_abs() {
    let (_t, mut e) = exchange();
    cases(&mut e, "-987", &[("${abs()}", 987), ("${abs(-5)}", 5)]);
    assert_expression(&e, "${abs(${body})}", "987");
    e.message_mut().set_header("myVal", "0");
    assert_expression(&e, "${abs(${header.myVal})}", "0");
    e.message_mut().set_header("myVal", -222);
    assert_expression(&e, "${abs(${header.myVal})}", "222");
}

fn boolean(e: &Exchange, text: &str) -> bool {
    match evaluate(e, text) {
        Value::Bool(b) => b,
        other => panic!("{text} evaluated to {other:?}"),
    }
}

#[test]
fn test_is_empty() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("");
    for exp in [
        "${isEmpty()}",
        "${isEmpty(${body})}",
        "${isEmpty(' ')}",
        "${isEmpty('   ')}",
        "${isEmpty(${empty(map)})}",
    ] {
        assert!(boolean(&e, exp), "{exp}");
    }
    assert!(!boolean(&e, "${isEmpty('Hello World')}"));
    e.message_mut().set_body(Value::Map(ValueMap::new()));
    assert!(boolean(&e, "${isEmpty()}"));
    e.message_mut().set_body(text_list(&["A", "B"]));
    assert!(!boolean(&e, "${isEmpty()}"));
}

#[test]
fn test_is_alpha() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("HelloWorld");
    for (exp, expected) in [
        ("${isAlpha()}", true),
        ("${isAlpha(${body})}", true),
        ("${isAlpha(3)}", false),
        ("${isAlpha('')}", false),
        ("${isAlpha(' ')}", false),
        ("${isAlpha('HiIamHere')}", true),
        ("${isAlpha('Hi_I_am_here')}", false),
        ("${isAlpha('Hi I am here!')}", false),
    ] {
        assert_eq!(expected, boolean(&e, exp), "{exp}");
    }
    e.message_mut().set_body(Value::Bytes(b"Hello".to_vec()));
    assert!(boolean(&e, "${isAlpha()}"));
    e.message_mut().set_body(Value::Bytes(b"Hello123".to_vec()));
    assert!(!boolean(&e, "${isAlpha()}"));
}

#[test]
fn test_is_alpha_numeric() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello123");
    for (exp, expected) in [
        ("${isAlphaNumeric()}", true),
        ("${isAlphaNumeric(${body})}", true),
        ("${isAlphaNumeric(3)}", true),
        ("${isAlphaNumeric('A')}", true),
        ("${isAlphaNumeric('')}", false),
        ("${isAlphaNumeric('!')}", false),
        ("${isAlphaNumeric('HiIamHere')}", true),
        ("${isAlphaNumeric('Hi_I_am_here')}", false),
        ("${isAlphaNumeric('Hi I am here!')}", false),
    ] {
        assert_eq!(expected, boolean(&e, exp), "{exp}");
    }
    for (body, expected) in [
        (&b"Hello"[..], true),
        (b"Hello123", true),
        (b"Hello123!", false),
    ] {
        e.message_mut().set_body(Value::Bytes(body.to_vec()));
        assert_eq!(expected, boolean(&e, "${isAlphaNumeric()}"));
    }
}

#[test]
fn test_is_numeric() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(123);
    for (exp, expected) in [
        ("${isNumeric()}", true),
        ("${isNumeric(${body})}", true),
        ("${isNumeric(3)}", true),
        ("${isNumeric('-4')}", false),
        ("${isNumeric('1.99')}", false),
        ("${isNumeric('')}", false),
        ("${isNumeric('Hello')}", false),
    ] {
        assert_eq!(expected, boolean(&e, exp), "{exp}");
    }
    e.message_mut().set_body(Value::Bytes(b"Hello".to_vec()));
    assert!(!boolean(&e, "${isNumeric()}"));
    e.message_mut().set_body(Value::Bytes(b"123".to_vec()));
    assert!(boolean(&e, "${isNumeric()}"));
}

#[test]
fn test_not() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("");
    assert!(boolean(&e, "${not()}"));
    e.message_mut().set_body("Hello");
    assert!(!boolean(&e, "${not()}"));
    assert!(!boolean(&e, "${not(${body} == 'Hello')}"));
    e.message_mut().set_body("Bye");
    assert!(boolean(&e, "${not(${body} == 'Hello')}"));
}

#[test]
fn test_kind_of_type() {
    let (_t, mut e) = exchange();
    let cases: Vec<(Value, &str)> = vec![
        (Value::Null, "null"),
        (Value::from("Hello"), "string"),
        (Value::from(123), "number"),
        (Value::from(98.76), "number"),
        (Value::from(true), "boolean"),
        (text_list(&["A", "B"]), "array"),
        (Value::Bytes(b"abc".to_vec()), "array"),
        (
            Value::object(camel_core::api::spi::uuid::DefaultUuidGenerator::new()),
            "object",
        ),
    ];
    for (body, expected) in cases {
        e.message_mut().set_body(body);
        assert_expression(&e, "${kindOfType()}", expected);
        assert_expression(&e, "${kindOfType(${body})}", expected);
    }
}

#[test]
fn test_throw_exception() {
    let (_t, e) = exchange();
    let simple = e.context().resolve_language("simple").unwrap();
    let err = simple
        .create_expression("${throwException('Forced error')}")
        .unwrap()
        .evaluate(&e)
        .unwrap_err();
    assert!(err.is(ErrorKind::IllegalArgument));
    assert_eq!(Some("Forced error"), err.message());
    let err = simple
        .create_expression("${throwException('Some IO error','java.io.IOException')}")
        .unwrap()
        .evaluate(&e)
        .unwrap_err();
    let cause = std::error::Error::source(&err)
        .and_then(|c| c.downcast_ref::<CamelError>())
        .unwrap();
    assert!(cause.is(ErrorKind::Io), "{cause:?}");
    assert_eq!(Some("Some IO error"), cause.message());
}

// testConvertTo: the OGNL method calls on the converted value (.repeat(2), .substring(2)) are
// Java methods, not available in Rust
#[test]
fn test_convert_to() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_eq!(
        Value::Bytes(b"Hello World".to_vec()),
        evaluate(&e, "${convertTo(byte[])}")
    );
    assert_eq!(
        Value::Bytes(b"Hello World".to_vec()),
        evaluate(&e, "${convertTo(${body},byte[])}")
    );
    e.message_mut().set_body("987");
    assert_eq!(Value::Int(987), evaluate(&e, "${convertTo(int)}"));
    e.message_mut().set_body("true");
    assert_eq!(Value::Bool(true), evaluate(&e, "${convertTo(boolean)}"));
}

#[test]
fn test_uuid() {
    let (t, e) = exchange();
    for exp in [
        "${uuid}",
        "${uuid(default)}",
        "${uuid(short)}",
        "${uuid(simple)}",
        "${uuid(classic)}",
        "${uuid()}",
        "${uuid(Short)}",
    ] {
        assert!(string(&e, exp).is_some(), "{exp}");
    }
    struct MyGen;
    impl camel_core::api::spi::uuid::UuidGenerator for MyGen {
        fn generate_uuid(&self) -> String {
            "1234".to_string()
        }
    }
    let generator: std::sync::Arc<dyn camel_core::api::spi::uuid::UuidGenerator> =
        std::sync::Arc::new(MyGen);
    use camel_core::api::spi::registry::Registry;
    t.context
        .simple_registry()
        .bind("mygen", Value::object(generator))
        .unwrap();
    assert_expression(&e, "${uuid(mygen)}", "1234");
}

#[test]
fn test_hash() {
    use camel_core::languages::simple::misc_functions::digest;
    use camel_core::util::string_helper::bytes_to_hex;
    let (_t, e) = exchange();
    assert_expression(
        &e,
        "${hash(hello)}",
        bytes_to_hex(&digest("SHA-256", b"hello").unwrap()),
    );
    let body = e.message().body().to_string();
    assert_expression(
        &e,
        "${hash(${body})}",
        bytes_to_hex(&digest("SHA-256", body.as_bytes()).unwrap()),
    );
    assert!(string(&e, "${hash(${header.foo})}").is_some());
    assert!(string(&e, "${hash(hello,SHA3-256)}").is_some());
    assert_expression(
        &e,
        "${hash(${body},SHA3-256)}",
        bytes_to_hex(&digest("SHA3-256", body.as_bytes()).unwrap()),
    );
    assert!(string(&e, "${hash(${header.foo},SHA3-256)}").is_some());
    assert_eq!(None, string(&e, "${hash(${header.unknown})}"));
}

#[test]
fn test_new_empty() {
    let (_t, e) = exchange();
    for ty in ["list", "LIST", "List", "set", "SET", "Set"] {
        assert_eq!(
            Value::List(vec![]),
            evaluate(&e, &format!("${{newEmpty({ty})}}"))
        );
    }
    for ty in ["map", "MAP", "Map"] {
        assert_eq!(
            Value::Map(ValueMap::new()),
            evaluate(&e, &format!("${{newEmpty({ty})}}"))
        );
    }
    for ty in ["string", "STRING", "String"] {
        assert_eq!(
            Value::from(""),
            evaluate(&e, &format!("${{newEmpty({ty})}}"))
        );
    }
    let simple = e.context().resolve_language("simple").unwrap();
    for bad in [
        "${newEmpty(falseSyntax}",
        "${newEmpty()}",
        "${newEmpty(}",
        "${newEmpty}",
    ] {
        let err = simple.create_expression(bad).err().expect(bad);
        assert!(
            err.is_subtype("SimpleIllegalSyntaxException"),
            "{bad}: {err:?}"
        );
    }
    let err = simple
        .create_expression("${newEmpty(unknownType)}")
        .unwrap()
        .evaluate(&e)
        .unwrap_err();
    assert!(err.is(ErrorKind::IllegalArgument));
}

// testIif: the ${file:name} cases come with the file functions
#[test]
fn test_iif() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("foo", 44);
    assert_expression(
        &e,
        "${iif(${header.foo} > 0,positive,negative)}",
        "positive",
    );
    e.message_mut().set_header("foo", -123);
    assert_expression(
        &e,
        "${iif(${header.foo} > 0,positive,negative)}",
        "negative",
    );
    e.message_mut().set_body("Hello World");
    e.message_mut().set_header("foo", 44);
    assert_expression(
        &e,
        "${iif(${header.foo} > 0,${body},Bye World)}",
        "Hello World",
    );
    e.message_mut().set_header("foo", -123);
    assert_expression(
        &e,
        "${iif(${header.foo} > 0,${body},Bye World)}",
        "Bye World",
    );
    assert_expression(&e, "${iif(${header.foo} > 0,${body},${null})}", Value::Null);
}

fn text(e: &Exchange, exp: &str) -> String {
    evaluate(e, exp).to_string()
}

// testDistinct: the ${join(..)} case comes with the join function
#[test]
fn test_distinct() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("1,2,3,3,4,3,5");
    assert_eq!(Some(5), evaluate(&e, "${distinct()}").size());
    assert_eq!("[1, 2, 3, 4, 5]", text(&e, "${distinct()}"));
    assert_eq!(
        "[Z, X, A, B, C, D, E]",
        text(&e, "${distinct('Z','X','Z','A','B','A','C','D','B','E')}")
    );
    assert_eq!(
        "[Z, 4, 1, 2, 3, 5, A, B, C, D, E]",
        text(
            &e,
            "${distinct('Z','4',${body},'A','B','A','C','D','B','E')}"
        )
    );
    e.message_mut().set_body(Value::Null);
    assert_eq!("[]", text(&e, "${distinct()}"));
}

#[test]
fn test_reverse() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("1,2,3,4,5");
    assert_eq!("[5, 4, 3, 2, 1]", text(&e, "${reverse()}"));
    assert_eq!(
        "[E, B, D, C, A, B, A, Z, X, Z]",
        text(&e, "${reverse('Z','X','Z','A','B','A','C','D','B','E')}")
    );
    assert_eq!(
        "[E, B, D, C, A, B, A, 5, 4, 3, 2, 1, 4, Z]",
        text(
            &e,
            "${reverse('Z','4',${body},'A','B','A','C','D','B','E')}"
        )
    );
    e.message_mut().set_body(Value::Null);
    assert_eq!("[]", text(&e, "${reverse()}"));
}

#[test]
fn test_shuffle() {
    let (_t, mut e) = exchange();
    let input = "1,2,3,4,5,6,7,8,9,0";
    e.message_mut().set_body(input);
    assert_eq!(Some(10), evaluate(&e, "${shuffle()}").size());
    let (s, s2) = (text(&e, "${shuffle()}"), text(&e, "${shuffle()}"));
    assert_ne!(input, s);
    assert_ne!(s, s2, "should be random when calling again");
    e.message_mut().set_body(Value::Null);
    assert_eq!("[]", text(&e, "${shuffle()}"));
}

#[test]
fn test_split() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("A,B,C,D,E");
    let arr = text_list(&["A", "B", "C", "D", "E"]);
    assert_eq!(arr, evaluate(&e, "${split()}"));
    assert_eq!(arr, evaluate(&e, "${split(',')}"));
    assert_eq!(text_list(&["A,B,C,D,E"]), evaluate(&e, "${split(';')}"));
    e.message_mut().set_header("myHead", "1;2;3;4");
    assert_eq!(
        text_list(&["1", "2", "3", "4"]),
        evaluate(&e, "${split(${header.myHead},;)}")
    );
    e.message_mut().set_body("A1,B1,C1\nA2,B2,C2\nA3,B3,C3");
    assert_eq!(
        text_list(&["A1,B1,C1", "A2,B2,C2", "A3,B3,C3"]),
        evaluate(&e, "${split(\\n)}")
    );
}

#[test]
fn test_sort() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(text_list(&["Z", "A", "H", "D"]));
    assert_eq!(text_list(&["A", "D", "H", "Z"]), evaluate(&e, "${sort()}"));
    assert_eq!(
        text_list(&["Z", "H", "D", "A"]),
        evaluate(&e, "${sort(true)}")
    );
    assert_eq!(
        text_list(&["A", "D", "H", "Z"]),
        evaluate(&e, "${sort(${body})}")
    );
    e.message_mut().set_body("5,2,1,3,4");
    assert_eq!(
        text_list(&["1", "2", "3", "4", "5"]),
        evaluate(&e, "${sort()}")
    );
}

#[test]
fn test_for_each() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Camel,World,Cheese");
    assert_eq!(
        text_list(&["Hello Camel", "Hello World", "Hello Cheese"]),
        evaluate(&e, "${forEach(${body},'Hello ${body}')}")
    );
    assert_eq!(
        text_list(&["Bye Camel", "Bye World", "Bye Cheese"]),
        evaluate(&e, "${forEach(${body},'Bye ${body}')}")
    );
    e.message_mut().set_body("1,2,3");
    assert_eq!(
        list(&[8, 9, 10]),
        evaluate(&e, "${forEach(${body},${sum(${body},7)})}")
    );
    e.message_mut().set_body(Value::Null);
    assert_eq!(
        Value::List(vec![]),
        evaluate(&e, "${forEach(${body},'Hello ${body}')}")
    );
}

#[test]
fn test_filter() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Camel,Dog,Cheese");
    assert_eq!(
        text_list(&["Camel", "Cheese"]),
        evaluate(&e, "${filter(${body},${length()} > 4)}")
    );
    assert_eq!(
        text_list(&["Dog"]),
        evaluate(&e, "${filter(${body},${contains(dog)})}")
    );
}

#[test]
fn test_list() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("4");
    assert_expression(&e, "${list(1,2,3)}", "[1, 2, 3]");
    assert_expression(&e, "${list(1,2,3,${body})}", "[1, 2, 3, 4]");
    assert_expression(&e, "${list('a','b','c')}", "[a, b, c]");
    assert_expression(&e, "${list()}", "[]");
}

#[test]
fn test_map() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("d");
    assert_expression(&e, "${map(1,a,2,b,3,c)}", "{1=a, 2=b, 3=c}");
    assert_expression(&e, "${map(1,a,2,b,3,c,4,${body})}", "{1=a, 2=b, 3=c, 4=d}");
    assert_expression(&e, "${map()}", "{}");
}

#[test]
fn test_range() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("5");
    assert_expression(&e, "${range(1,4)}", "[1, 2, 3]");
    assert_expression(&e, "${range(1,${body})}", "[1, 2, 3, 4]");
    assert_expression(&e, "${range(0,10)}", "[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]");
    assert_expression(&e, "${range(1,2)}", "[1]");
    for exp in ["${range(1,1)}", "${range(0,0)}", "${range(4,1)}"] {
        assert_expression(&e, exp, Value::Null);
    }
}

#[test]
fn test_collection_functions_on_a_single_comma() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(",");
    assert_expression(&e, "${isEmpty()}", true);
}

fn evaluate_err(e: &Exchange, text: &str) -> String {
    let simple = e.context().resolve_language("simple").unwrap();
    match simple.create_expression(text) {
        Ok(x) => x.evaluate(e).unwrap_err().to_string(),
        Err(err) => err.to_string(),
    }
}

#[test]
fn test_null_number_arguments() {
    let (_t, e) = exchange();
    let m = evaluate_err(&e, "${collate(${header.none})}");
    assert!(
        m.contains("collate number expression evaluated to null"),
        "{m}"
    );
    let m = evaluate_err(&e, "${range(1,${header.none})}");
    assert!(m.contains("range expression evaluated to null"), "{m}");
    let m = evaluate_err(&e, "${random(5,5)}");
    assert!(m.contains("requires max to be greater than min"), "{m}");
}

// the join case of testDistinct
#[test]
fn test_distinct_join() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("1,2,3,3,4,3,5");
    assert_expression(&e, "${join(',','',${distinct()})}", "1,2,3,4,5");
}

// Rust tests of the join, collate, skip, random, sys and env functions (no SimpleTest cases)
#[test]
fn test_join_collate_skip_random_sys_env() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(text_list(&["A", "B", "C"]));
    assert_expression(&e, "${join()}", "A,B,C");
    assert_expression(&e, "${join(;)}", "A;B;C");
    assert_expression(&e, "${join(&,id=)}", "id=A&id=B&id=C");
    assert_eq!("[[A, B], [C]]", text(&e, "${collate(2)}"));
    assert_eq!("[C]", text(&e, "${skip(2)}"));
    for _ in 0..20 {
        let v = <i64 as camel_core::api::convert::FromValue>::from_value(&evaluate(
            &e,
            "${random(1,3)}",
        ))
        .unwrap()
        .unwrap();
        assert!((1..3).contains(&v), "{v}");
    }
    camel_core::util::system_properties::set_property("simple.test.prop", "x1");
    assert_expression(&e, "${sys.simple.test.prop}", "x1");
    assert_expression(&e, "${sys.simple.test.missing}", Value::Null);
    let path = std::env::var("PATH").unwrap_or_default();
    assert_expression(&e, "${env.PATH}", path.clone());
    assert_expression(&e, "${sysenv:PATH}", path);
}

// the functions changing the exchange: evaluated with Expression::evaluate_mut

fn evaluate_mut(e: &mut Exchange, text: &str) -> Value {
    let simple = e.context().resolve_language("simple").unwrap();
    simple
        .create_expression(text)
        .unwrap()
        .evaluate_mut(e)
        .unwrap()
}

fn texts(v: &[&str]) -> Value {
    Value::List(v.iter().map(|s| Value::from(*s)).collect())
}

fn text_map(v: &[(&str, &str)]) -> Value {
    let mut m = ValueMap::new();
    for (k, x) in v {
        m.insert(k.to_string(), Value::from(*x));
    }
    Value::Map(m)
}

#[test]
fn test_set_header() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    assert_eq!(
        Value::Null,
        evaluate_mut(&mut e, "${setHeader(foo,${body})}")
    );
    assert_eq!(Some(&Value::from("Hello World")), e.message().header("foo"));

    e.message_mut().set_body("123");
    assert_eq!(
        Value::Null,
        evaluate_mut(&mut e, "${setHeader(bar,int,${body})}")
    );
    assert_eq!(Some(&Value::Int(123)), e.message().header("bar"));

    // null should remove the header
    assert_eq!(
        Value::Null,
        evaluate_mut(&mut e, "${setHeader(bar,${null})}")
    );
    assert_eq!(None, e.message().header("bar"));
}

#[test]
fn test_set_variable() {
    let (_t, mut e) = exchange();
    e.remove_variable("cheese");
    e.message_mut().set_body("Hello World");
    assert_eq!(
        Value::Null,
        evaluate_mut(&mut e, "${setVariable(foo,${body})}")
    );
    assert_eq!(Some(&Value::from("Hello World")), e.variable("foo"));

    e.message_mut().set_body("123");
    assert_eq!(
        Value::Null,
        evaluate_mut(&mut e, "${setVariable(bar,int,${body})}")
    );
    assert_eq!(Some(&Value::Int(123)), e.variable("bar"));

    // null should remove the variable
    assert_eq!(
        Value::Null,
        evaluate_mut(&mut e, "${setVariable(bar,${null})}")
    );
    assert_eq!(None, e.variable("bar"));
    assert_eq!(Some(&Value::from("Hello World")), e.variable("foo"));
}

#[test]
fn test_set_variable_mapping() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    let map = "${setVariable(count,${body.length})}\nInput: ${body}\nBytes: ${variable.count}\n";
    let expected = "\nInput: Hello World\nBytes: 11\n";
    assert_eq!(Value::from(expected), evaluate_mut(&mut e, map));
}

#[test]
fn test_set_header_needs_evaluate_mut() {
    let (_t, e) = exchange();
    let simple = e.context().resolve_language("simple").unwrap();
    let err = simple
        .create_expression("${setHeader(foo,bar)}")
        .unwrap()
        .evaluate(&e)
        .unwrap_err();
    assert!(err.message().unwrap().contains("evaluate_mut"), "{err}");
}

#[test]
fn test_list_add() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(texts(&["A", "B"]));
    assert_eq!(
        texts(&["A", "B", "C"]),
        evaluate_mut(&mut e, "${listAdd('C')}")
    );

    e.message_mut().set_header("myChar", "D");
    assert_eq!(
        texts(&["A", "B", "C", "D"]),
        evaluate_mut(&mut e, "${listAdd(${header.myChar})}")
    );
    assert_eq!(&texts(&["A", "B", "C", "D"]), e.message().body());

    // two-parameter form: add to a header list instead of body
    e.message_mut().set_header("myList", texts(&["X"]));
    assert_eq!(
        texts(&["X", "Y"]),
        evaluate_mut(&mut e, "${listAdd(${header.myList},'Y')}")
    );
    assert_eq!(Some(&texts(&["X", "Y"])), e.message().header("myList"));
}

#[test]
fn test_list_remove() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(texts(&["A", "B", "C"]));
    assert_eq!(
        texts(&["A", "B"]),
        evaluate_mut(&mut e, "${listRemove('C')}")
    );
    assert_eq!(texts(&["B"]), evaluate_mut(&mut e, "${listRemove(0)}"));

    // Java adds to the body list it holds; here the body is changed in the exchange
    e.message_mut().set_body(texts(&["B", "C", "D", "E"]));
    assert_eq!(
        texts(&["B", "C", "D"]),
        evaluate_mut(&mut e, "${listRemove(last)}")
    );
}

#[test]
fn test_map_add() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(text_map(&[("A", "AAA"), ("B", "BBB")]));
    assert_eq!(
        text_map(&[("A", "AAA"), ("B", "BBB"), ("C", "CCC")]),
        evaluate_mut(&mut e, "${mapAdd('C','CCC')}")
    );

    e.message_mut().set_header("myChar", "DDD");
    assert_eq!(
        text_map(&[("A", "AAA"), ("B", "BBB"), ("C", "CCC"), ("D", "DDD")]),
        evaluate_mut(&mut e, "${mapAdd('D',${header.myChar})}")
    );

    // two-parameter form: add to a header map instead of body
    e.message_mut()
        .set_header("myMap", text_map(&[("X", "XXX")]));
    assert_eq!(
        text_map(&[("X", "XXX"), ("Y", "YYY")]),
        evaluate_mut(&mut e, "${mapAdd(${header.myMap},'Y','YYY')}")
    );
    assert_eq!(
        Some(&text_map(&[("X", "XXX"), ("Y", "YYY")])),
        e.message().header("myMap")
    );
}

#[test]
fn test_map_remove() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(text_map(&[("A", "AAA"), ("B", "BBB"), ("C", "CCC")]));
    assert_eq!(
        text_map(&[("A", "AAA"), ("B", "BBB")]),
        evaluate_mut(&mut e, "${mapRemove('C')}")
    );

    // two-parameter form: remove from a header map instead of body
    e.message_mut()
        .set_header("myMap", text_map(&[("X", "XXX"), ("Z", "ZZZ")]));
    assert_eq!(
        text_map(&[("Z", "ZZZ")]),
        evaluate_mut(&mut e, "${mapRemove(${header.myMap},'X')}")
    );
}

// the basic functions and the OGNL navigation (Java BeanExpression on the values)

fn predicate(e: &Exchange, text: &str) -> bool {
    let simple = e.context().resolve_language("simple").unwrap();
    simple.create_predicate(text).unwrap().matches(e).unwrap()
}

fn error(e: &Exchange, text: &str) -> CamelError {
    let simple = e.context().resolve_language("simple").unwrap();
    match simple.create_expression(text) {
        Ok(x) => x.evaluate(e).unwrap_err(),
        Err(err) => err,
    }
}

fn cause(e: &CamelError) -> &CamelError {
    e.cause()
        .and_then(|c| c.downcast_ref::<CamelError>())
        .unwrap_or_else(|| panic!("no cause: {e:?}"))
}

/// Java `OrderLine`.
struct OrderLine {
    id: i64,
    name: &'static str,
}

impl camel_core::util::value::Bean for OrderLine {
    fn property(&self, name: &str) -> Option<Value> {
        match name {
            "id" => Some(Value::Int(self.id)),
            "name" => Some(Value::from(self.name)),
            _ => None,
        }
    }
}

fn order_line(id: i64, name: &'static str) -> Value {
    Value::bean(OrderLine { id, name })
}

#[test]
fn test_simple_expression_or_predicate() {
    let (_t, e) = exchange();
    for _ in 0..2 {
        assert!(predicate(&e, "${header.bar} == 123"));
        assert!(!predicate(&e, "${header.bar} == 124"));
        assert_expression(&e, "${body}", "<hello id='m123'>world!</hello>");
    }
}

#[test]
fn test_result_type() {
    let (_t, e) = exchange();
    assert_eq!(
        Some(123),
        <i64 as camel_core::api::convert::FromValue>::from_value(&evaluate(&e, "${header.bar}"))
            .unwrap()
    );
    assert_eq!(Some("123".to_string()), string(&e, "${header.bar}"));
    assert_eq!(None, string(&e, "${header.unknown}"));
}

#[test]
fn test_constant_null_and_file_dir_expressions() {
    let (_t, e) = exchange();
    assert_expression(&e, "Hello World", "Hello World");
    assert_eq!(Value::Null, evaluate(&e, "${null}"));
    assert_expression(&e, "file:mydir", "file:mydir");
}

#[test]
fn test_empty_expression() {
    let (_t, e) = exchange();
    assert_expression(&e, "", "");
    assert_expression(&e, " ", " ");
    assert!(!predicate(&e, ""));
    assert!(!predicate(&e, " "));
}

#[test]
fn test_exchange_expression() {
    let (_t, e) = exchange();
    let v = evaluate(&e, "${exchange}");
    let x = v.downcast_ref::<Exchange>().expect("the exchange");
    assert_eq!(e.exchange_id(), x.exchange_id());
    assert_eq!(format!("Exchange[{}]", e.exchange_id()), v.to_string());
}

#[test]
fn test_log_exchange_expression() {
    use camel_core::api::spi::exchange_formatter::ExchangeFormatter;
    let (_t, e) = exchange();
    let expected =
        camel_core::support::exchange_formatter::DefaultExchangeFormatter::default().format(&e);
    assert_expression(&e, "${logExchange}", expected.as_str());
}

#[test]
fn test_exchange_ognl_expression() {
    let (_t, e) = exchange();
    assert_expression(&e, "${exchange.exchangeId}", e.exchange_id());
    assert_expression(
        &e,
        "${exchange.class.name}",
        "org.apache.camel.support.DefaultExchange",
    );
}

#[test]
fn test_body_ognl_expression() {
    let (_t, e) = exchange();
    let simple = e.context().resolve_language("simple").unwrap();
    assert!(simple.create_expression("${body}").is_ok());
    assert!(simple.create_expression("${body.xxx}").is_ok());
    // must start with a dot
    let err = simple.create_expression("${bodyxxx}").err().unwrap();
    assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err}");
    assert!(simple.create_expression("$simple{body}").is_ok());
}

#[test]
fn test_body_expression_not_string_type() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(123);
    assert_eq!(Value::Int(123), evaluate(&e, "${body}"));
}

/// Java `MyClass` with `getMyArray()`.
struct MyClass;

impl camel_core::util::value::Bean for MyClass {
    fn property(&self, name: &str) -> Option<Value> {
        (name == "myArray")
            .then(|| Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]))
    }
}

#[test]
fn test_body_expression_with_array() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::bean(MyClass));
    assert!(matches!(evaluate(&e, "${body.myArray}"), Value::List(l) if l.len() == 3));
    assert_eq!(Value::Int(3), evaluate(&e, "${body.myArray.length}"));
}

#[test]
fn test_simple_expressions() {
    let (_t, mut e) = exchange();
    let id = e.exchange_id().to_string();
    assert_expression(&e, "${exchangeId}", id.as_str());
    assert_expression(&e, "${id}", id.as_str());
    for exp in ["${body}", "${in.body}"] {
        assert_expression(&e, exp, "<hello id='m123'>world!</hello>");
    }
    for exp in [
        "${in.header.foo}",
        "${in.headers.foo}",
        "${header.foo}",
        "${headers.foo}",
    ] {
        assert_expression(&e, exp, "abc");
    }
    assert_eq!(Value::Null, evaluate(&e, "${routeId}"));
    assert_eq!(Value::Null, evaluate(&e, "${fromRouteId}"));
    e.set_from_route_id("myRouteId");
    assert_expression(&e, "${routeId}", "myRouteId");
}

#[test]
fn test_simple_thread_id_and_name() {
    let (_t, e) = exchange();
    let id = format!("{:?}", std::thread::current().id());
    assert_expression(&e, "${threadId}", id.as_str());
    assert_expression(
        &e,
        "The id is ${threadId}",
        format!("The id is {id}").as_str(),
    );
    let name = std::thread::current().name().unwrap_or("main").to_string();
    assert_expression(&e, "${threadName}", name.as_str());
    assert_expression(
        &e,
        "The name is ${threadName}",
        format!("The name is {name}").as_str(),
    );
}

#[test]
fn test_simple_step_id_property_and_camel_id() {
    let (_t, mut e) = exchange();
    assert_eq!(Value::Null, evaluate(&e, "${stepId}"));
    e.set_property("CamelStepId", "foo");
    assert_expression(&e, "${stepId}", "foo");
    e.set_property("medal", "gold");
    assert_expression(&e, "${exchangeProperty.medal}", "gold");
    assert_expression(&e, "${exchangeProperty:medal}", "gold");
    let name = e.context().name().to_string();
    assert_expression(&e, "${camelId}", name.as_str());
}

fn cool_map() -> ValueMap {
    let mut map = ValueMap::new();
    map.insert("cool".into(), Value::from("Camel rocks"));
    map.insert("dude".into(), Value::from("Hey dude"));
    map.insert("code".into(), Value::Int(4321));
    map
}

#[test]
fn test_ognl_body_list_and_map() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(Value::List(vec![Value::Map(cool_map())]));
    assert_expression(&e, "${in.body[0][cool]}", "Camel rocks");
    assert_expression(&e, "${body[0][cool]}", "Camel rocks");
    assert_expression(&e, "${in.body[0][code]}", 4321);
    assert_expression(&e, "${body[0][code]}", 4321);
}

#[test]
fn test_ognl_body_empty_list() {
    let (_t, mut e) = exchange();
    let mut map = ValueMap::new();
    map.insert("list".into(), Value::List(vec![]));
    e.message_mut().set_body(Value::Map(map));
    assert_eq!(
        Value::Null,
        evaluate(&e, "${in.body?.get('list')[0].toString}")
    );
}

#[test]
fn test_ognl_body_expression() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("hello world");
    assert!(predicate(&e, "${body} == 'hello world'"));
    assert!(predicate(&e, "${body.toUpperCase()} == 'HELLO WORLD'"));
}

#[test]
fn test_ognl_body_as_expression() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(b"hello world".to_vec());
    // there is no upper case method on byte array, but we can convert to String
    let err = error(&e, "${body.toUpperCase()}");
    assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
    let c = cause(&err);
    assert!(c.is_subtype("MethodNotFoundException"), "{c}");
    assert!(c
        .message()
        .unwrap()
        .starts_with("Method with name: toUpperCase() not found"));
    for f in ["bodyAs", "mandatoryBodyAs"] {
        assert!(predicate(&e, &format!("${{{f}(String)}} == 'hello world'")));
        assert!(predicate(
            &e,
            &format!("${{{f}(String).toUpperCase()}} == 'HELLO WORLD'")
        ));
    }
    // and the body of the exchange is not changed
    assert_eq!(&Value::Bytes(b"hello world".to_vec()), e.message().body());
}

#[test]
fn test_ognl_call_replace() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("cool", "Camel rocks");
    e.message_mut().set_header("dude", "Hey dude");
    assert_expression(
        &e,
        "${headers.cool.replaceAll(\"rocks\", \"is so cool\")}",
        "Camel is so cool",
    );
}

#[test]
fn test_ognl_body_list_and_map_and_method() {
    let (_t, mut e) = exchange();
    let mut map = ValueMap::new();
    map.insert("camel".into(), order_line(123, "Camel in Action"));
    map.insert("amq".into(), order_line(456, "ActiveMQ in Action"));
    e.message_mut().set_body(Value::List(vec![Value::Map(map)]));
    for body in ["in.body", "body"] {
        assert_expression(&e, &format!("${{{body}[0][camel].id}}"), 123);
        assert_expression(
            &e,
            &format!("${{{body}[0][camel].name}}"),
            "Camel in Action",
        );
        assert_expression(&e, &format!("${{{body}[0][camel].getId}}"), 123);
        assert_expression(
            &e,
            &format!("${{{body}[0][camel].getName}}"),
            "Camel in Action",
        );
    }
}

fn assert_index_out_of_bounds(e: &Exchange, text: &str) {
    let err = error(e, text);
    let c = cause(&err);
    assert!(c.is_subtype("IndexOutOfBoundsException"), "{c}");
    assert!(
        c.message()
            .unwrap()
            .starts_with("Index: 2, Size: 2 out of bounds with List from bean"),
        "{c}"
    );
}

#[test]
fn test_ognl_property_list() {
    let (_t, mut e) = exchange();
    e.set_property(
        "wicket",
        Value::List(vec![
            Value::from("Camel in Action"),
            Value::from("ActiveMQ in Action"),
        ]),
    );
    assert_expression(&e, "${exchangeProperty.wicket[0]}", "Camel in Action");
    assert_expression(&e, "${exchangeProperty.wicket[1]}", "ActiveMQ in Action");
    assert_index_out_of_bounds(&e, "${exchangeProperty.wicket[2]}");
    assert_eq!(
        Value::Null,
        evaluate(&e, "${exchangeProperty.unknown[cool]}")
    );
}

#[test]
fn test_ognl_property_lines_list() {
    let (_t, mut e) = exchange();
    e.set_property(
        "wicket",
        Value::List(vec![
            order_line(123, "Camel in Action"),
            order_line(456, "ActiveMQ in Action"),
        ]),
    );
    assert_expression(&e, "${exchangeProperty.wicket[0].getId}", 123);
    assert_expression(
        &e,
        "${exchangeProperty.wicket[1].getName}",
        "ActiveMQ in Action",
    );
    assert_index_out_of_bounds(&e, "${exchangeProperty.wicket[2]}");
    assert_eq!(
        Value::Null,
        evaluate(&e, "${exchangeProperty.unknown[cool]}")
    );
}

#[test]
fn test_ognl_property_map() {
    let (_t, mut e) = exchange();
    e.set_property("wicket", Value::Map(cool_map()));
    assert_expression(&e, "${exchangeProperty.wicket[cool]}", "Camel rocks");
    assert_expression(&e, "${exchangeProperty.wicket[dude]}", "Hey dude");
    assert_eq!(
        Value::Null,
        evaluate(&e, "${exchangeProperty.wicket[unknown]}")
    );
    assert_expression(&e, "${exchangeProperty.wicket[code]}", 4321);
    // no property named unknown
    assert_eq!(
        Value::Null,
        evaluate(&e, "${exchangeProperty?.unknown[cool]}")
    );
    assert_eq!(
        Value::Null,
        evaluate(&e, "${exchangeProperty.unknown[cool]}")
    );
}

#[test]
fn test_ognl_property_map_with_dot() {
    let (_t, mut e) = exchange();
    let mut map = ValueMap::new();
    map.insert("this.code".into(), Value::from("This code"));
    e.set_property("wicket", Value::Map(map));
    assert_expression(&e, "${exchangeProperty.wicket[this.code]}", "This code");
}

#[test]
fn test_ognl_property_map_not_map() {
    let (_t, e) = exchange();
    let err = error(&e, "${exchangeProperty.foobar[bar]}");
    assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
    let c = cause(&err);
    assert!(c.is_subtype("IndexOutOfBoundsException"), "{c}");
    assert_eq!(
        Some("Key: bar not found in bean: cba of type: java.lang.String using OGNL path [[bar]]"),
        c.message()
    );
}

#[test]
fn test_ognl_property_map_illegal_syntax() {
    let (_t, e) = exchange();
    let err = error(&e, "${exchangeProperty.foobar[bar}");
    assert!(err.is(ErrorKind::ExpressionIllegalSyntax), "{err}");
    assert!(err.message().unwrap().starts_with(
        "Valid syntax: ${exchangeProperty.OGNL} was: exchangeProperty.foobar[bar at location 0"
    ));
}

#[test]
fn test_ognl_header_empty() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("beer", "");
    assert!(predicate(&e, "${header.beer} == ''"));
    assert!(predicate(&e, "${header.beer} == \"\""));
    assert!(!predicate(&e, "${header.beer} == ' '"));
    assert!(!predicate(&e, "${header.beer} == \" \""));
    e.message_mut().set_header("beer", " ");
    assert!(!predicate(&e, "${header.beer} == ''"));
    assert!(!predicate(&e, "${header.beer} == \"\""));
    assert!(predicate(&e, "${header.beer} == ' '"));
    assert!(predicate(&e, "${header.beer} == \" \""));
    assert!(predicate(&e, "${header.beer.toString().trim()} == ''"));
    assert!(predicate(&e, "${header.beer.toString().trim()} == \"\""));
    e.message_mut().set_header("beer", "   ");
    assert!(predicate(&e, "${header.beer.trim()} == ''"));
    assert!(predicate(&e, "${header.beer.trim()} == \"\""));
}

#[test]
fn test_complex_expressions() {
    let (_t, e) = exchange();
    for start in ["${", "$simple{"] {
        let x = |s: &str| s.replace("${", start);
        assert_expression(&e, &x("hey ${in.header.foo}"), "hey abc");
        assert_expression(&e, &x("hey ${in.header:foo}"), "hey abc");
        assert_expression(&e, &x("hey ${in.header.foo}!"), "hey abc!");
        assert_expression(&e, &x("hey ${in.header:foo}!"), "hey abc!");
        assert_expression(
            &e,
            &x("hey ${in.header.foo}-${in.header.foo}!"),
            "hey abc-abc!",
        );
        assert_expression(
            &e,
            &x("hey ${in.header:foo}-${in.header.foo}!"),
            "hey abc-abc!",
        );
        assert_expression(&e, &x("hey ${in.header.foo}${in.header.foo}"), "hey abcabc");
        assert_expression(&e, &x("hey ${in.header:foo}${in.header.foo}"), "hey abcabc");
        assert_expression(&e, &x("${in.header.foo}${in.header.foo}"), "abcabc");
        assert_expression(&e, &x("${in.header.foo}"), "abc");
        assert_expression(&e, &x("${in.header:foo}"), "abc");
        assert_expression(&e, &x("${in.header.foo}!"), "abc!");
        assert_expression(&e, &x("${in.header:foo}!"), "abc!");
    }
    assert_expression(&e, "${in.header:foo}${in.header:foo}", "abcabc");
}

#[test]
fn test_invalid_complex_expression() {
    let (_t, e) = exchange();
    let err = error(&e, "hey ${foo");
    assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err}");
    assert!(err.message().unwrap().contains(" at location 8"), "{err}");
}

#[test]
fn test_predicates() {
    let (_t, e) = exchange();
    assert!(predicate(&e, "${body}"));
    assert!(predicate(&e, "${header.foo}"));
    assert!(!predicate(&e, "${header.madeUpHeader}"));
}

#[test]
fn test_exception_message_and_stacktrace() {
    let (_t, mut e) = exchange();
    // CAMEL-24651: no exception, no stack trace
    assert_eq!(Value::Null, evaluate(&e, "${exception.stacktrace}"));
    e.set_exception(Some(CamelError::illegal_argument("Just testing")));
    assert_expression(&e, "${exception.message}", "Just testing");
    assert_expression(
        &e,
        "Hello ${exception.message} World",
        "Hello Just testing World",
    );
    // the Rust errors have no stack frames: the stack trace is the error and its causes
    let out = string(&e, "${exception.stacktrace}").unwrap();
    assert!(out.starts_with("java.lang.IllegalArgumentException: Just testing"));
}

#[test]
fn test_exception() {
    let (_t, mut e) = exchange();
    e.set_exception(Some(CamelError::illegal_argument("Just testing")));
    let v = evaluate(&e, "${exception}");
    let x = v.downcast_ref::<CamelError>().expect("the exception");
    assert!(x.is(ErrorKind::IllegalArgument));
    assert_eq!(Some("Just testing"), x.message());
    assert_expression(
        &e,
        "${exception}",
        "java.lang.IllegalArgumentException: Just testing",
    );
    assert_expression(
        &e,
        "${exception.class.simpleName}",
        "IllegalArgumentException",
    );
}

#[test]
fn test_body_as() {
    let (_t, mut e) = exchange();
    for f in ["bodyAs", "mandatoryBodyAs"] {
        e.message_mut().set_body("<hello id='m123'>world!</hello>");
        assert_expression(
            &e,
            &format!("${{{f}(String)}}"),
            "<hello id='m123'>world!</hello>",
        );
        assert_expression(
            &e,
            &format!("${{{f}('String')}}"),
            "<hello id='m123'>world!</hello>",
        );
        e.message_mut().set_body(456);
        for t in ["Integer", "int", "'int'"] {
            assert_expression(&e, &format!("${{{f}({t})}}"), 456);
        }
        let err = error(&e, &format!("${{{f}(XXX)}}"));
        assert!(err.to_string().contains("XXX"), "{err}");
    }
    e.message_mut().set_body(Value::Null);
    assert_eq!(Value::Null, evaluate(&e, "${bodyAs('String')}"));
    let err = error(&e, "${mandatoryBodyAs('String')}");
    assert!(err.is(ErrorKind::InvalidPayload), "{err}");
}

#[test]
fn test_header_empty_body() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::Null);
    for exp in [
        "${header.foo}",
        "${header:foo}",
        "${headers.foo}",
        "${headers:foo}",
        "${in.header.foo}",
        "${in.header:foo}",
        "${in.headers.foo}",
        "${in.headers:foo}",
        "${headers[foo]}",
        "${in.headers[foo]}",
    ] {
        assert_expression(&e, exp, "abc");
    }
}

#[test]
fn test_ognl_on_headers_and_properties_with_bracket() {
    let (_t, mut e) = exchange();
    for key in ["order", "purchase.order", "foo.bar.qux", "purchase order"] {
        e.message_mut()
            .set_header(key, order_line(123, "Camel in Action"));
        assert_expression(&e, &format!("${{headers[{key}].name}}"), "Camel in Action");
        assert_expression(
            &e,
            &format!("${{in.headers[{key}].name}}"),
            "Camel in Action",
        );
        assert_expression(
            &e,
            &format!("${{in.headers['{key}'].name}}"),
            "Camel in Action",
        );
        e.set_property(key, order_line(123, "Camel in Action"));
        assert_expression(
            &e,
            &format!("${{exchangeProperty[{key}].name}}"),
            "Camel in Action",
        );
        assert_expression(
            &e,
            &format!("${{exchangeProperty['{key}'].name}}"),
            "Camel in Action",
        );
    }
}

#[test]
fn test_is_instance_of_empty_body() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(Value::Null);
    let simple = e.context().resolve_language("simple").unwrap();
    let err = simple.create_predicate("${body} is null").err().unwrap();
    assert!(err.is_subtype("SimpleIllegalSyntaxException"), "{err}");
    assert!(err.message().unwrap().contains(" at location 11"), "{err}");
}

#[test]
fn test_headers_and_variables() {
    let (_t, mut e) = exchange();
    let headers: ValueMap = e
        .message()
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    assert_eq!(2, headers.len());
    assert_eq!(Value::Map(headers.clone()), evaluate(&e, "${headers}"));
    assert_eq!(Value::Map(headers.clone()), evaluate(&e, "${in.headers}"));
    assert_expression(&e, "${headers.size}", 2);
    for (k, v) in &headers {
        e.set_variable(k, v.clone());
    }
    e.message_mut().remove_headers("*");
    assert_eq!(3, e.variables().len());
    assert_eq!(
        Value::Map(e.variables().clone()),
        evaluate(&e, "${variables}")
    );
    assert_expression(&e, "${variables.size}", 3);
}

#[test]
fn test_header_and_variable_key_with_space_and_as() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("some key", "Some Value");
    // the variables are the headers (Java putAll) for the variable cases
    let mut v = e.clone();
    for (k, x) in e.message().headers().iter() {
        v.set_variable(k, x.clone());
    }
    for (x, scope, plural) in [(&e, "header", "headers"), (&v, "variable", "variables")] {
        assert_expression(x, &format!("${{{scope}As(foo,String)}}"), "abc");
        assert_expression(x, &format!("${{{scope}As(some key,String)}}"), "Some Value");
        assert_expression(
            x,
            &format!("${{{scope}As('some key',String)}}"),
            "Some Value",
        );
        for s in [scope, plural] {
            assert_expression(x, &format!("${{{s}[foo]}}"), "abc");
            assert_expression(x, &format!("${{{s}[some key]}}"), "Some Value");
            assert_expression(x, &format!("${{{s}['some key']}}"), "Some Value");
        }
        for args in [
            "bar,int",
            "bar, int",
            "'bar', int",
            "'bar','int'",
            "'bar','Integer'",
            "'bar',\"int\"",
        ] {
            assert_expression(x, &format!("${{{scope}As({args})}}"), 123);
        }
        assert_expression(x, &format!("${{{scope}As(bar,String)}}"), "123");
        assert_eq!(
            Value::Null,
            evaluate(x, &format!("${{{scope}As(unknown,String)}}"))
        );
        let err = error(x, &format!("${{{scope}As(unknown String)}}"));
        assert!(
            err.message().unwrap().starts_with(&format!(
                "Valid syntax: ${{{scope}As(key, type)}} was: {scope}As(unknown String)"
            )),
            "{err}"
        );
        let err = error(x, &format!("${{{scope}As(fool,String).test}}"));
        assert!(
            err.message().unwrap().starts_with(&format!(
                "Valid syntax: ${{{scope}As(key, type)}} was: {scope}As(fool,String).test"
            )),
            "{err}"
        );
        let err = error(x, &format!("${{{scope}As(bar,XXX)}}"));
        assert!(err.to_string().contains("XXX"), "{err}");
    }
    assert_expression(&v, "${variable[cheese]}", "gauda");
    assert_expression(&v, "${variables[cheese]}", "gauda");
}

#[test]
fn test_illegal_syntax() {
    let (_t, e) = exchange();
    for exp in ["hey ${xxx} how are you?", "${xxx}"] {
        assert!(error(&e, exp)
            .message()
            .unwrap()
            .starts_with("Unknown function: xxx"));
    }
    assert!(error(&e, "${bodyAs(xxx}")
        .message()
        .unwrap()
        .starts_with("Valid syntax: ${bodyAs(type)} was: bodyAs(xxx"));
}

#[test]
fn test_ognl_header_list_and_map() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header(
        "wicket",
        Value::List(vec![
            Value::from("Camel in Action"),
            Value::from("ActiveMQ in Action"),
        ]),
    );
    assert_expression(&e, "${header.wicket[0]}", "Camel in Action");
    assert_expression(&e, "${header.wicket[1]}", "ActiveMQ in Action");
    assert_index_out_of_bounds(&e, "${header.wicket[2]}");
    assert_eq!(Value::Null, evaluate(&e, "${header.unknown[cool]}"));

    e.message_mut().set_header(
        "wicket",
        Value::List(vec![
            order_line(123, "Camel in Action"),
            order_line(456, "ActiveMQ in Action"),
        ]),
    );
    assert_expression(&e, "${header.wicket[0].getId}", 123);
    assert_expression(&e, "${header.wicket[1].getName}", "ActiveMQ in Action");
    assert_index_out_of_bounds(&e, "${header.wicket[2]}");

    e.message_mut().set_header("wicket", Value::Map(cool_map()));
    assert_expression(&e, "${header.wicket[cool]}", "Camel rocks");
    assert_expression(&e, "${header.wicket[dude]}", "Hey dude");
    assert_eq!(Value::Null, evaluate(&e, "${header.wicket[unknown]}"));
    assert_expression(&e, "${header.wicket[code]}", 4321);
    assert_eq!(Value::Null, evaluate(&e, "${header?.unknown[cool]}"));
    assert_eq!(Value::Null, evaluate(&e, "${header.unknown[cool]}"));

    let mut map = ValueMap::new();
    map.insert("this.code".into(), Value::from("This code"));
    e.message_mut().set_header("wicket", Value::Map(map));
    assert_expression(&e, "${header.wicket[this.code]}", "This code");

    let err = error(&e, "${header.foo[bar]}");
    assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
    assert_eq!(
        Some("Key: bar not found in bean: abc of type: java.lang.String using OGNL path [[bar]]"),
        cause(&err).message()
    );
    assert!(error(&e, "${header.foo[bar}")
        .message()
        .unwrap()
        .starts_with("Valid syntax: ${header.name[key]} was: header.foo[bar"));
}

#[test]
fn test_body_ognl_as_map() {
    let (_t, mut e) = exchange();
    let mut map = ValueMap::new();
    map.insert("foo".into(), Value::from("Camel"));
    map.insert("bar".into(), Value::Int(6));
    map.insert("foo.bar".into(), Value::from("Camel"));
    e.message_mut().set_body(Value::Map(map));
    for b in ["in.body", "body"] {
        assert_expression(&e, &format!("${{{b}[foo]}}"), "Camel");
        assert_expression(&e, &format!("${{{b}[bar]}}"), 6);
        assert_expression(&e, &format!("${{{b}[foo.bar]}}"), "Camel");
    }
}

/// Java `Animal`.
struct Animal {
    name: &'static str,
    age: i64,
    friend: std::sync::Mutex<Option<Value>>,
}

impl Animal {
    fn new(name: &'static str, age: i64) -> std::sync::Arc<Animal> {
        std::sync::Arc::new(Animal {
            name,
            age,
            friend: std::sync::Mutex::new(None),
        })
    }

    fn set_friend(&self, friend: &std::sync::Arc<Animal>) {
        *self.friend.lock().unwrap() = Some(animal_value(friend));
    }
}

impl std::fmt::Display for Animal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl camel_core::util::value::Bean for Animal {
    fn property(&self, name: &str) -> Option<Value> {
        match name {
            "name" => Some(Value::from(self.name)),
            "age" => Some(Value::Int(self.age)),
            "friend" => Some(self.friend.lock().unwrap().clone().unwrap_or_default()),
            "dangerous" => Some(Value::Bool(self.name.contains("Tiger"))),
            _ => None,
        }
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        match (method, args) {
            ("setFriend", [friend]) => {
                *self.friend.lock().unwrap() = Some(friend.clone());
                Some(Ok(Value::Null))
            }
            _ => None,
        }
    }
}

fn animal_value(a: &std::sync::Arc<Animal>) -> Value {
    Value::Object(
        camel_core::util::value::ObjectRef::from_arc(a.clone())
            .with_bean::<Animal>()
            .with_display(|a: &Animal, f| std::fmt::Display::fmt(a, f)),
    )
}

#[test]
fn test_body_ognl_simple() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(animal_value(&Animal::new("Camel", 6)));
    assert_expression(&e, "${in.body.getName}", "Camel");
    assert_expression(&e, "${in.body.getAge}", 6);
    assert_expression(&e, "${in.body.name}", "Camel");
    assert_expression(&e, "${in.body.age}", 6);
}

#[test]
fn test_body_ognl_simple_operator_and_nested() {
    let (_t, mut e) = exchange();
    let tiger = Animal::new("Tony the Tiger", 13);
    let camel = Animal::new("Camel", 6);
    camel.set_friend(&tiger);
    e.message_mut().set_body(animal_value(&camel));
    for (name, age, friend, dangerous) in [
        ("getName", "getAge", "getFriend", "isDangerous"),
        ("name", "age", "friend", "dangerous"),
    ] {
        assert!(predicate(
            &e,
            &format!("${{in.body.{name}}} contains 'Camel'")
        ));
        assert!(!predicate(
            &e,
            &format!("${{in.body.{name}}} contains 'Tiger'")
        ));
        assert!(predicate(&e, &format!("${{in.body.{age}}} < 10")));
        assert!(!predicate(&e, &format!("${{in.body.{age}}} > 10")));
        assert!(predicate(&e, &format!("${{in.body.{age}}} <= '6'")));
        assert!(!predicate(&e, &format!("${{in.body.{age}}} > '6'")));
        assert!(predicate(
            &e,
            &format!("${{in.body.{age}}} < ${{body.{friend}.{age}}}")
        ));
        assert!(predicate(
            &e,
            &format!("${{in.body.{friend}.{dangerous}}} == true")
        ));
        assert_expression(
            &e,
            &format!("${{in.body.{friend}.{name}}}"),
            "Tony the Tiger",
        );
        assert_expression(&e, &format!("${{in.body.{friend}.{age}}}"), "13");
        // using null safe to avoid the NPE
        assert_eq!(
            Value::Null,
            evaluate(&e, &format!("${{in.body.{friend}?.{friend}.{name}}}"))
        );
        // without null safe we get an NPE
        let err = error(&e, &format!("${{in.body.{friend}.{friend}.{name}}}"));
        assert_eq!(
            Some(
                format!(
                    "Failed to invoke method: .{friend}.{friend}.{name} on {} due last method returned null and therefore cannot continue to invoke method .{name} on a null instance",
                    std::any::type_name::<Animal>()
                )
                .as_str()
            ),
            err.message()
        );
    }
    // just to mix it a bit
    assert_expression(&e, "${in.body.friend.getName}", "Tony the Tiger");
    assert_expression(&e, "${in.body.getFriend.age}", "13");
}

#[test]
fn test_body_ognl_reentrant_and_boolean() {
    let (_t, mut e) = exchange();
    let camel = Animal::new("Camel", 6);
    let tiger = Animal::new("Tony the Tiger", 13);
    let elephant = Animal::new("Big Ella", 48);
    camel.set_friend(&tiger);
    tiger.set_friend(&elephant);
    elephant.set_friend(&camel);
    e.message_mut().set_body(animal_value(&camel));
    for f in ["getFriend", "friend"] {
        let n = if f == "friend" { "name" } else { "getName" };
        assert_expression(&e, &format!("${{body.{f}.{f}.{f}.{n}}}"), "Camel");
        assert_expression(
            &e,
            &format!("${{body.{f}.{f}.{f}.{f}.{n}}}"),
            "Tony the Tiger",
        );
        assert_expression(
            &e,
            &format!("${{body.{f}.{f}.{f}.{f}.{f}.{n}}}"),
            "Big Ella",
        );
    }
    e.message_mut().set_body(animal_value(&tiger));
    assert_expression(&e, "${body.isDangerous}", "true");
    assert_expression(&e, "${body.dangerous}", "true");
    e.message_mut().set_body(animal_value(&camel));
    assert_expression(&e, "${body.isDangerous}", "false");
    assert_expression(&e, "${body.dangerous}", "false");
    // the cycle of friends is broken so the Arcs are freed
    *elephant.friend.lock().unwrap() = None;
}

/// Java `Order`.
struct Order(Vec<Value>);

impl camel_core::util::value::Bean for Order {
    fn property(&self, name: &str) -> Option<Value> {
        (name == "lines").then(|| Value::List(self.0.clone()))
    }
}

fn order() -> Value {
    Value::bean(Order(vec![
        order_line(123, "Camel in Action"),
        order_line(456, "ActiveMQ in Action"),
    ]))
}

#[test]
fn test_body_ognl_order_list() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(order());
    for (lines, id, name) in [("getLines", "getId", "getName"), ("lines", "id", "name")] {
        for (index, expected_id, expected_name) in [
            ("0", 123, "Camel in Action"),
            ("1", 456, "ActiveMQ in Action"),
            ("last", 456, "ActiveMQ in Action"),
            ("last-1", 123, "Camel in Action"),
        ] {
            assert_expression(
                &e,
                &format!("${{in.body.{lines}[{index}].{id}}}"),
                expected_id,
            );
            assert_expression(
                &e,
                &format!("${{in.body.{lines}[{index}].{name}}}"),
                expected_name,
            );
        }
        for (index, message) in [
            ("3", "Index: 3, Size: 2 out of bounds with List from bean"),
            (
                "last - 2",
                "Index: -1, Size: 2 out of bounds with List from bean",
            ),
            ("2", "Index: 2, Size: 2 out of bounds with List from bean"),
        ] {
            let err = error(&e, &format!("${{in.body.{lines}[{index}].{id}}}"));
            assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
            let c = cause(&err);
            assert!(c.is_subtype("IndexOutOfBoundsException"), "{c}");
            assert!(c.message().unwrap().starts_with(message), "{c}");
        }
        let err = error(&e, &format!("${{in.body.{lines}[last - XXX].{id}}}"));
        assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
        let c = cause(&err);
        assert!(c.is(ErrorKind::ExpressionIllegalSyntax), "{c}");
        assert_eq!(Some("last - XXX"), c.detail());
        for index in ["3", "2"] {
            assert_eq!(
                Value::Null,
                evaluate(&e, &format!("${{in.body?.{lines}[{index}].{id}}}"))
            );
        }
        let rating = if lines == "lines" {
            "rating"
        } else {
            "getRating"
        };
        let err = error(&e, &format!("${{in.body.{lines}[0]?.{rating}}}"));
        assert!(err.is_subtype("RuntimeBeanExpressionException"), "{err}");
        let c = cause(&err);
        assert!(c.is_subtype("MethodNotFoundException"), "{c}");
        assert!(c
            .message()
            .unwrap()
            .starts_with(&format!("Method with name: {rating} not found")));
    }
    assert_expression(&e, "${in.body.lines.size}", 2);
}

#[test]
fn test_body_ognl_list_map_and_array() {
    let (_t, mut e) = exchange();
    let mut cells = ValueMap::new();
    cells.insert("ABC".into(), Value::from("123"));
    cells.insert("DEF".into(), Value::from("456"));
    let mut cells2 = ValueMap::new();
    cells2.insert("HIJ".into(), Value::from("789"));
    e.message_mut()
        .set_body(Value::List(vec![Value::Map(cells), Value::Map(cells2)]));
    for (exp, expected) in [
        ("${in.body[0][ABC]}", "123"),
        ("${in.body[0][DEF]}", "456"),
        ("${in.body[0]['ABC']}", "123"),
        ("${in.body[0]['DEF']}", "456"),
        ("${in.body[1][HIJ]}", "789"),
        ("${in.body[1]['HIJ']}", "789"),
    ] {
        assert_expression(&e, exp, expected);
    }
    // Java lists and arrays are both a Value::List
    e.message_mut().set_body(Value::List(vec![
        order_line(123, "Camel in Action"),
        order_line(456, "ActiveMQ in Action"),
    ]));
    for (id, name) in [("getId", "getName"), ("id", "name")] {
        assert_expression(&e, &format!("${{in.body[0].{id}}}"), 123);
        assert_expression(&e, &format!("${{in.body[0].{name}}}"), "Camel in Action");
        assert_expression(&e, &format!("${{in.body[1].{id}}}"), 456);
        assert_expression(&e, &format!("${{in.body[1].{name}}}"), "ActiveMQ in Action");
    }
}

#[test]
fn test_body_ognl_on_string() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Camel");
    assert_expression(&e, "${body.substring(2)}", "mel");
    assert_expression(&e, "${body.substring(2, 4)}", "me");
    assert_expression(&e, "${body.length()}", 5);
    assert_expression(&e, "${body.toUpperCase()}", "CAMEL");
    assert_expression(&e, "${body.toUpperCase().substring(2)}", "MEL");
    assert_expression(&e, "${body.toLowerCase().length()}", 5);
    e.message_mut().set_header("max", 4);
    e.message_mut().set_header("min", 2);
    assert_expression(&e, "${body.substring(${header.min}, ${header.max})}", "me");
    e.message_mut().set_body(Value::Null);
    e.message_mut().set_header("name", "Camel");
    assert_expression(
        &e,
        "${header.name.substring(${header.min}, ${header.max})}",
        "me",
    );
}

#[test]
fn test_body_ognl_replace() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Kamel is a cool Kamel");
    assert_expression(
        &e,
        "${body.replace(\"Kamel\", \"Camel\")}",
        "Camel is a cool Camel",
    );
    e.message_mut().set_body("foo$bar$baz");
    assert_expression(&e, "${body.replace('$', '-')}", "foo-bar-baz");
    e.message_mut().set_body("foo\\bar\\baz");
    assert_expression(&e, "${body.replace('\\', '\\\\')}", "foo\\\\bar\\\\baz");
    e.message_mut().set_body("http:camel.apache.org");
    assert_expression(
        &e,
        "${body.replaceFirst('http:', 'https:')}",
        "https:camel.apache.org",
    );
    assert_expression(&e, "${body.replaceFirst('http:', '')}", "camel.apache.org");
    for exp in [
        "${body.replaceFirst('http:', ' ')}",
        "${body.replaceFirst('http:',    ' ')}",
        "${body.replaceFirst('http:',' ')}",
    ] {
        assert_expression(&e, exp, " camel.apache.org");
    }
    e.message_mut().set_body("Hello O'Conner");
    for (exp, expected) in [
        ("${body.replace(\"O'C\", \"OC\")}", "Hello OConner"),
        ("${body.replace(\"O'C\", \"O C\")}", "Hello O Conner"),
        ("${body.replace(\"O'C\", \"O-C\")}", "Hello O-Conner"),
        ("${body.replace(\"O'C\", \"O''C\")}", "Hello O''Conner"),
        ("${body.replace(\"O'C\", \"O\n'C\")}", "Hello O\n'Conner"),
    ] {
        assert_expression(&e, exp, expected);
    }
    e.message_mut().set_body("Bik (Ru)");
    assert_expression(
        &e,
        "${body.replaceAll(\"Bik \\(Ru\\)\",\"bik_ru\").replaceAll(\"b\",\"c\")}",
        "cik_ru",
    );
    e.message_mut().set_body("Hello (( World (((( Again");
    assert_expression(
        &e,
        "${body.replace(\"((\", \"--\").replace(\"((((\", \"----\")}",
        "Hello -- World ---- Again",
    );
}

#[test]
fn test_body_ognl_spaces() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello World");
    // "Hello World".compareTo("foo") in Java
    let foo = ('H' as i64) - ('f' as i64);
    for (exp, expected) in [
        // no quotes, which is discouraged to use
        ("${body.compareTo(Hello World)}", 0),
        ("${body.compareTo('Hello World')}", 0),
        ("${body.compareTo(${body})}", 0),
        ("${body.compareTo('foo')}", foo),
        ("${body.compareTo( 'Hello World' )}", 0),
        ("${body.compareTo( ${body} )}", 0),
        ("${body.compareTo( 'foo' )}", foo),
    ] {
        assert_expression(&e, exp, expected);
    }
}

#[test]
fn test_class_simple_name() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello");
    for exp in [
        "${body.getClass().getSimpleName()}",
        "${body.getClass.getSimpleName}",
        "${body.class.simpleName}",
    ] {
        assert_expression(&e, exp, "String");
    }
    e.set_exception(Some(CamelError::illegal_argument("Forced")));
    for exp in [
        "${exception.getClass().getSimpleName()}",
        "${exception.getClass.getSimpleName}",
        "${exception.class.simpleName}",
    ] {
        assert_expression(&e, exp, "IllegalArgumentException");
    }
}

#[test]
fn test_slash_json_like_and_function_ends() {
    let (_t, mut e) = exchange();
    assert_expression(&e, "foo/${header.foo}", "foo/abc");
    assert_expression(&e, "foo\\${header.foo}", "foo\\abc");
    e.message_mut().set_body("Something");
    assert_expression(
        &e,
        "{\n\"data\": \"${body}\"\n}",
        "{\n\"data\": \"Something\"\n}",
    );
    for (exp, expected) in [
        ("{{", "{{"),
        ("}}", "}}"),
        ("{{}}", "{{}}"),
        ("{{foo}}", "{{foo}}"),
        ("{{${body}}}", "{{Something}}"),
        ("{{${body}-${body}}}", "{{Something-Something}}"),
    ] {
        assert_expression(&e, exp, expected);
    }
}

#[test]
fn test_camel_context_ognl() {
    let (_t, e) = exchange();
    let name = e.context().name().to_string();
    assert_expression(&e, "${camelContext.getName()}", name.as_str());
    assert_expression(
        &e,
        "${camelContext.version}",
        camel_core::api::beans::CAMEL_VERSION,
    );
}

#[test]
fn test_array_length() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body(Value::List(vec![Value::from("foo"), Value::from("bar")]));
    assert_expression(&e, "${body[0]}", "foo");
    assert_expression(&e, "${body[1]}", "bar");
    assert_expression(&e, "${body.length}", 2);
    e.message_mut().set_body(Value::List(vec![
        Value::from("foo"),
        Value::from("bar"),
        Value::from("beer"),
    ]));
    assert_expression(&e, "${body.length}", 3);
    e.message_mut().set_body(Value::List(vec![
        Value::Int(1),
        Value::Int(20),
        Value::Int(300),
    ]));
    assert_expression(&e, "${body[0]}", 1);
    assert_expression(&e, "${body[1]}", 20);
    assert_expression(&e, "${body[2]}", 300);
    assert_expression(&e, "${body.length}", 3);
    e.message_mut().set_body(vec![65u8, 66, 67]);
    assert_expression(&e, "${body.length}", 3);
}

#[test]
fn test_simple_map_boolean() {
    let (_t, mut e) = exchange();
    for credit in [true, false] {
        let mut map = ValueMap::new();
        map.insert("isCredit".into(), Value::Bool(credit));
        e.message_mut().set_body(Value::Map(map));
        assert_eq!(credit, predicate(&e, "${body[isCredit]} == true"));
        assert_eq!(!credit, predicate(&e, "${body[isCredit]} == false"));
        assert_eq!(credit, predicate(&e, "${body['isCredit']} == true"));
        assert_eq!(!credit, predicate(&e, "${body['isCredit']} == false"));
        // wrong case
        assert!(!predicate(&e, "${body['IsCredit']} == true"));
    }
}

#[test]
fn test_simple_regexp() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("12345678");
    assert!(predicate(&e, "${body} regex '\\d+'"));
    assert!(!predicate(&e, "${body} regex '\\w{1,4}'"));
    let re = "${body} regex '^(tel:\\+)(974)(44)(\\d+)|^(974)(44)(\\d+)'";
    for (body, expected) in [
        ("tel:+97444549697", true),
        ("97444549697", true),
        ("tel:+87444549697", false),
        ("87444549697", false),
    ] {
        e.message_mut().set_body(body);
        assert_eq!(expected, predicate(&e, re), "{body}");
    }
}

#[test]
fn test_body_ognl_on_animal_with_ognl_params() {
    let (_t, mut e) = exchange();
    let tiger = Animal::new("tiger", 13);
    e.message_mut().set_body(animal_value(&tiger));
    e.message_mut()
        .set_header("friend", animal_value(&Animal::new("donkey", 4)));
    assert_eq!(
        Value::Null,
        evaluate(&e, "${body.setFriend(${header.friend})}")
    );
    assert_eq!("tiger", tiger.name);
    assert_eq!(13, tiger.age);
    let friend = tiger
        .friend
        .lock()
        .unwrap()
        .clone()
        .expect("Should have a friend");
    let f = friend.downcast_ref::<Animal>().unwrap();
    assert_eq!(("donkey", 4), (f.name, f.age));
}

#[test]
fn test_body_as_one_line_and_body_type() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello\nGreat\nWorld");
    assert_expression(&e, "${bodyOneLine}", "HelloGreatWorld");
    assert_expression(&e, "Hi ${bodyOneLine}", "Hi HelloGreatWorld");
    assert_expression(&e, "Hi ${bodyOneLine} Again", "Hi HelloGreatWorld Again");
    e.message_mut().set_body("Hello World");
    assert_expression(&e, "${bodyType}", "String");
}

#[test]
fn test_list_index_by_nested_function() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header(
        "ITEMS",
        Value::List(vec![Value::from("1"), Value::from("99")]),
    );
    e.message_mut().set_header("TOTAL_LOOPS", 2);
    let exp = "${header.ITEMS[${exchangeProperty.CamelLoopIndex}]}";
    e.set_property("CamelLoopIndex", 0);
    assert_expression(&e, exp, "1");
    e.set_property("CamelLoopIndex", 1);
    assert_expression(&e, exp, "99");
}

#[test]
fn test_message_timestamp_and_inlined_result_type() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("CamelMessageTimestamp", 1234);
    assert_expression(&e, "${messageTimestamp}", 1234);
    e.message_mut().set_body("123");
    for exp in ["${int:body}", "${integer:body}", "${long:body}"] {
        assert_eq!(Value::Int(123), evaluate(&e, exp));
    }
    e.message_mut().set_body("true");
    assert_eq!(Value::Bool(true), evaluate(&e, "${boolean:body}"));
    e.message_mut().set_body("Hello World");
    assert_eq!(Value::from("Hello World"), evaluate(&e, "${string:body}"));
}

#[test]
fn test_contains_function() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello Camel");
    assert!(predicate(&e, "${contains(Camel)}"));
    assert!(predicate(&e, "${contains(camel)}"));
    assert!(!predicate(&e, "${contains(world)}"));
    e.set_variable("myVar", "Cat");
    assert!(!predicate(&e, "${contains(${variable.myVar})}"));
    e.set_variable("myVar", "Camel");
    assert!(predicate(&e, "${contains(${variable.myVar})}"));
    e.message_mut()
        .set_body(texts(&["Hello", "Dog", "Cat", "Camel", "Bye", "World"]));
    assert!(predicate(&e, "${contains(camel)}"));
    assert!(predicate(&e, "${contains(world)}"));
    assert!(!predicate(&e, "${contains(fish)}"));
}

#[test]
fn test_assert_expression() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello");
    evaluate(&e, "${assert(${body} == 'Hello', 'Must be Hello')}");
    e.message_mut().set_body("Bye");
    let err = error(&e, "${assert(${body} == 'Hello', 'Must be Hello')}");
    assert!(err.is_subtype("SimpleAssertionException"), "{err}");
    assert_eq!(Some("Must be Hello"), err.message());
}

#[test]
fn test_ternary_operator() {
    let (_t, mut e) = exchange();
    // the same expression object evaluates correctly with different header values
    e.message_mut().set_header("foo", 44);
    let simple = e.context().resolve_language("simple").unwrap();
    let exp = simple
        .create_expression("${header.foo > 0 ? 'positive' : 'negative'}")
        .unwrap();
    assert_eq!(Value::from("positive"), exp.evaluate(&e).unwrap());
    e.message_mut().set_header("foo", -123);
    assert_eq!(Value::from("negative"), exp.evaluate(&e).unwrap());
    // a ternary with a constant condition
    assert_expression(&e, "${true ? 'yes' : 'no'}", "yes");
    assert_expression(&e, "${false ? 'yes' : 'no'}", "no");
    // with the body
    e.message_mut().set_body("Hello World");
    e.message_mut().set_header("foo", 44);
    assert_expression(
        &e,
        "${header.foo > 0 ? ${body} : 'Bye World'}",
        "Hello World",
    );
    e.message_mut().set_header("foo", -123);
    assert_expression(&e, "${header.foo > 0 ? ${body} : 'Bye World'}", "Bye World");
    assert_expression(&e, "${header.foo > 0 ? ${body} : ${null}}", Value::Null);
    // the ${file:name} part of the Java test waits for the file functions (see the backlog)
}

#[test]
fn test_ternary_operator_with_numbers() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("score", 85);
    assert_expression(&e, "${header.score >= 90 ? 'A' : 'B'}", "B");
    e.message_mut().set_header("score", 95);
    assert_expression(&e, "${header.score >= 90 ? 'A' : 'B'}", "A");
    e.message_mut().set_header("age", 25);
    assert_expression(&e, "${header.age >= 18 ? 'adult' : 'minor'}", "adult");
    e.message_mut().set_header("age", 15);
    assert_expression(&e, "${header.age >= 18 ? 'adult' : 'minor'}", "minor");
}

#[test]
fn test_ternary_operator_with_booleans() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("enabled", true);
    assert_expression(&e, "${header.enabled == true ? 'yes' : 'no'}", "yes");
    e.message_mut().set_header("enabled", false);
    assert_expression(&e, "${header.enabled == true ? 'yes' : 'no'}", "no");
}

#[test]
fn test_ternary_operator_with_null() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("value", Value::Null);
    assert_expression(&e, "${header.value == null ? 'empty' : 'full'}", "empty");
    e.message_mut().set_header("value", "something");
    assert_expression(&e, "${header.value == null ? 'empty' : 'full'}", "full");
}

#[test]
fn test_ternary_operator_nested() {
    let (_t, mut e) = exchange();
    let exp = "${header.score >= 90 ? 'A' : ${header.score} >= 80 ? 'B' : 'C'}";
    for (score, grade) in [(95, "A"), (85, "B"), (75, "C")] {
        e.message_mut().set_header("score", score);
        assert_expression(&e, exp, grade);
    }
}

#[test]
fn test_ternary_operator_with_strings() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("Hello");
    assert_expression(&e, "${body == 'Hello' ? 'greeting' : 'other'}", "greeting");
    e.message_mut().set_body("Goodbye");
    assert_expression(&e, "${body == 'Hello' ? 'greeting' : 'other'}", "other");
    e.message_mut().set_header("name", "John");
    assert_expression(
        &e,
        "${header.name contains 'John' ? 'found' : 'not found'}",
        "found",
    );
    e.message_mut().set_header("name", "Jane");
    assert_expression(
        &e,
        "${header.name contains 'John' ? 'found' : 'not found'}",
        "not found",
    );
}
