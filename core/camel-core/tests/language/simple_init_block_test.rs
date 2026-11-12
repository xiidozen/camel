//! Ports of `SimpleInitBlockTest`, `SimpleInitBlockEdgeCasesTest`,
//! `SimpleInitBlockOverlappingKeysTest` and `SimpleInitBlockFunctionTest`.

use crate::simple_test::{assert_expression, assert_predicate, exchange};
use camel_core::prelude::*;

const INIT: &str = "$init{
  $sum := ${sum(${header.lines},100)};
  $sku := ${body contains 'Camel' ? '123' : '999'};
}init$
orderId=$sku,total=$sum
";

const INIT2: &str = "$init{
  $sum := ${sum(${header.lines},100)};
  $sku := ${body contains 'Camel' ? '123' : '999'};
}init$
$sum > 200 && $sku != 999
";

const INIT3: &str = "$init{
  $sum := ${sum(${header.lines},100)};
  $sku := ${body contains 'Camel' ? '123' : '999'};
}init$
";

const INIT4: &str = "$init{
  $sum := ${sum(${header.lines},100)};
  $sku := ${body contains 'Hi := Me $sku' ? '123' : '999'};
}init$
orderId=$sku,total=$sum
";

const INIT5: &str = "$init{
  $sum := ${sum(${header.lines},100)};
  $sku := ${body contains 'Hi := Me $sku'
               ?
             '123'
               :
             '999'
          };
}init$
orderId=$sku,total=$sum
";

const INIT6: &str = "$init{
  $greeting := ${uppercase('Hello ${body}')};
  $level := ${header.code > 999 ? 'Gold' : 'Silver'};
}init$
{
  \"message\": \"$greeting\",
  \"status\": \"$level\"
}
";

fn with(
    body: impl Into<Value>,
    header: &str,
    value: impl Into<Value>,
) -> (crate::support::ContextTestSupport, Exchange) {
    let (t, mut e) = exchange();
    e.message_mut().set_body(body);
    e.message_mut().set_header(header, value);
    (t, e)
}

#[test]
fn test_init_block_expression() {
    let (_t, e) = with("Hello Camel", "lines", "75,33");
    assert_expression(&e, INIT, "orderId=123,total=208\n");
}

#[test]
fn test_init_block_only_expression() {
    let (_t, mut e) = with("Hello Camel", "lines", "75,33");
    let exp = e
        .context()
        .resolve_language("simple")
        .unwrap()
        .create_expression(INIT3)
        .unwrap();
    assert_eq!(Value::from(""), exp.evaluate_mut(&mut e).unwrap());
    assert_eq!(Some(&Value::from("123")), e.variable("sku"));
    assert_eq!(Some(&Value::from(208)), e.variable("sum"));
}

#[test]
fn test_init_block_predicate() {
    for (body, lines, expected) in [
        ("Hello Camel", "75,33", true),
        ("Hello Camel", "3,5", false),
        ("Hello World", "75,99", false),
        ("Hello World", "3,5", false),
    ] {
        let (_t, e) = with(body, "lines", lines);
        assert_predicate(&e, INIT2, expected);
    }
}

#[test]
fn test_init_block_assignment_in_function_and_span_lines() {
    let (_t, e) = with("Hello Hi := Me $sku", "lines", "75,33");
    assert_expression(&e, INIT4, "orderId=123,total=208\n");
    let (_t, e) = with("Hello Hi := Me $sku", "lines", "76,34");
    assert_expression(&e, INIT5, "orderId=123,total=210\n");
}

#[test]
fn test_init_block_example() {
    let (_t, e) = with("joe", "code", "456");
    assert_expression(
        &e,
        INIT6,
        "{\n  \"message\": \"HELLO JOE\",\n  \"status\": \"Silver\"\n}\n",
    );
}

#[test]
fn test_init_block_values() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(1);
    e.message_mut().set_header("foo", 2);
    e.message_mut().set_header("bar", 3);
    let cases = [
        ("$init{\n  $a := ${body};\n  $b := ${header.foo};\n  $c := ${header.bar};\n}init$\naverage: ${average($a,$b,$c)}\n", "average: 2\n"),
        ("$init{\n  $a := ${val(4)};\n  $b := ${val(5)};\n  $c := ${val(6)};\n}init$\naverage: ${average($a,$b,$c)}\n", "average: 5\n"),
        ("$init{\n  $a := '5';\n  $b := '6';\n  $c := '7';\n}init$\naverage: ${average($a,$b,$c)}\n", "average: 6\n"),
        ("$init{\n  $a := 6;\n  $b := 7;\n  $c := 8;\n}init$\n${average($a,$b,$c)}\n", "7\n"),
    ];
    for (exp, expected) in cases {
        assert_expression(&e, exp, expected);
    }
    let boolean = "$init{\n  $a := true;\n  $b := false;\n}init$\n${body != null ? $a : $b}\n";
    assert_expression(&e, boolean, "true\n");
    e.message_mut().set_body(Value::Null);
    assert_expression(&e, boolean, "false\n");
    e.message_mut().set_body("Camel");
    assert_expression(
        &e,
        "$init{\n  $bar := ${val(Hi from ${body})};\n}init$\n$bar\n",
        "Hi from Camel\n",
    );
    assert_expression(
        &e,
        "$init{\n  $bar := 'Hi from ${body}';\n}init$\n$bar\n",
        "Hi from Camel\n",
    );
}

