//! Port of `PropertiesComponentIgnoreMissingPropertyTest`.

use super::with_location;
use camel_core::api::spi::properties::PropertiesComponent;
use camel_core::support::endpoint_helper::resolve_endpoint_uri_property_placeholders;

#[test]
fn test_ignore_missing_property() {
    let t = with_location();
    let pc = t.context.properties();
    pc.set_ignore_missing_property(true);
    assert_eq!(
        Some("{{?foo}}".to_string()),
        pc.resolve_property("{{?foo}}")
    );
    assert_eq!(Some("{{foo}}".to_string()), pc.resolve_property("{{foo}}"));
    assert_eq!(
        Some("10".to_string()),
        pc.resolve_property("{{myQueueSize}}")
    );
    let resolve =
        |uri: &str| resolve_endpoint_uri_property_placeholders(t.context.as_ref(), uri).unwrap();
    assert_eq!("{{?foo}}", resolve("{{?foo}}"));
    assert_eq!("{{foo}}", resolve("{{foo}}"));
    assert_eq!(
        "foo:dummy?a=1&b={{foo}}&c=10",
        resolve("foo:dummy?a=1&b={{foo}}&c={{myQueueSize}}")
    );
}
