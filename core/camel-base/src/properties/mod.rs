//! The properties component (Java `org.apache.camel.component.properties`): resolves the
//! property placeholders (`{{key}}`) of endpoint uris, expressions and route options from the
//! initial properties, the `.properties` files of the locations, the
//! [`PropertiesSource`]s, the override properties, the environment variables and the system
//! properties (see [`camel_api::spi::properties`] for the syntax).
//!
//! The lookup order of a key (Java `DefaultPropertiesParser` and `DefaultPropertiesLookup`):
//! the environment variable and the system property (override mode, the default), the override
//! properties, the sources, the locations (in their order), the initial properties, then the
//! environment variable and the system property in fallback mode.
//!
//! Differences with Java:
//!
//! * `classpath:` locations are looked up as files under the classpath roots
//!   ([`DefaultPropertiesComponent::set_classpath_roots`], default `src/main/resources`,
//!   `src/test/resources` and the current directory): Rust has no classpath.
//! * The properties of the locations are loaded when the locations are set.
//! * Local properties (route templates), lookup listeners and the resolved values are not
//!   available yet.
//!
//! # Examples
//!
//! ```
//! use camel_api::spi::properties::PropertiesComponent;
//! use camel_base::properties::DefaultPropertiesComponent;
//!
//! let pc = DefaultPropertiesComponent::new();
//! pc.add_initial_property("cool.end", "mock:result");
//! pc.add_initial_property("cool.name", "Camel");
//! assert_eq!(Some("mock:result".to_string()), pc.parse_uri("{{cool.end}}").unwrap());
//! assert_eq!(Some("Hello Camel".to_string()), pc.parse_uri("Hello {{cool.name}}").unwrap());
//! assert_eq!(Some("World".to_string()), pc.parse_uri("{{cool.other:World}}").unwrap());
//! assert_eq!(None, pc.parse_uri("{{?cool.missing}}").unwrap());
//! assert!(pc.parse_uri("{{cool.missing}}").is_err());
//! ```

pub mod functions;
pub mod parser;

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::spi::properties::{
    PropertiesComponent, PropertiesFunction, PropertiesSource, PREFIX_TOKEN, SUFFIX_TOKEN,
};
use camel_util::charset::Charset;
use camel_util::value::Value;
use functions::{
    BooleanPropertiesFunction, EnvPropertiesFunction, ServiceHostPropertiesFunction,
    ServicePortPropertiesFunction, ServicePropertiesFunction, SysPropertiesFunction,
};
use indexmap::IndexMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Weak};

pub use parser::{MODE_FALLBACK, MODE_NEVER, MODE_OVERRIDE};

/// A location of properties (Java `PropertiesLocation`): `resolver:path;optional=true`, the
/// resolver being `classpath` (the default), `file` or `ref` (a map bound in the registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertiesLocation {
    resolver: String,
    path: String,
    optional: bool,
}

impl PropertiesLocation {
    /// Parses the location.
    pub fn new(location: &str) -> Self {
        let location = location.trim();
        let (resolver, mut rest) = match location.split_once(':') {
            Some((r, p)) => (r.to_string(), p.to_string()),
            None => ("classpath".to_string(), location.to_string()),
        };
        let mut optional = false;
        // only when the location ends with an option (a path may contain a semicolon)
        if let Some(i) = rest.rfind(';') {
            if let Some(v) = rest[i + 1..].strip_prefix("optional=") {
                optional = v.eq_ignore_ascii_case("true");
                rest.truncate(i);
            }
        }
        PropertiesLocation {
            resolver,
            path: rest,
            optional,
        }
    }

    /// The resolver (`classpath`, `file` or `ref`).
    pub fn resolver(&self) -> &str {
        &self.resolver
    }

    /// The path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether a missing location is ignored.
    pub fn is_optional(&self) -> bool {
        self.optional
    }
}

