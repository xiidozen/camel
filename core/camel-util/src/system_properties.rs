//! A process-wide properties registry, the Rust counterpart of Java system properties
//! (`System.getProperty` / `System.setProperty`).
//!
//! Camel reads many settings from JVM system properties (`-Dcamel.main.name=...`). Rust has no
//! such registry, so this module provides one. When a property is not set explicitly, the
//! environment variable with the same name is used as fallback (also tried in the upper-case
//! form with `.` and `-` replaced by `_`, so `camel.main.name` falls back to `CAMEL_MAIN_NAME`).

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
    fn env_fallback() {
        // PATH is set in every test environment
        assert!(get_property("PATH").is_some());
        assert!(get_property("path").is_some());
    }
}
