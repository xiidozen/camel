//! The file producer (Java `GenericFileProducer` and `FileOperations.storeFile`).

use crate::{operation_failed, FileEndpoint, FileExist};
use camel_api::endpoint::{Producer, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    FILE_CHECKSUM, FILE_NAME, FILE_NAME_ONLY, FILE_NAME_PRODUCED, FILE_PARENT, OVERRULE_FILE_NAME,
};
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
    /// name of the `fileName` option or the `CamelFileName` header (replaced by the
    /// `CamelOverruleFileName` header), or the message id.
    fn create_file_name(&self, exchange: &mut Exchange) -> CamelResult<String> {
        let c = &self.endpoint.config;
        // the overrule is the file name from now on, the fileName option evaluated with it
        if let Some(v) = exchange.message_mut().remove_header(OVERRULE_FILE_NAME) {
            if !v.is_null() {
                exchange
                    .message_mut()
                    .set_header(FILE_NAME, Value::Text(v.to_string()));
            }
        }
        let name = match &c.file_name {
            Some(expr) => self.evaluate(exchange, expr)?,
            None => exchange
                .message()
                .header(FILE_NAME)
                .filter(|v| !v.is_null())
                .map(|v| v.to_string()),
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
            None => generated_file_name(exchange),
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

    /// Writes the content of the message to the file.
    ///
    /// # Returns
    ///
    /// The bytes written.
    fn write(&self, exchange: &Exchange, path: &str, append: bool) -> CamelResult<Vec<u8>> {
        let c = &self.endpoint.config;
        if c.auto_create {
            if let Some(parent) = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                // the directories created get chmodDirectory (Java buildDirectory)
                let mut created = Vec::new();
                let mut dir = Some(parent);
                while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty() && !d.exists()) {
                    created.push(d.to_path_buf());
                    dir = d.parent();
                }
                std::fs::create_dir_all(parent).map_err(|e| {
                    operation_failed(format!(
                        "Cannot create directory: {}: {e}",
                        parent.display()
                    ))
                })?;
                if let Some(mode) = c.chmod_directory {
                    for d in &created {
                        set_mode(d, mode)?;
                    }
                }
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
        if let Some(mode) = c.chmod {
            set_mode(Path::new(path), mode)?;
        }
        if c.force_writes {
            file.sync_all()
                .map_err(|e| operation_failed(format!("Cannot write file: {path}: {e}")))?;
        }
        // the last modified of the message (Java keepLastModified)
        if c.keep_last_modified {
            let modified = exchange
                .message()
                .header(camel_api::exchange_constants::FILE_LAST_MODIFIED)
                .and_then(|v| match v {
                    camel_util::value::Value::Int(i) => Some(*i),
                    camel_util::value::Value::Text(t) => t.parse().ok(),
                    _ => None,
                });
            if let Some(millis) = modified.filter(|m| *m > 0) {
                let time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis as u64);
                file.set_modified(time).map_err(|e| {
                    operation_failed(format!("Cannot set last modified of file: {path}: {e}"))
                })?;
            }
        }
        log::debug!("Wrote [{path}] to [{}]", self.endpoint.uri);
        Ok(bytes)
    }

    /// Moves the existing file away (Java `GenericFileDefaultMoveExistingFileStrategy`).
    fn move_existing(&self, exchange: &mut Exchange, target: &str) -> CamelResult<()> {
        let c = &self.endpoint.config;
        let expr = c.move_existing.clone().ok_or_else(|| {
            CamelError::illegal_argument(
                "You must configure the moveExisting option when fileExist=Move",
            )
        })?;
        // a directory keeps the file name (Java configureMoveOrPreMoveExpression)
        let expr = if !expr.contains("${") || expr.ends_with('/') || expr.ends_with('\\') {
            format!("{}/${{file:onlyname}}", expr.trim_end_matches(['/', '\\']))
        } else {
            expr
        };
        let only = target
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(target)
            .to_string();
        let parent = Path::new(target)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        // the expression sees the existing file: its name, its parent directory
        let saved: Vec<(&str, Option<Value>)> = [FILE_NAME, FILE_NAME_ONLY, FILE_PARENT]
            .iter()
            .map(|k| (*k, exchange.message().header(k).cloned()))
            .collect();
        exchange.message_mut().set_header(FILE_NAME, only.clone());
        exchange
            .message_mut()
            .set_header(FILE_NAME_ONLY, only.clone());
        exchange.message_mut().set_header(FILE_PARENT, parent);
        let name = self.evaluate(exchange, &expr);
        for (k, v) in saved {
            match v {
                Some(v) => exchange.message_mut().set_header(k, v),
                None => {
                    exchange.message_mut().remove_header(k);
                }
            }
        }
        let name = name?.unwrap_or_default();
        if name.is_empty() {
            return Err(operation_failed(format!(
                "moveExisting evaluated as empty String, cannot move existing file: {target}"
            )));
        }
        let mut to = PathBuf::from(&name);
        if !to.is_absolute() {
            to = Path::new(&c.directory).join(to);
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if to.exists() {
            if !c.eager_delete_target_file {
                return Err(operation_failed(format!(
                    "Cannot move existing file from: {target} to: {} as there already exists a file: {}",
                    to.display(),
                    to.display()
                )));
            }
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
        let written = match &temp {
            Some(temp) => {
                if c.file_exist != FileExist::TryRename && Path::new(temp).exists() {
                    let _ = std::fs::remove_file(temp);
                }
                let written = if c.file_exist == FileExist::Append && exists {
                    // append to a copy of the target
                    std::fs::copy(&target, temp).map_err(|e| {
                        operation_failed(format!("Cannot copy file: {target} to {temp}: {e}"))
                    })?;
                    self.write(exchange, temp, true)?
                } else {
                    self.write(exchange, temp, false)?
                };
                if c.file_exist != FileExist::TryRename && Path::new(&target).exists() {
                    std::fs::remove_file(&target).map_err(|e| {
                        operation_failed(format!("Cannot delete file: {target}: {e}"))
                    })?;
                }
                std::fs::rename(temp, &target).map_err(|_| {
                    operation_failed(format!("Cannot rename file from: {temp} to: {target}"))
                })?;
                written
            }
            None => self.write(exchange, &target, c.file_exist == FileExist::Append)?,
        };
        // the checksum of the written content, and its file (Java writeChecksumFile)
        if let Some(algorithm) = &c.checksum_file_algorithm {
            let hash = checksum(algorithm, &written)?;
            if c.checksum_write_file {
                let file = format!("{target}.{algorithm}");
                log::trace!("Writing checksum file: [{file}]");
                std::fs::write(&file, hash.as_bytes()).map_err(|_| {
                    operation_failed(format!("Error writing checksum file [{file}]"))
                })?;
            }
            exchange.message_mut().set_header(FILE_CHECKSUM, hash);
        }
        // an empty done file next to the file (Java writeDoneFile)
        if c.done_file_name.is_some() {
            let done = self.endpoint.create_done_file_name(&target)?;
            log::trace!("Writing done file: [{done}]");
            if Path::new(&done).exists() {
                std::fs::remove_file(&done).map_err(|_| {
                    operation_failed(format!("Cannot delete existing done file: {done}"))
                })?;
            }
            std::fs::write(&done, b"")
                .map_err(|e| operation_failed(format!("Cannot write file: {done}: {e}")))?;
        }
        exchange
            .message_mut()
            .set_header(FILE_NAME_PRODUCED, target);
        Ok(())
    }
}

/// The lower case hex digest of the bytes (Java `MessageDigest` of the algorithm, such as `md5`,
/// `sha1`, `sha256` or `SHA-512`).
fn checksum(algorithm: &str, bytes: &[u8]) -> CamelResult<String> {
    use sha2::Digest;
    let name = algorithm.to_ascii_lowercase().replace(['-', '_'], "");
    let digest: Vec<u8> = match name.as_str() {
        "md5" => md5::Md5::digest(bytes).to_vec(),
        "sha1" | "sha" => sha1::Sha1::digest(bytes).to_vec(),
        "sha224" => sha2::Sha224::digest(bytes).to_vec(),
        "sha256" => sha2::Sha256::digest(bytes).to_vec(),
        "sha384" => sha2::Sha384::digest(bytes).to_vec(),
        "sha512" => sha2::Sha512::digest(bytes).to_vec(),
        "sha512/224" => sha2::Sha512_224::digest(bytes).to_vec(),
        "sha512/256" => sha2::Sha512_256::digest(bytes).to_vec(),
        _ => {
            return Err(CamelError::illegal_argument(format!(
                "{algorithm} MessageDigest not available"
            )))
        }
    };
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// The name of a file written without a name (Java `getGeneratedFileName`): the message id,
/// sanitized.
pub fn generated_file_name(exchange: &Exchange) -> String {
    exchange
        .message()
        .message_id()
        .unwrap_or(exchange.exchange_id())
        .replace([':', '/', '\\'], "-")
}

/// The bytes of the text in the charset (UTF-8 by default, ISO-8859-1/US-ASCII as one byte per
/// character).
fn encode(text: &str, charset: Option<&str>) -> Vec<u8> {
    match charset.and_then(|c| camel_util::charset::Charset::for_name(c).ok()) {
        Some(cs) => cs.encode(text),
        None => text.as_bytes().to_vec(),
    }
}

/// Sets the permissions of the file (unix; other platforms ignore them, as Java without POSIX
/// file attributes).
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> CamelResult<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|e| {
        operation_failed(format!(
            "Cannot set permissions of file: {}: {e}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> CamelResult<()> {
    Ok(())
}

impl Service for FileProducer {}

impl Processor for FileProducer {
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        // the file name header is kept, the overrule used once (Java doProcess)
        let existing = exchange.message().header(FILE_NAME).cloned();
        let result = self.process_exchange(exchange);
        let m = exchange.message_mut();
        m.remove_header(OVERRULE_FILE_NAME);
        match existing {
            Some(v) => m.set_header(FILE_NAME, v),
            None => {
                m.remove_header(FILE_NAME);
            }
        }
        Box::pin(std::future::ready(result))
    }
}

impl Producer for FileProducer {
    fn endpoint(&self) -> SharedEndpoint {
        self.endpoint.clone()
    }
}