impl fmt::Display for PropertiesLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.resolver, self.path)?;
        if self.optional {
            f.write_str(";optional=true")?;
        }
        Ok(())
    }
}

/// The configuration of the component; replaced as a whole when changed, so a parse uses one
/// consistent snapshot without holding a lock (functions may resolve properties again).
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) functions: IndexMap<String, Arc<dyn PropertiesFunction>>,
    pub(crate) sources: Vec<Arc<dyn PropertiesSource>>,
    locations: Vec<PropertiesLocation>,
    location_properties: Vec<(PropertiesLocation, IndexMap<String, String>)>,
    initial: IndexMap<String, String>,
    overrides: IndexMap<String, String>,
    pub(crate) ignore_missing_property: bool,
    ignore_missing_location: bool,
    pub(crate) nested_placeholder: bool,
    pub(crate) default_fallback_enabled: bool,
    pub(crate) system_properties_mode: u8,
    pub(crate) environment_variable_mode: u8,
    classpath_roots: Vec<PathBuf>,
    encoding: Option<String>,
}

impl Config {
    /// The value of the properties of the component (Java `DefaultPropertiesLookup`).
    pub(crate) fn lookup(&self, key: &str) -> Option<String> {
        if let Some(v) = self.overrides.get(key) {
            return Some(v.clone());
        }
        for s in &self.sources {
            if let Some(v) = s.get_property(key) {
                return Some(v);
            }
        }
        for (_, props) in &self.location_properties {
            if let Some(v) = props.get(key) {
                return Some(v.clone());
            }
        }
        self.initial.get(key).cloned()
    }
}

/// The text of a property value (Java converts the non-string values to strings).
fn text_of(value: impl Into<Value>) -> String {
    value.into().to_string()
}

/// Restores the escaped double curly braces (`\{{` and `\}}`), keeping the other backslashes
/// (Java `PropertiesComponent.unescape`).
fn unescape(input: &str) -> String {
    let Some(mut index) = input.find('\\') else {
        return input.to_string();
    };
    let bytes = input.as_bytes();
    let length = bytes.len();
    let mut result = String::with_capacity(length);
    let mut start = 0;
    loop {
        result.push_str(&input[start..index]);
        start = index + 1;
        if index + 2 < length {
            let (next, after) = (bytes[index + 1], bytes[index + 2]);
            if !((next == b'{' && after == b'{') || (next == b'}' && after == b'}')) {
                result.push('\\');
            }
        } else {
            result.push('\\');
            break;
        }
        match input[start..].find('\\') {
            Some(i) => index = start + i,
            None => break,
        }
    }
    if start < length {
        result.push_str(&input[start..]);
    }
    result
}

/// The properties component (Java `PropertiesComponent`).
pub struct DefaultPropertiesComponent {
    config: RwLock<Arc<Config>>,
    context: RwLock<Option<Weak<dyn CamelContext>>>,
}

impl Default for DefaultPropertiesComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for DefaultPropertiesComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultPropertiesComponent")
            .field("locations", &self.locations())
            .finish()
    }
}

impl DefaultPropertiesComponent {
    /// A component with the default functions (`env`, `sys`, `service`, `service.host`,
    /// `service.port`; `boolean` once added to a context) and no properties.
    pub fn new() -> Self {
        let mut functions: IndexMap<String, Arc<dyn PropertiesFunction>> = IndexMap::new();
        let defaults: [Arc<dyn PropertiesFunction>; 5] = [
            Arc::new(EnvPropertiesFunction),
            Arc::new(SysPropertiesFunction),
            Arc::new(ServicePropertiesFunction),
            Arc::new(ServiceHostPropertiesFunction),
            Arc::new(ServicePortPropertiesFunction),
        ];
        for f in defaults {
            functions.insert(f.name().to_string(), f);
        }
        DefaultPropertiesComponent {
            config: RwLock::new(Arc::new(Config {
                functions,
                sources: Vec::new(),
                locations: Vec::new(),
                location_properties: Vec::new(),
                initial: IndexMap::new(),
                overrides: IndexMap::new(),
                ignore_missing_property: false,
                ignore_missing_location: false,
                nested_placeholder: true,
                default_fallback_enabled: true,
                system_properties_mode: MODE_OVERRIDE,
                environment_variable_mode: MODE_OVERRIDE,
                classpath_roots: vec![
                    PathBuf::from("src/main/resources"),
                    PathBuf::from("src/test/resources"),
                    PathBuf::from("."),
                ],
                encoding: None,
            })),
            context: RwLock::new(None),
        }
    }

