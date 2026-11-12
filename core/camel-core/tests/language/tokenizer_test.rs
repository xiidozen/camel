//! Port of `TokenizerTest` (the tokenize language).

use crate::simple_test::exchange;
use camel_core::prelude::*;

fn tokens(e: &Exchange, t: TokenizeExpression) -> Option<Vec<String>> {
    t.evaluate_tokens(e)
        .unwrap()
        .map(|l| l.into_iter().map(|v| v.to_string()).collect())
}

const PERSONS: &str = "<persons><person>James</person><person>Claus</person><person>Jonathan</person><person>Hadrian</person></persons>";

fn persons(open: &str) -> Vec<String> {
    ["James", "Claus", "Jonathan", "Hadrian"]
        .iter()
        .map(|n| format!("{open}{n}</person>"))
        .collect()
}

#[test]
fn test_tokenize() {
    let (_t, mut e) = exchange();
    e.message_mut().set_header("names", "Claus,James,Willem");
    assert_eq!(
        Some(vec!["Claus".to_string(), "James".into(), "Willem".into()]),
        tokens(&e, tokenize(",").source("header:names"))
    );
    e.message_mut().set_body("Hadrian,Charles");
    assert_eq!(
        Some(vec!["Hadrian".to_string(), "Charles".into()]),
        tokens(&e, tokenize(","))
    );
    e.message_mut().set_body("The little fox");
    assert_eq!(
        Some(vec!["The".to_string(), "little".into(), "fox".into()]),
        tokens(&e, tokenize_regex(r"(\W+)\s*"))
    );
    e.message_mut().set_header("quote", "Camel rocks");
    assert_eq!(
        Some(vec!["Camel".to_string(), "rocks".into()]),
        tokens(&e, tokenize_regex(r"(\W+)\s*").source("header:quote"))
    );
}

#[test]
fn test_tokenize_pair() {
    let (_t, mut e) = exchange();
    e.message_mut()
        .set_body("2011-11-11\n!James@!Claus@\n2 records");
    assert_eq!(
        Some(vec!["James".to_string(), "Claus".into()]),
        tokens(&e, tokenize_pair("!", "@", false))
    );
    e.message_mut()
        .set_body("2011-11-11\n[START]James[END]\n[START]Claus[END]\n2 records");
    assert_eq!(
        Some(vec!["James".to_string(), "Claus".into()]),
        tokens(&e, tokenize_pair("[START]", "[END]", false))
    );
    e.message_mut().set_header("foo", "[START]");
    e.message_mut().set_header("bar", "[END]");
    assert_eq!(
        Some(vec!["James".to_string(), "Claus".into()]),
        tokens(&e, tokenize_pair("${header.foo}", "${header.bar}", false))
    );
    assert_eq!(
        Some(vec![
            "[START]James[END]".to_string(),
            "[START]Claus[END]".into()
        ]),
        tokens(&e, tokenize_pair("[START]", "[END]", true))
    );
}

#[test]
fn test_tokenize_xml() {
    let (_t, mut e) = exchange();
    e.message_mut().set_body(PERSONS);
    assert_eq!(
        Some(persons("<person>")),
        tokens(&e, tokenize_xml("<person>", None))
    );
    e.message_mut().set_header("foo", "<person>");
    assert_eq!(
        Some(persons("<person>")),
        tokens(&e, tokenize_xml("${header.foo}", None))
    );
    assert_eq!(
        Some(persons("<person>")),
        tokens(&e, tokenize_xml("person", None))
    );
    e.message_mut().set_body("<?xml version=\"1.0\"?><!-- bla bla --><persons>\n<person>James</person>\n<person>Claus</person>\n<!-- more bla bla --><person>Jonathan</person>\n<person>Hadrian</person>\n</persons>   ");
    assert_eq!(
        Some(persons("<person>")),
        tokens(&e, tokenize_xml("<person>", None))
    );
    e.message_mut()
        .set_body("<?xml version=\"1.0\"?><!-- bla bla --><persons></persons>   ");
    assert_eq!(Some(vec![]), tokens(&e, tokenize_xml("<person>", None)));
    e.message_mut().set_body("");
    assert_eq!(Some(vec![]), tokens(&e, tokenize_xml("<person>", None)));
    e.message_mut().set_body(Value::Null);
    assert_eq!(None, tokens(&e, tokenize_xml("<person>", None)));
    e.message_mut().set_body(
        "<?xml version='1.0' encoding='UTF-8'?>\n<Trips>\n<Trip>\n<TripType>\n</TripType>\n</Trip>\n</Trips>",
    );
    assert_eq!(
        1,
        tokens(&e, tokenize_xml("Trip", Some("Trips")))
            .unwrap()
            .len()
    );
}

