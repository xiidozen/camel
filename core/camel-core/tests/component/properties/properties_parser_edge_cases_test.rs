//! Port of `PropertiesParserEdgeCasesTest`.

use crate::support::ContextTestSupport;
use camel_core::prelude::*;
use camel_core::util::system_properties;

fn context(nested: bool) -> ContextTestSupport {
    let t = ContextTestSupport::new();
    t.context.properties().set_initial_properties([
        ("a", "A"),
        ("dir", "C:\\temp\\"),
        ("file", "f.txt"),
    ]);
    t.context.properties().set_nested_placeholder(nested);
    t
}

fn resolve(t: &ContextTestSupport, text: &str) -> Option<String> {
    t.context.resolve_property_placeholders(text).unwrap()
}

#[test]
fn test_optional_function() {
    system_properties::set_property("camel.parser.test", "S");
    let t = context(true);
    assert_eq!(
        Some("S".to_string()),
        resolve(&t, "{{sys:?camel.parser.test}}")
    );
    assert_eq!(
        Some("S".to_string()),
        resolve(&t, "{{?sys:camel.parser.test}}")
    );
    assert_eq!(None, resolve(&t, "{{sys:?camel.parser.nope}}"));
    assert_eq!(None, resolve(&t, "{{?sys:camel.parser.nope}}"));
    system_properties::clear_property("camel.parser.test");
}

#[test]
fn test_ignore_missing_property() {
    let t = context(true);
    t.context.properties().set_ignore_missing_property(true);
    assert_eq!(Some("{{nope}}".to_string()), resolve(&t, "{{nope}}"));
    assert_eq!(
        Some("x-{{nope}}-A".to_string()),
        resolve(&t, "x-{{nope}}-{{a}}")
    );
}

#[test]
fn test_value_ending_with_backslash() {
    for nested in [true, false] {
        let t = context(nested);
        assert_eq!(
            Some("C:\\temp\\f.txt".to_string()),
            resolve(&t, "{{dir}}{{file}}")
        );
    }
}

#[test]
fn test_not_nested_optional() {
    let t = context(false);
    assert_eq!(Some("abcdef".to_string()), resolve(&t, "abc{{?x}}def"));
    assert_eq!(Some("abc".to_string()), resolve(&t, "abc{{?x}}"));
    assert_eq!(Some(String::new()), resolve(&t, "{{?x}}{{?y}}"));
}

#[test]
fn test_escaped_backslash() {
    for nested in [true, false] {
        let t = context(nested);
        assert_eq!(Some("x\\A".to_string()), resolve(&t, "x\\\\{{a}}"));
    }
}

#[test]
fn test_nested_option_only_in_placeholder() {
    let t = context(true);
    assert_eq!(
        Some("http://x?nested=true&A".to_string()),
        resolve(&t, "http://x?nested=true&{{a}}")
    );
}