    fn snapshot(&self) -> Arc<Config> {
        self.config.read().unwrap().clone()
    }

    fn update(&self, f: impl FnOnce(&mut Config)) {
        let mut guard = self.config.write().unwrap();
        let mut config = (**guard).clone();
        f(&mut config);
        *guard = Arc::new(config);
    }

    /// Sets the context (for the `ref:` locations and the `boolean` function); the engine
    /// calls it when the component is added to a context.
    pub fn set_camel_context(&self, context: Weak<dyn CamelContext>) {
        *self.context.write().unwrap() = Some(context.clone());
        self.add_properties_function(Arc::new(BooleanPropertiesFunction::new(context)));
    }

    /// Adds the property to the initial properties (the lowest precedence).
    pub fn add_initial_property(&self, key: &str, value: impl Into<Value>) {
        let value = text_of(value);
        self.update(|c| {
            c.initial.insert(key.to_string(), value);
        });
    }

    /// Replaces the initial properties.
    pub fn set_initial_properties<K: Into<String>, V: Into<Value>>(
        &self,
        properties: impl IntoIterator<Item = (K, V)>,
    ) {
        let map: IndexMap<String, String> = properties
            .into_iter()
            .map(|(k, v)| (k.into(), text_of(v)))
            .collect();
        self.update(|c| c.initial = map);
    }

    /// Adds the property to the override properties (the highest precedence after the
    /// environment variables and system properties in override mode).
    pub fn add_override_property(&self, key: &str, value: impl Into<Value>) {
        let value = text_of(value);
        self.update(|c| {
            c.overrides.insert(key.to_string(), value);
        });
    }

    /// Replaces the override properties.
    pub fn set_override_properties<K: Into<String>, V: Into<Value>>(
        &self,
        properties: impl IntoIterator<Item = (K, V)>,
    ) {
        let map: IndexMap<String, String> = properties
            .into_iter()
            .map(|(k, v)| (k.into(), text_of(v)))
            .collect();
        self.update(|c| c.overrides = map);
    }

    /// Adds the function (replacing one with the same name).
    pub fn add_properties_function(&self, function: Arc<dyn PropertiesFunction>) {
        self.update(|c| {
            c.functions.insert(function.name().to_string(), function);
        });
    }

    /// Whether there is a function with the name.
    pub fn has_properties_function(&self, name: &str) -> bool {
        self.snapshot().functions.contains_key(name)
    }

    /// Adds the source of properties (looked up before the locations).
    pub fn add_properties_source(&self, source: Arc<dyn PropertiesSource>) {
        self.update(|c| c.sources.push(source));
    }

    /// The locations.
    pub fn locations(&self) -> Vec<String> {
        self.snapshot()
            .locations
            .iter()
            .map(|l| l.to_string())
            .collect()
    }

    /// Sets the locations of the `.properties` files and loads them (Java `setLocations`).
    ///
    /// # Errors
    ///
    /// A location that cannot be loaded, unless it is optional or missing locations are
    /// ignored.
    pub fn set_locations(&self, locations: &[&str]) -> CamelResult<()> {
        let locations: Vec<PropertiesLocation> = locations
            .iter()
            .map(|l| PropertiesLocation::new(l))
            .collect();
        let config = self.snapshot();
        let mut loaded = Vec::new();
        for l in &locations {
            if let Some(props) = self.load_location(&config, l)? {
                loaded.push((l.clone(), props));
            }
        }
        self.update(|c| {
            c.locations = locations;
            c.location_properties = loaded;
        });
        Ok(())
    }

