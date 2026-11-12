//! The file producer (Java `GenericFileProducer` and `FileOperations.storeFile`).

use crate::{operation_failed, FileEndpoint, FileExist};
use camel_api::endpoint::{Producer, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{FILE_NAME, FILE_NAME_PRODUCED, OVERRULE_FILE_NAME};
use camel_api::processor::{BoxFuture, Processor};
use camel_api::service::Service;
use camel_util::value::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Writes the message body to a file.
pub struct FileProducer {
    endpoint: Arc<FileEndpoint>,
}

impl FileProducer {
    pub(crate) fn new(endpoint: Arc<FileEndpoint>) -> Self {
        FileProducer { endpoint }
    }

    /// The text of the evaluated simple expression.
    fn evaluate(&self, exchange: &mut Exchange, text: &str) -> CamelResult<Option<String>> {
        let v = self.endpoint.expression(text)?.evaluate_mut(exchange)?;
        Ok((!v.is_null()).then(|| v.to_string()))
    }

    /// The path of the file to write (Java `createFileName`): the starting directory with the
    /// name of the `CamelOverruleFileName` header, the `fileName` option, the `CamelFileName`
    /// header, or the message id.
    fn create_file_name(&self, exchange: &mut Exchange) -> CamelResult<String> {
        let c = &self.endpoint.config;
        let overrule = exchange.message_mut().remove_header(OVERRULE_FILE_NAME);
        let name = match overrule {
            Some(v) if !v.is_null() => {
                exchange.message_mut().set_header(FILE_NAME, v.clone());
                Some(v.to_string())
            }
            _ => match &c.file_name {
                Some(expr) => self.evaluate(exchange, expr)?,
                None => exchange
                    .message()
                    .header(FILE_NAME)
                    .filter(|v| !v.is_null())
                    .map(|v| v.to_string()),
            },
        };
        let name = name.map(|n| {
            if c.flatten {
                n.rsplit(['/', '\\']).next().unwrap_or(&n).to_string()
            } else {
                n
            }
        });
        let base = if c.directory.is_empty()
            || c.directory.ends_with('/')
            || c.directory.ends_with('\\')
        {
            c.directory.clone()
        } else {
            format!("{}/", c.directory)
        };
        let name = match name {
            Some(n) => n,
            None => {
                // Java getGeneratedFileName: the sanitized message id
                let id = exchange
                    .message()
                    .message_id()
                    .unwrap_or(exchange.exchange_id())
                    .to_string();
                id.replace([':', '/', '\\'], "-")
            }
        };
        let answer = format!("{base}{name}");
        if c.jail_starting_directory {
            let compact = camel_util::file_util::compact_path_with(&answer, "/");
            let base_compact = camel_util::file_util::compact_path_with(&base, "/");
            if !base_compact.is_empty()
                && !Path::new(&compact).starts_with(base_compact.trim_end_matches('/'))
            {
                return Err(CamelError::illegal_argument(format!(
                    "Cannot write file with name: {compact} as the filename is jailed to the starting directory: {base_compact}"
                )));
            }
        }
        Ok(answer)
    }

    /// The path of the temporary file (Java `createTempFileName`).
    fn create_temp_file_name(&self, exchange: &mut Exchange, target: &str) -> CamelResult<String> {
        let c = &self.endpoint.config;
        let expr = match (&c.temp_file_name, &c.temp_prefix) {
            (Some(t), _) => t.clone(),
            (None, Some(p)) => format!("{p}${{file:onlyname}}"),
            (None, None) => return Ok(target.to_string()),
        };
        let had_name = exchange.message().header(FILE_NAME).is_some();
        if !had_name {
            let only = target
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(target)
                .to_string();
            exchange.message_mut().set_header(FILE_NAME, only);
        }
        let temp = self.evaluate(exchange, &expr)?.unwrap_or_default();
        if !had_name {
            exchange.message_mut().remove_header(FILE_NAME);
        }
        Ok(match target.rfind(['/', '\\']) {
            Some(i) => format!("{}{temp}", &target[..=i]),
            None => temp,
        })
    }

    /// The bytes of the body (with the `appendChars`).
    fn content(&self, exchange: &Exchange, path: &str) -> CamelResult<Vec<u8>> {
        let c = &self.endpoint.config;
        let mut bytes = match exchange.message().body() {
            Value::Null => {
                if !c.allow_null_body {
                    return Err(operation_failed(format!(
                        "Cannot write null body to file: {path}"
                    )));
                }
                Vec::new()
            }
            Value::Bytes(b) => b.clone(),
            Value::Text(t) => encode(t, c.charset.as_deref()),
            other => encode(&other.to_string(), c.charset.as_deref()),
        };
        if let Some(chars) = &c.append_chars {
            bytes.extend(encode(chars, c.charset.as_deref()));
        }
        Ok(bytes)
    }

    fn write(&self, exchange: &Exchange, path: &str, append: bool) -> CamelResult<()> {
        let c = &self.endpoint.config;
        if c.auto_create {
            if let Some(parent) = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    operation_failed(format!(
                        "Cannot create directory: {}: {e}",
                        parent.display()
                    ))
                })?;
            }
        }
        let bytes = self.content(exchange, path)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .map_err(|e| operation_failed(format!("Cannot write file: {path}: {e}")))?;
        file.write_all(&bytes)
            .map_err(|e| operation_failed(format!("Cannot write file: {path}: {e}")))?;
        log::debug!("Wrote [{path}] to [{}]", self.endpoint.uri);
        Ok(())
    }

    /// Moves the existing file away (Java `GenericFileDefaultMoveExistingFileStrategy`).
    fn move_existing(&self, exchange: &mut Exchange, target: &str) -> CamelResult<()> {
        let c = &self.endpoint.config;
        let expr = c.move_existing.clone().ok_or_else(|| {
            CamelError::illegal_argument(
                "You must configure the moveExisting option when fileExist=Move",
            )
        })?;
        let had = exchange.message().header(FILE_NAME).cloned();
        let only = target
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(target)
            .to_string();
        exchange.message_mut().set_header(FILE_NAME, only.clone());
        let name = self.evaluate(exchange, &expr)?.unwrap_or_default();
        match had {
            Some(v) => exchange.message_mut().set_header(FILE_NAME, v),
            None => {
                exchange.message_mut().remove_header(FILE_NAME);
            }
        }
        let mut to = PathBuf::from(&name);
        if !to.is_absolute() {
            to = Path::new(&c.directory).join(to);
        }
        if name.ends_with('/') || to.is_dir() {
            to = to.join(&only);
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if to.exists() {
            std::fs::remove_file(&to).map_err(|e| {
                operation_failed(format!("Cannot delete file: {}: {e}", to.display()))
            })?;
        }
        std::fs::rename(target, &to).map_err(|e| {
            operation_failed(format!(
                "Cannot rename file from: {target} to: {}: {e}",
                to.display()
            ))
        })
    }

    fn process_exchange(&self, exchange: &mut Exchange) -> CamelResult<()> {
        let c = &self.endpoint.config;
        let target = self.create_file_name(exchange)?;
        let temp = if c.temp_file_name.is_some() || c.temp_prefix.is_some() {
            Some(self.create_temp_file_name(exchange, &target)?)
        } else {
            None
        };
        let exists = Path::new(&target).exists();
        match c.file_exist {
            FileExist::Fail if exists => {
                return Err(operation_failed(format!(
                    "File already exist: {target}. Cannot write new file."
                )))
            }
            FileExist::Ignore if exists => {
                log::trace!(
                    "An existing file already exists: {target}. Ignore and do not override it."
                );
                return Ok(());
            }
            FileExist::Move if exists => self.move_existing(exchange, &target)?,
            _ => {}
        }
        match &temp {
            Some(temp) => {
                if c.file_exist != FileExist::TryRename && Path::new(temp).exists() {
                    let _ = std::fs::remove_file(temp);
                }
                if c.file_exist == FileExist::Append && exists {
                    // append to a copy of the target
                    std::fs::copy(&target, temp).map_err(|e| {
                        operation_failed(format!("Cannot copy file: {target} to {temp}: {e}"))
                    })?;
                    self.write(exchange, temp, true)?;
                } else {
                    self.write(exchange, temp, false)?;
                }
                if c.file_exist != FileExist::TryRename && Path::new(&target).exists() {
                    std::fs::remove_file(&target).map_err(|e| {
                        operation_failed(format!("Cannot delete file: {target}: {e}"))
                    })?;
                }
                std::fs::rename(temp, &target).map_err(|_| {
                    operation_failed(format!("Cannot rename file from: {temp} to: {target}"))
                })?;
            }
            None => self.write(exchange, &target, c.file_exist == FileExist::Append)?,
        }
        exchange
            .message_mut()
            .set_header(FILE_NAME_PRODUCED, target);
        Ok(())
    }
}

/// The bytes of the text in the charset (UTF-8 by default, ISO-8859-1/US-ASCII as one byte per
/// character).
fn encode(text: &str, charset: Option<&str>) -> Vec<u8> {
    match charset.map(|c| c.to_ascii_uppercase()) {
        Some(c) if c == "ISO-8859-1" || c == "LATIN1" || c == "US-ASCII" => text
            .chars()
            .map(|ch| if (ch as u32) < 256 { ch as u8 } else { b'?' })
            .collect(),
        _ => text.as_bytes().to_vec(),
    }
}

impl Service for FileProducer {}

impl Processor for FileProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(std::future::ready(self.process_exchange(exchange)))
    }
}

impl Producer for FileProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
