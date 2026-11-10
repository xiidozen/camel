//! Helper for the user home directory. Port of `org.apache.camel.util.HomeHelper`.

use crate::system_properties;

/// Resolves the user home directory.
///
/// Uses the `user.home` system property, falling back to the `HOME` environment variable if it
/// is blank or starts with `?` (which can be returned on Linux in some weird situations).
pub fn resolve_home_dir() -> Option<String> {
    match system_properties::get_property("user.home") {
        Some(dir) if !dir.trim().is_empty() && !dir.starts_with('?') => Some(dir),
        _ => std::env::var("HOME").ok(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolves() {
        assert!(super::resolve_home_dir().is_some());
    }
}