    /// Adds a location (Java `addLocation`).
    ///
    /// # Errors
    ///
    /// See [`set_locations`](Self::set_locations).
    pub fn add_location(&self, location: &str) -> CamelResult<()> {
        let mut all = self.locations();
        all.push(location.to_string());
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        self.set_locations(&refs)
    }

    fn load_location(
        &self,
        config: &Config,
        location: &PropertiesLocation,
    ) -> CamelResult<Option<IndexMap<String, String>>> {
        let missing = |message: String| {
            if location.optional || config.ignore_missing_location {
                Ok(None)
            } else {
                Err(CamelError::wrap_runtime(CamelError::new(
                    camel_api::error::ErrorKind::RuntimeCamel,
                    message,
                )))
            }
        };
        if location.resolver == "ref" {
            let context = self
                .context
                .read()
                .unwrap()
                .clone()
                .and_then(|w| w.upgrade());
            let value = context
                .and_then(|c| c.registry())
                .and_then(|r| r.lookup_by_name(&location.path));
            return match value {
                Some(Value::Map(m)) => Ok(Some(
                    m.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
                )),
                _ => missing(format!(
                    "Properties {} not found in registry",
                    location.path
                )),
            };
        }
        let path = match location.resolver.as_str() {
            "file" => Some(PathBuf::from(&location.path)),
            "classpath" => config
                .classpath_roots
                .iter()
                .map(|r| r.join(location.path.trim_start_matches('/')))
                .find(|p| p.is_file()),
            other => {
                return Err(CamelError::illegal_argument(format!(
                    "Unknown properties location resolver: {other}"
                )))
            }
        };
        let Some(path) = path.filter(|p| p.is_file()) else {
            return missing(format!(
                "Properties file {} not found in {}",
                location.path,
                if location.resolver == "file" {
                    "the file system"
                } else {
                    "the classpath roots"
                }
            ));
        };
        load_file(&path, config.encoding.as_deref()).map(Some)
    }

    /// Whether a missing property keeps its placeholder instead of failing (default `false`).
    pub fn set_ignore_missing_property(&self, ignore: bool) {
        self.update(|c| c.ignore_missing_property = ignore);
    }

    /// Whether a missing location is ignored (default `false`).
    pub fn set_ignore_missing_location(&self, ignore: bool) {
        self.update(|c| c.ignore_missing_location = ignore);
    }

    /// Whether the values of the properties may have placeholders (default `true`).
    pub fn set_nested_placeholder(&self, nested: bool) {
        self.update(|c| c.nested_placeholder = nested);
    }

    /// Whether `{{key:default}}` has a default value (default `true`).
    pub fn set_default_fallback_enabled(&self, enabled: bool) {
        self.update(|c| c.default_fallback_enabled = enabled);
    }

    /// When the system properties are looked up: [`MODE_NEVER`], [`MODE_FALLBACK`] or
    /// [`MODE_OVERRIDE`] (the default).
    pub fn set_system_properties_mode(&self, mode: u8) {
        self.update(|c| c.system_properties_mode = mode);
    }

    /// When the environment variables are looked up: [`MODE_NEVER`], [`MODE_FALLBACK`] or
    /// [`MODE_OVERRIDE`] (the default).
    pub fn set_environment_variable_mode(&self, mode: u8) {
        self.update(|c| c.environment_variable_mode = mode);
    }

    /// The charset of the `.properties` files (default ISO-8859-1, as Java).
    pub fn set_encoding(&self, encoding: Option<&str>) {
        let encoding = encoding.map(str::to_string);
        self.update(|c| c.encoding = encoding);
    }

    /// The directories the `classpath:` locations are looked up in.
    pub fn set_classpath_roots(&self, roots: Vec<PathBuf>) {
        self.update(|c| c.classpath_roots = roots);
    }

