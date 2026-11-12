//! Loading resources by uri (Java `ResourceHelper`): `classpath:` (searched in the classpath
//! roots `src/main/resources`, `src/test/resources` and the current directory, as Rust has no
//! classpath) and `file:`.

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use std::path::PathBuf;

/// The directories searched for `classpath:` resources.
pub const CLASSPATH_ROOTS: [&str; 3] = ["src/main/resources", "src/test/resources", "."];

/// Whether the uri starts with a resource scheme (Java `ResourceHelper.hasScheme`).
pub fn has_scheme(uri: &str) -> bool {
    ["classpath:", "file:", "http:", "https:", "ref:", "bean:"]
        .iter()
        .any(|s| uri.starts_with(s))
}

/// The path of the resource of the uri, if it exists.
pub fn resolve_resource_path(uri: &str) -> Option<PathBuf> {
    if let Some(path) = uri.strip_prefix("file:") {
        let p = PathBuf::from(path);
        return p.is_file().then_some(p);
    }
    let path = uri.strip_prefix("classpath:").unwrap_or(uri);
    let path = path.trim_start_matches('/');
    CLASSPATH_ROOTS
        .iter()
        .map(|r| PathBuf::from(r).join(path))
        .find(|p| p.is_file())
}

/// The content of the resource of the uri as text.
///
/// # Errors
///
/// A missing resource (a `FileNotFoundException`), an unsupported scheme or an IO error.
pub fn resolve_resource_as_string(uri: &str) -> CamelResult<String> {
    if !(uri.starts_with("classpath:") || uri.starts_with("file:") || !has_scheme(uri)) {
        return Err(CamelError::illegal_argument(format!(
            "Unsupported resource scheme in: {uri} (classpath: and file: are supported)"
        )));
    }
    let path = resolve_resource_path(uri).ok_or_else(|| {
        CamelError::new(
            ErrorKind::FileNotFound,
            format!("Cannot find resource: {uri} in classpath for URI: {uri}"),
        )
    })?;
    std::fs::read_to_string(&path)
        .map_err(|e| CamelError::new(ErrorKind::Io, format!("Cannot read resource: {uri}: {e}")))
}