#[test]
fn test_tokenize_xml_namespaces() {
    let (_t, mut e) = exchange();
    let acme = "<person xmlns=\"http:acme.com/persons\">";
    let body = "<?xml version=\"1.0\"?><persons xmlns=\"http:acme.com/persons\">\n<person>James</person>\n<person>Claus</person>\n<person>Jonathan</person>\n<person>Hadrian</person>\n</persons>\n";
    e.message_mut().set_body(body);
    assert_eq!(
        Some(persons(acme)),
        tokens(&e, tokenize_xml("<person>", Some("<persons>")))
    );
    assert_eq!(
        Some(persons("<person>")),
        tokens(&e, tokenize_xml("<person>", None))
    );
    e.message_mut().set_body("<?xml version=\"1.0\"?><persons xmlns=\"http:acme.com/persons\" xmlns:foo=\"http:foo.com\">\n<person>James</person>\n<person>Claus</person>\n<person>Jonathan</person>\n<person>Hadrian</person>\n</persons>\n");
    assert_eq!(
        Some(persons(
            "<person xmlns=\"http:acme.com/persons\" xmlns:foo=\"http:foo.com\">"
        )),
        tokens(&e, tokenize_xml("<person>", Some("<persons>")))
    );
    e.message_mut().set_body("<?xml version=\"1.0\"?><persons>\n<person xmlns=\"http:acme.com/persons\">James</person>\n<person xmlns=\"http:acme.com/persons\">Claus</person>\n<person xmlns=\"http:acme.com/persons\">Jonathan</person>\n<person xmlns=\"http:acme.com/persons\">Hadrian</person>\n</persons>\n");
    assert_eq!(
        Some(persons(acme)),
        tokens(&e, tokenize_xml("<person>", None))
    );
    let local = "<?xml version=\"1.0\"?><persons xmlns=\"http:acme.com/persons\">\n<person xmlns:foo=\"http:foo.com\">James</person>\n<person>Claus</person>\n<person>Jonathan</person>\n<person xmlns:bar=\"http:bar.com\">Hadrian</person>\n</persons>\n";
    e.message_mut().set_body(local);
    assert_eq!(
        Some(vec![
            "<person xmlns:foo=\"http:foo.com\" xmlns=\"http:acme.com/persons\">James</person>"
                .to_string(),
            "<person xmlns=\"http:acme.com/persons\">Claus</person>".into(),
            "<person xmlns=\"http:acme.com/persons\">Jonathan</person>".into(),
            "<person xmlns:bar=\"http:bar.com\" xmlns=\"http:acme.com/persons\">Hadrian</person>"
                .into(),
        ]),
        tokens(&e, tokenize_xml("<person>", Some("<persons>")))
    );
    assert_eq!(
        Some(vec![
            "<person xmlns:foo=\"http:foo.com\">James</person>".to_string(),
            "<person>Claus</person>".into(),
            "<person>Jonathan</person>".into(),
            "<person xmlns:bar=\"http:bar.com\">Hadrian</person>".into(),
        ]),
        tokens(&e, tokenize_xml("<person>", None))
    );
    let with_ids =
        |e: &Exchange, inherit: Option<&str>| tokens(e, tokenize_xml("<person>", inherit));
    let ids = |ns: &str| -> Vec<String> {
        ["James", "Claus", "Jonathan", "Hadrian"]
            .iter()
            .enumerate()
            .map(|(i, n)| format!("<person id=\"{}\"{ns}>{n}</person>", i + 1))
            .collect()
    };
    e.message_mut().set_body("<persons><person id=\"1\">James</person><person id=\"2\">Claus</person><person id=\"3\">Jonathan</person><person id=\"4\">Hadrian</person></persons>");
    assert_eq!(Some(ids("")), with_ids(&e, None));
    for root in [
        "<persons xmlns=\"http:acme.com/persons\">",
        "<persons riders=\"true\" xmlns=\"http:acme.com/persons\">",
    ] {
        e.message_mut().set_body(format!("{root}<person id=\"1\">James</person><person id=\"2\">Claus</person><person id=\"3\">Jonathan</person><person id=\"4\">Hadrian</person></persons>"));
        assert_eq!(
            Some(ids(" xmlns=\"http:acme.com/persons\"")),
            with_ids(&e, Some("<persons>"))
        );
    }
}