    fn parse(&self, uri: &str, keep_unresolved_optional: bool) -> CamelResult<Option<String>> {
        let config = self.snapshot();
        let mut key = uri.to_string();
        // enclose the tokens if missing
        if !key.contains(PREFIX_TOKEN) {
            key = format!("{PREFIX_TOKEN}{key}");
        }
        if !key.contains(SUFFIX_TOKEN) {
            key.push_str(SUFFIX_TOKEN);
        }
        // a key starting with ! negates a boolean value
        let negate_prefix = format!("{PREFIX_TOKEN}!");
        let negate = key.starts_with(&negate_prefix);
        if negate {
            key = format!("{PREFIX_TOKEN}{}", &key[negate_prefix.len()..]);
        }
        let mut answer = parser::parse_uri(&config, &key, keep_unresolved_optional)?;
        if negate {
            answer = answer.map(|a| {
                if a.eq_ignore_ascii_case("true") {
                    "false".to_string()
                } else if a.eq_ignore_ascii_case("false") {
                    "true".to_string()
                } else {
                    a
                }
            });
        }
        Ok(answer.map(|a| unescape(&a)))
    }
}

/// Loads a `.properties` file, trimming the values (Java
/// `AbstractLocationPropertiesSource`).
fn load_file(path: &Path, encoding: Option<&str>) -> CamelResult<IndexMap<String, String>> {
    let bytes = std::fs::read(path).map_err(|e| {
        CamelError::wrap_runtime(CamelError::new(
            camel_api::error::ErrorKind::RuntimeCamel,
            format!("Error loading properties from {}: {e}", path.display()),
        ))
    })?;
    let charset = match encoding {
        Some(e) => Charset::for_name(e).map_err(|e| CamelError::illegal_argument(e.to_string()))?,
        None => Charset::for_name("ISO-8859-1").expect("ISO-8859-1"),
    };
    let text = charset.decode(&bytes);
    let entries = camel_util::properties::parse_properties(&text)
        .map_err(|e| CamelError::illegal_argument(e.to_string()))?;
    Ok(entries
        .into_iter()
        .map(|(k, v)| (k, v.trim().to_string()))
        .collect())
}

impl PropertiesComponent for DefaultPropertiesComponent {
    fn parse_uri_keep_unresolved(
        &self,
        uri: &str,
        keep_unresolved_optional: bool,
    ) -> CamelResult<Option<String>> {
        self.parse(uri, keep_unresolved_optional)
    }

    fn resolve_property(&self, key: &str) -> Option<String> {
        let keep = self.snapshot().ignore_missing_property;
        self.parse(key, keep).ok().flatten()
    }

    fn load_properties(&self) -> Vec<(String, String)> {
        let c = self.snapshot();
        let mut all: IndexMap<String, String> = c.initial.clone();
        for (_, props) in &c.location_properties {
            all.extend(props.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        for s in &c.sources {
            if let Some(props) = s.load_properties() {
                all.extend(props);
            }
        }
        all.extend(c.overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        all.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        let l = PropertiesLocation::new(" file:/etc/app.properties;optional=true ");
        assert_eq!(
            ("file", "/etc/app.properties", true),
            (l.resolver(), l.path(), l.is_optional())
        );
        assert_eq!("file:/etc/app.properties;optional=true", l.to_string());
        let l = PropertiesLocation::new("org/foo/a;b.properties");
        assert_eq!(
            ("classpath", "org/foo/a;b.properties", false),
            (l.resolver(), l.path(), l.is_optional())
        );
    }

    #[test]
    fn unescapes() {
        assert_eq!("a{{b}}", unescape("a\\{{b\\}}"));
        assert_eq!("C:\\temp\\", unescape("C:\\temp\\"));
        assert_eq!("x\\y", unescape("x\\y"));
        assert_eq!("plain", unescape("plain"));
    }
}
