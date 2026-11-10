//! A process-wide properties registry, the Rust counterpart of Java system properties
//! (`System.getProperty` / `System.setProperty`).
//!
//! Camel reads many settings from JVM system properties (`-Dcamel.main.name=...`). Rust has no
//! such registry, so this module provides one. When a property is not set explicitly, the
//! environment variable with the same name is used as fallback (also tried in the upper-case
//! form with `.` and `-` replaced by `_`, so `camel.main.name` falls back to `CAMEL_MAIN_NAME`).
//!
//! The standard JVM properties that Camel code reads are provided as built-in defaults:
//! `java.io.tmpdir`, `user.dir`, `user.home`, `user.name`, `os.name`, `os.arch`,
//! `file.separator`, `path.separator` and `line.separator`.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

fn registry() -> &'static RwLock<HashMap<String, String>> {
    static PROPS: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    PROPS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Gets the property with the given name, falling back to the environment.
pub fn get_property(name: &str) -> Option<String> {
    if let Some(v) = registry().read().unwrap().get(name) {
        return Some(v.clone());
    }
    std::env::var(name)
        .ok()
        .or_else(|| std::env::var(name.to_uppercase().replace(['.', '-'], "_")).ok())
        .or_else(|| builtin_property(name))
}

/// The value of a standard JVM system property, computed from the Rust runtime.
fn builtin_property(name: &str) -> Option<String> {
    let path = |p: std::path::PathBuf| p.to_string_lossy().into_owned();
    match name {
        "java.io.tmpdir" => Some(path(std::env::temp_dir())),
        "user.dir" => std::env::current_dir().ok().map(path),
        "user.home" => std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok(),
        "user.name" => std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok(),
        "os.name" => Some(
            match std::env::consts::OS {
                "linux" => "Linux",
                "macos" => "Mac OS X",
                "windows" => "Windows",
                other => other,
            }
            .to_string(),
        ),
        "os.arch" => Some(std::env::consts::ARCH.to_string()),
        "file.separator" => Some(std::path::MAIN_SEPARATOR.to_string()),
        "path.separator" => Some(if cfg!(windows) { ";" } else { ":" }.to_string()),
        "line.separator" => Some(if cfg!(windows) { "\r\n" } else { "\n" }.to_string()),
        _ => None,
    }
}

/// Sets the property, returning the previous value if any.
pub fn set_property(name: &str, value: &str) -> Option<String> {
    registry()
        .write()
        .unwrap()
        .insert(name.to_string(), value.to_string())
}

/// Removes the property, returning the previous value if any.
pub fn clear_property(name: &str) -> Option<String> {
    registry().write().unwrap().remove(name)
}

/// Returns a snapshot of all the explicitly set properties.
pub fn properties() -> HashMap<String, String> {
    registry().read().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_clear() {
        assert_eq!(None, get_property("camel.test.sysprop.x"));
        assert_eq!(None, set_property("camel.test.sysprop.x", "1"));
        assert_eq!(Some("1".to_string()), get_property("camel.test.sysprop.x"));
        assert!(properties().contains_key("camel.test.sysprop.x"));
        assert_eq!(
            Some("1".to_string()),
            clear_property("camel.test.sysprop.x")
        );
        assert_eq!(None, get_property("camel.test.sysprop.x"));
    }

    #[test]
    fn builtin_properties() {
        assert!(get_property("java.io.tmpdir").is_some());
        assert!(get_property("user.dir").is_some());
        assert_eq!(
            Some(std::path::MAIN_SEPARATOR.to_string()),
            get_property("file.separator")
        );
        assert!(get_property("os.name").is_some());
    }

    #[test]
    fn env_fallback() {
        // PATH is set in every test environment
        assert!(get_property("PATH").is_some());
        assert!(get_property("path").is_some());
    }
}
