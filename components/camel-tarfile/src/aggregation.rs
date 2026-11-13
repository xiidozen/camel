//! The tar aggregation strategy (Java `TarAggregationStrategy`).

use crate::strip_path;
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{FILE_LENGTH, FILE_NAME, FILE_NAME_ONLY};
use camel_api::processor::AggregationStrategy;
use camel_util::value::Value;

/// The property holding the entries of the zip being aggregated.
const ENTRIES: &str = "CamelTarAggregationEntries";

/// Aggregates the bodies of the exchanges in a tar archive (Java `TarAggregationStrategy`):
/// each body (not empty) is an entry, named after the message id, or the `CamelFileName`
/// header (`useFilenameHeader`; for a consumed file its name, or its path with
/// `preserveFolderStructure`). The aggregated body is the tar archive, with the file headers
/// of a file named `prefix<id>suffix` (Java binds the temporary tar file). Null bodies are
/// skipped.
#[derive(Debug, Clone)]
pub struct TarAggregationStrategy {
    file_prefix: Option<String>,
    file_suffix: String,
    allow_empty_files: bool,
    preserve_folder_structure: bool,
    use_filename_header: bool,
}

impl Default for TarAggregationStrategy {
    fn default() -> Self {
        Self::new(false, false)
    }
}

impl TarAggregationStrategy {
    /// The strategy (Java `new TarAggregationStrategy(preserveFolderStructure,
    /// useFilenameHeader)`).
    pub fn new(preserve_folder_structure: bool, use_filename_header: bool) -> Self {
        TarAggregationStrategy {
            file_prefix: None,
            file_suffix: ".tar".to_string(),
            allow_empty_files: false,
            preserve_folder_structure,
            use_filename_header,
        }
    }

    /// With the prefix of the name of the tar file.
    pub fn with_file_prefix(mut self, prefix: &str) -> Self {
        self.file_prefix = Some(prefix.to_string());
        self
    }

    /// With the suffix of the name of the tar file (default `.tar`).
    pub fn with_file_suffix(mut self, suffix: &str) -> Self {
        self.file_suffix = suffix.to_string();
        self
    }

    /// With the empty bodies added as entries.
    pub fn with_allow_empty_files(mut self, allow: bool) -> Self {
        self.allow_empty_files = allow;
        self
    }

    fn entry_name(&self, e: &Exchange) -> String {
        let header = e.message().header(FILE_NAME).map(|v| v.to_string());
        let message_id = || {
            e.message()
                .message_id()
                .unwrap_or(e.exchange_id())
                .to_string()
        };
        // a consumed file (Java File bodies)
        if e.message().header(FILE_NAME_ONLY).is_some() {
            return match header {
                Some(h) if self.preserve_folder_structure => h,
                Some(h) => strip_path(&h).to_string(),
                None => message_id(),
            };
        }
        match header {
            Some(h) if self.use_filename_header => h,
            _ => message_id(),
        }
    }
}

/// The entries of the aggregated exchange (name, content).
fn entries_of(e: &Exchange) -> Vec<(String, Vec<u8>)> {
    match e.property(ENTRIES) {
        Some(Value::List(l)) => l
            .iter()
            .filter_map(|v| match v {
                Value::List(p) if p.len() == 2 => match (&p[0], &p[1]) {
                    (Value::Text(n), Value::Bytes(b)) => Some((n.clone(), b.clone())),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl AggregationStrategy for TarAggregationStrategy {
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        let Some(new) = new_exchange else {
            return Ok(old_exchange);
        };
        if new.message().body().is_null() && old_exchange.is_some() {
            return Ok(old_exchange);
        }
        let (mut answer, mut entries, file_name) = match old_exchange {
            None => {
                let name = format!(
                    "{}{}{}",
                    self.file_prefix.as_deref().unwrap_or_default(),
                    new.exchange_id(),
                    self.file_suffix
                );
                (new.clone(), Vec::new(), name)
            }
            Some(old) => {
                let entries = entries_of(&old);
                let name = old
                    .message()
                    .header(FILE_NAME)
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                (old, entries, name)
            }
        };
        let body = new.message().body();
        if !body.is_null() {
            let bytes = camel_api::spi::data_format::input_bytes(body).map_err(|e| {
                CamelError::new(
                    camel_api::error::ErrorKind::GenericFileOperationFailed,
                    e.to_string(),
                )
            })?;
            if self.allow_empty_files || !bytes.is_empty() {
                let name = self.entry_name(&new);
                entries.push((name, bytes));
            }
        }
        let zip = crate::tar_entries(entries.iter().map(|(n, c)| (n.as_str(), &c[..])))?;
        let m = answer.message_mut();
        m.set_header(FILE_NAME, file_name.as_str());
        m.set_header(FILE_NAME_ONLY, file_name.as_str());
        m.set_header(FILE_LENGTH, zip.len() as i64);
        m.set_body(Value::Bytes(zip));
        answer.set_property(
            ENTRIES,
            Value::List(
                entries
                    .into_iter()
                    .map(|(n, c)| Value::List(vec![Value::Text(n), Value::Bytes(c)]))
                    .collect(),
            ),
        );
        Ok(Some(answer))
    }
}

/// The names of the entries of a tar archive (for tests and checks).
///
/// # Errors
///
/// The data is not a tar archive.
pub fn entry_names(data: &[u8]) -> CamelResult<Vec<String>> {
    Ok(crate::untar_entries(data, u64::MAX)?
        .into_iter()
        .map(|(n, _, _)| n)
        .collect())
}
