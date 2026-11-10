//! Port of `SimpleTest` (in progress: the tests of the ported functions; the others are still
//! in the Java test, see the backlog).

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::util::value::ValueMap;

/// The Java `ExchangeTestSupport` exchange: headers foo=abc, bar=123, a body, a property and a
/// variable.
fn exchange() -> (ContextTestSupport, Exchange) {
    let t = ContextTestSupport::new();
    let mut e = Exchange::new(t.context.as_shared());
    e.message_mut().set_header("foo", "abc");
    e.message_mut().set_header("bar", 123);
    e.message_mut().set_body("<hello id='m123'>world!</hello>");
    e.set_property("foobar", "cba");
    e.set_variable("cheese", "gauda");
    (t, e)
}

fn evaluate(e: &Exchange, text: &str) -> Value {
    let simple = e.context().resolve_language("simple").unwrap();
    simple.create_expression(text).unwrap().evaluate(e).unwrap()
}

/// Java `assertExpression`: the value converted to the type of the expected value.
fn assert_expression(e: &Exchange, text: &str, expected: impl Into<Value>) {
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
