//! Helper for property locations. Port of `org.apache.camel.util.LocationHelper`.

use crate::file_util::strip_path;
use crate::properties::OrderedLocationProperties;

/// The location as human-readable text, for the given key of the properties.
///
/// The scheme and path of the location are removed, long names are clipped to the last 28
/// characters, and the well-known locations are turned into friendly names (`initial` and
/// `override` -> `camel-main`, `SYS` -> `JVM System Property`, `ENV` -> `OS Environment
/// Variable`, `arguments` and `CLI` -> `Command Line`). The result is `[location]` padded to
/// 30 characters.
///
/// # Arguments
///
/// * `properties` - the properties
/// * `key` - the key
///
/// # Returns
///
/// The location, or `[]` padded if not possible to resolve a location.
///
/// # Examples
///
/// ```
/// use camel_util::{location_helper::location_summary, properties::OrderedLocationProperties};
/// let mut p = OrderedLocationProperties::new();
/// p.put_location("classpath:conf/application.properties", "a", "1");
/// assert_eq!("[application.properties]      ", location_summary(&p, "a"));
/// ```
pub fn location_summary(properties: &OrderedLocationProperties, key: &str) -> String {
    let mut loc = properties.get_location(key).unwrap_or("").to_string();
    // remove scheme to make it shorter
    if let Some(pos) = loc.find(':') {
        loc = loc[pos + 1..].to_string();
    }
    // strip paths so location is only the name
    loc = strip_path(&loc).to_string();
    // clip long name
    let n = loc.chars().count();
    if n > 28 {
        loc = loc.chars().skip(n - 28).collect();
    }
    let loc = match loc.as_str() {
        "initial" | "override" => "camel-main".to_string(),
        "SYS" => "JVM System Property".to_string(),
        "ENV" => "OS Environment Variable".to_string(),
        "arguments" | "CLI" => "Command Line".to_string(),
        _ => loc,
    };
    format!("{:<30}", format!("[{loc}]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let mut p = OrderedLocationProperties::new();
        p.put_location("ENV", "a", "1");
        p.put_location("initial", "b", "1");
        p.put_location(
            "file:/very/long/path/to/a-really-long-application-name.properties",
            "c",
            "1",
        );
        assert_eq!("[OS Environment Variable]     ", location_summary(&p, "a"));
        assert_eq!("[camel-main]                  ", location_summary(&p, "b"));
        // clipped to the last 28 characters, like the Java version
        assert_eq!("[-application-name.properties]", location_summary(&p, "c"));
        assert_eq!(
            "[]                            ",
            location_summary(&p, "missing")
        );
    }
}