#[test]
fn test_edge_cases() {
    let (_t, mut e) = exchange();
    assert_expression(
        &e,
        "$init{\n  // $foo := 'x';\n  $bar := 'y';\n}init$\n[${variable.foo}][$bar]",
        "[][y]",
    );
    assert_expression(
        &e,
        "$init{\n  $bar := 'y'; // a trailing comment\n}init$\n[$bar]",
        "[y]",
    );
    assert_expression(
        &e,
        "$init{\n  $url := 'http://camel.apache.org';\n}init$\n$url",
        "http://camel.apache.org",
    );
    assert_expression(&e, "$init{ $minAge := 18; }init$[$minAge]", "[18]");
    assert_expression(&e, "$init{ $a := 'x'; $b := 'y'; }init$[$a$b]", "[xy]");
    assert_expression(&e, "$init{ $a := 'x; y'; }init$[$a]", "[x; y]");
    assert_expression(
        &e,
        "$init{\r\n  $a := 'x';\r\n  $b := 'y';\r\n}init$\r\n[$a$b]",
        "[xy]",
    );
    e.message_mut().set_body(" A ");
    assert_predicate(
        &e,
        "$init{\n  $clean ~:= ${trim()};\n}init$\n$clean() == 'A'",
        true,
    );
    e.message_mut().set_body("  abc  ");
    assert_expression(&e, "$init{\n  $f ~:= ${trim()};\n}init$\n[$f()]", "[abc]");
    assert_expression(
        &e,
        "$init{\n  $f ~:= ${uppercase()};\n}init$\n[$f()]",
        "[  ABC  ]",
    );
    // the error points into the init block
    let text = "$init{\n  $a = 5;\n}init$";
    let err = e
        .context()
        .resolve_language("simple")
        .unwrap()
        .create_expression(text)
        .err()
        .expect("a syntax error");
    assert!(err.is(ErrorKind::ExpressionIllegalSyntax), "{err}");
    let index = text.find('=').unwrap();
    assert!(
        err.message()
            .unwrap_or_default()
            .contains(&format!("at location {index}\n")),
        "{err}"
    );
}

#[test]
fn test_overlapping_keys() {
    let (_t, e) = exchange();
    assert_expression(
        &e,
        "$init{\n  $n := 'Bob';\n  $name := 'Alice';\n}init$\nfull=$name,short=$n\n",
        "full=Alice,short=Bob\n",
    );
    assert_expression(
        &e,
        "$init{\n  $v := 'one';\n  $val := 'two';\n  $value := 'three';\n}init$\n$value,$val,$v\n",
        "three,two,one\n",
    );
}

const CLEAN: &str = "  $clean ~:= ${trim()} ~> ${normalizeWhitespace()} ~> ${uppercase()};\n";

#[test]
fn test_init_block_functions() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body("   Hello  big   World      ");
    let words = "You said: HELLO BIG WORLD in 3 words\n";
    let cases = [
        (format!("$init{{\n{CLEAN}}}init$\nYou said: $clean()\n"), "You said: HELLO BIG WORLD\n"),
        (format!("$init{{\n{CLEAN}  $count ~:= ${{function(clean)}} ~> ${{split(' ')}} ~> ${{size()}};\n}}init$\nYou said: $clean() in $count() words\n"), words),
        (format!("$init{{\n{CLEAN}  $count ~:= ${{quote()}} ~> ${{function(clean)}} ~> ${{unquote()}} ~> ${{trim()}} ~> ${{split(' ')}} ~> ${{size()}};\n}}init$\nYou said: $clean() in $count() words\n"), words),
        (format!("$init{{\n{CLEAN}  $count ~:= ${{clean()}} ~> ${{split(' ')}} ~> ${{size()}};\n}}init$\nYou said: $clean() in $count() words\n"), words),
        (format!("$init{{\n{CLEAN}  $count ~:= ${{quote()}} ~> ${{clean()}} ~> ${{unquote()}} ~> ${{trim()}} ~> ${{split(' ')}} ~> ${{size()}};\n}}init$\nYou said: $clean() in $count() words\n"), words),
        (format!("$init{{\n{CLEAN}}}init$\nYou said: ${{clean()}}\n"), "You said: HELLO BIG WORLD\n"),
    ];
    for (exp, expected) in &cases {
        assert_expression(&e, exp, *expected);
    }
    e.message_mut().set_body("Hello World");
    assert_expression(
        &e,
        &format!("$init{{\n{CLEAN}}}init$\nYou said: ${{clean('  Clean this text  please ...    ')}} and then do something else\n"),
        "You said: CLEAN THIS TEXT PLEASE ... and then do something else\n",
    );
}
