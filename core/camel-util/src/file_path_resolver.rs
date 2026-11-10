//! A resolver for file paths that supports resolving with system and environment properties.
//! Port of `org.apache.camel.util.FilePathResolver`.

use crate::io_helper::lookup_environment_variable;
use crate::string_helper::{count_char, split_on_character};
use crate::system_properties;

/// Resolves the path.
///
/// The pattern is:
///
/// * `${env:key}` or `${env.key}` for environment variables (see
///   [`lookup_environment_variable`]).
/// * `${key}` for system properties (see [`crate::system_properties`]).
///
/// For example: `${env.KARAF_HOME}/data/logs`. Placeholders that cannot be resolved are kept
/// as-is.
///
/// # Arguments
///
/// * `path` - the path
///
/// # Returns
///
/// The resolved path.
///
/// # Examples
///
/// ```
/// use camel_util::{file_path_resolver::resolve_path, system_properties};
/// system_properties::set_property("beer", "Carlsberg");
/// assert_eq!("/foo/Carlsberg", resolve_path("/foo/${beer}"));
/// ```
pub fn resolve_path(path: &str) -> String {
    let mut path = path.to_string();
    let count = count_char(&path, '}') + 1;
    if count <= 1 {
        return path;
    }
    for fun in split_on_character(&path.clone(), "}", count)
        .into_iter()
        .flatten()
    {
        for prefix in ["${env:", "${env."] {
            if let Some(pos) = fun.find(prefix) {
                let key = &fun[pos + prefix.len()..];
                if let Some(value) = lookup_environment_variable(key) {
                    path = path.replace(&format!("{prefix}{key}}}"), &value);
                }
            }
        }
    }
    let count = count_char(&path, '}') + 1;
    if count <= 1 {
        return path;
    }
    for fun in split_on_character(&path.clone(), "}", count)
        .into_iter()
        .flatten()
    {
        if let Some(pos) = fun.find("${") {
            let key = &fun[pos + 2..];
            if let Some(value) = system_properties::get_property(key) {
                path = path.replace(&format!("${{{key}}}"), &value);
            }
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_path_resolver() {
        // the Java build set FOO_SERVICE_HOST=myserver for the test JVM
        std::env::set_var("FOO_SERVICE_HOST", "myserver");
        assert_eq!("/foo/bar", resolve_path("/foo/bar"));
        assert_eq!(
            "/foo/myserver/bar",
            resolve_path("/foo/${env:FOO_SERVICE_HOST}/bar")
        );
        assert_eq!(
            "/foo/myserver/bar",
            resolve_path("/foo/${env.FOO_SERVICE_HOST}/bar")
        );

        let tmp = system_properties::get_property("java.io.tmpdir").unwrap();
        assert_eq!(format!("{tmp}foo"), resolve_path("${java.io.tmpdir}foo"));
        system_properties::set_property("beer", "Carlsberg");
        assert_eq!(
            format!("{tmp}foo/Carlsberg"),
            resolve_path("${java.io.tmpdir}foo/${beer}")
        );
        assert_eq!(
            format!("/myprefix/{tmp}bar/Carlsberg"),
            resolve_path("/myprefix/${java.io.tmpdir}bar/${beer}")
        );
        assert_eq!(
            "/foo/myserver/bar/Carlsberg",
            resolve_path("/foo/${env:FOO_SERVICE_HOST}/bar/${beer}")
        );
        assert_eq!(
            "/foo/myserver/bar/Carlsberg",
            resolve_path("/foo/${env.FOO_SERVICE_HOST}/bar/${beer}")
        );
        // unknown placeholders are kept
        assert_eq!(
            "/foo/${camel.unknown.prop.xyz}",
            resolve_path("/foo/${camel.unknown.prop.xyz}")
        );
    }
}
