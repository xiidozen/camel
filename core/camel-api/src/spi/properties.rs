//! Property placeholders (Java `org.apache.camel.spi.PropertiesComponent`,
//! `PropertiesFunction` and `PropertiesSource`): `{{key}}` in endpoint uris, expressions and
//! route options is replaced by the value of the property `key`.
//!
//! The syntax:
//!
//! * `{{key}}` - the property, an error if it is missing
//! * `{{key:default}}` - the property, else the default value
//! * `{{?key}}` - an optional property: nothing if it is missing
//! * `{{!key}}` - a boolean property negated
//! * `{{env:NAME}}`, `{{sys:name}}`, `{{service:name}}`, ... - a function
//!   ([`PropertiesFunction`]) computing the value from the text after the colon
//! * `\{{` - a literal `{{`
//!
//! The implementation is `camel_base::properties::DefaultPropertiesComponent`.

use crate::error::CamelResult;

/// The prefix of a property placeholder.
pub const PREFIX_TOKEN: &str = "{{";

/// The suffix of a property placeholder.
pub const SUFFIX_TOKEN: &str = "}}";

/// The prefix of the key of an optional property placeholder (`{{?key}}`).
pub const OPTIONAL_TOKEN: &str = "?";

/// A function of property placeholders, `{{name:remainder}}` (Java `PropertiesFunction`), such
/// as `{{env:HOME}}`.
pub trait PropertiesFunction: Send + Sync {
    /// The name of the function (the text before the colon).
    fn name(&self) -> &str;

    /// The value for the text after the colon, `None` if there is none.
    fn apply(&self, remainder: &str) -> Option<String>;

    /// Whether the remainder is a property key to resolve first, the function applying to its
    /// value.
    fn lookup_first(&self, remainder: &str) -> bool {
        let _ = remainder;
        false
    }

    /// Whether a missing value of the remainder is allowed (as an optional placeholder).
    fn optional(&self, remainder: &str) -> bool {
        let _ = remainder;
        false
    }

    /// Whether the values of the function are sensitive (such as a vault), masked in logs.
    fn is_sensitive(&self) -> bool {
        false
    }
}

/// A source of properties (Java `PropertiesSource`), such as a `.properties` file or a vault.
pub trait PropertiesSource: Send + Sync {
    /// The name of the source.
    fn name(&self) -> &str;

    /// The value of the property, `None` if the source does not have it.
    fn get_property(&self, name: &str) -> Option<String>;

    /// All the properties of the source, if it can list them.
    fn load_properties(&self) -> Option<Vec<(String, String)>> {
        None
    }
}

/// Resolves property placeholders (Java `org.apache.camel.spi.PropertiesComponent`).
pub trait PropertiesComponent: Send + Sync {
    /// Replaces the placeholders of the text; a text without `{{` is a single key. `None` when
    /// the text is a single optional placeholder of a missing property.
    ///
    /// # Errors
    ///
    /// An `IllegalArgumentException` for a missing property, an unbalanced placeholder or a
    /// circular reference.
    fn parse_uri(&self, uri: &str) -> CamelResult<Option<String>> {
        self.parse_uri_keep_unresolved(uri, false)
    }

    /// Replaces the placeholders, keeping the optional placeholders of missing properties
    /// unresolved when asked (Java `parseUri(uri, keepUnresolvedOptional)`).
    ///
    /// # Errors
    ///
    /// See [`parse_uri`](Self::parse_uri).
    fn parse_uri_keep_unresolved(
        &self,
        uri: &str,
        keep_unresolved_optional: bool,
    ) -> CamelResult<Option<String>>;

    /// The value of the property key (with placeholders resolved), `None` if it is missing.
    fn resolve_property(&self, key: &str) -> Option<String> {
        self.parse_uri(key).ok().flatten()
    }

    /// All the properties of the initial, location and override properties and of the sources
    /// that can list them.
    fn load_properties(&self) -> Vec<(String, String)>;
}
