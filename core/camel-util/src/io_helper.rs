//! IO helper functions. Port of `org.apache.camel.util.IOHelper`.

use crate::string_helper::camel_case_to_dash;

/// Looks up the environment variable, trying the key in upper case, then normalized (dashes
/// and dots as underscores), and for camelCase keys with underscores between the words.
///
/// # Arguments
///
/// * `key` - the key such as `foo.service-host` or `fooServiceHost`
///
/// # Returns
///
/// The value, or `None` if no such environment variable exists.
///
/// # Examples
///
/// ```
/// use camel_util::io_helper::lookup_environment_variable;
/// // PATH is always set
/// assert!(lookup_environment_variable("path").is_some());
/// ```
pub fn lookup_environment_variable(key: &str) -> Option<String> {
    let upper_key = key.to_uppercase();
    std::env::var(&upper_key)
        .ok()
        .or_else(|| std::env::var(normalize_environment_variable(&upper_key)).ok())
        // camelCase keys should use underscore as separator
        .or_else(|| std::env::var(normalize_environment_variable(&camel_case_to_dash(key))).ok())
}

/// Converts the given key into an OS environment variable name: upper case, with dashes and
/// dots converted to underscores.
///
/// # Examples
///
/// ```
/// use camel_util::io_helper::normalize_environment_variable;
/// assert_eq!("CAMEL_MAIN_NAME", normalize_environment_variable("camel.main-name"));
/// ```
pub fn normalize_environment_variable(key: &str) -> String {
    key.to_uppercase().replace(['-', '.'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_environment_variable() {
        std::env::set_var("CAMEL_UTIL_IO_HELPER_TEST_VAR", "found");
        assert_eq!(
            Some("found".to_string()),
            lookup_environment_variable("camel_util_io_helper_test_var")
        );
        assert_eq!(
            Some("found".to_string()),
            lookup_environment_variable("camel.util.io-helper.test.var")
        );
        assert_eq!(
            Some("found".to_string()),
            lookup_environment_variable("camelUtilIoHelperTestVar")
        );
        assert_eq!(
            None,
            lookup_environment_variable("camel.util.io-helper.not.there")
        );
    }
}
