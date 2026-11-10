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
