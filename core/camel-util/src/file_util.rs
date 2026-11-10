//! File utilities. Port of `org.apache.camel.util.FileUtil`.
//!
//! The path helpers work on `&str` paths with both `/` and `\` as separators (Camel endpoint
//! URIs carry paths as text); the file system helpers take [`Path`]s.

use crate::system_properties;
use std::fs;
use std::io;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The default buffer size used for file IO.
pub const BUFFER_SIZE: usize = 128 * 1024;

const RETRY_SLEEP_MILLIS: u64 = 10;

/// Returns the user (working) directory (the Java `user.dir` system property).
pub fn get_user_dir() -> PathBuf {
    system_properties::get_property("user.dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Returns true, if the OS is Windows.
pub fn is_windows() -> bool {
    cfg!(windows)
}

/// Normalizes the path to cater for Windows and other platforms: on Windows `/` is converted
/// to `\`, on other systems `\` is converted to `/`.
pub fn normalize_path(path: &str) -> String {
    if is_windows() {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

fn is_separator_prefix(name: &str) -> bool {
    name.starts_with('/') || name.starts_with(MAIN_SEPARATOR)
}

/// Strips any leading separators.
pub fn strip_leading_separator(name: &str) -> &str {
    name.trim_start_matches(['/', MAIN_SEPARATOR])
}

/// Does the name start with a leading separator.
pub fn has_leading_separator(name: &str) -> bool {
    is_separator_prefix(name)
}

/// Strips the first leading separator.
pub fn strip_first_leading_separator(name: &str) -> &str {
    if is_separator_prefix(name) {
        &name[1..]
    } else {
        name
    }
}

/// Strips any trailing separators.
///
/// If the name consists only of separators it is returned as is (there must be some leading
/// text, as only trailing separators are removed).
pub fn strip_trailing_separator(name: &str) -> &str {
    let s = name.trim_end_matches(['/', MAIN_SEPARATOR]);
    if s.is_empty() {
        name
    } else {
        s
    }
}

fn last_separator(name: &str) -> Option<usize> {
    name.rfind(['/', '\\'])
}

/// Strips any leading paths (supports both `/` and `\` separators).
pub fn strip_path(name: &str) -> &str {
    match last_separator(name) {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

/// Strips the extension (everything from the first dot) of the file name.
///
/// A leading dot is a hidden file (such as `.bashrc`), not an extension.
pub fn strip_ext(name: &str) -> &str {
    strip_ext_mode(name, false)
}

/// Strips the extension of the file name.
///
/// # Arguments
///
/// * `name` - the file name, which may have a leading path
/// * `single_mode` - whether to only strip the last extension (`foo.tar.gz` -> `foo.tar`),
///   otherwise everything from the first dot is stripped (`foo`)
pub fn strip_ext_mode(name: &str, single_mode: bool) -> &str {
    let (base, only_name) = match last_separator(name) {
        Some(pos) => (pos + 1, &name[pos + 1..]),
        None => (0, name),
    };
    let pos2 = if single_mode {
        only_name.rfind('.')
    } else {
        only_name.get(1..).and_then(|s| s.find('.')).map(|p| p + 1)
    };
    match pos2 {
        Some(p) if p > 0 => &name[..base + p],
        _ => name,
    }
}

/// Returns only the extension of the file name (`tar.gz` for `bigfile.tar.gz`).
///
/// # Returns
///
/// The extension, or `None` if there is no extension.
pub fn only_ext(name: &str) -> Option<&str> {
    only_ext_mode(name, false)
}

/// Returns only the extension of the file name.
///
/// # Arguments
///
/// * `name` - the file name, which may have a leading path
/// * `single_mode` - whether to only return the last extension (`gz` for `bigfile.tar.gz`)
pub fn only_ext_mode(name: &str, single_mode: bool) -> Option<&str> {
    let name = strip_path(name);
    // a leading dot is a hidden file (such as .bashrc), not an extension
    let pos = if single_mode {
        name.rfind('.')
    } else {
        name.get(1..).and_then(|s| s.find('.')).map(|p| p + 1)
    };
    match pos {
        Some(p) if p > 0 => Some(&name[p + 1..]),
        _ => None,
    }
}

/// Returns only the leading path.
///
/// # Returns
///
/// The path, the root separator for a file in the root, or `None` if there is no path.
pub fn only_path(name: &str) -> Option<&str> {
    match last_separator(name) {
        Some(0) => Some(&name[..1]),
        Some(pos) => Some(&name[..pos]),
        None => None,
    }
}

/// Returns only the name of the file, without path and extension.
pub fn only_name(name: &str) -> &str {
    strip_ext_mode(strip_path(name), false)
}

/// Returns only the name of the file, without path and extension.
///
/// # Arguments
///
/// * `name` - the file name
/// * `single_mode` - whether to only strip the last extension
pub fn only_name_mode(name: &str, single_mode: bool) -> &str {
    strip_ext_mode(strip_path(name), single_mode)
}

/// Compacts a path by stacking it and reducing `..`, and uses OS specific file separators.
pub fn compact_path(path: &str) -> String {
    compact_path_with(path, std::path::MAIN_SEPARATOR_STR)
}

/// Compacts a path by stacking it and reducing `..`, and uses the given separator.
///
/// `http:` and `https:` URLs are returned as-is, and a `file:`, `classpath:` or `http:` scheme
/// is preserved. Leading and trailing separators are preserved (on Windows up to two leading
/// separators, for UNC paths).
///
/// # Examples
///
/// ```
/// use camel_util::file_util::compact_path_with;
/// assert_eq!("../baz", compact_path_with("foo/bar/../../../baz", "/"));
/// assert_eq!("file:foo/baz", compact_path_with("file:foo/bar/../baz", "/"));
/// assert_eq!("E:\\workspace\\target", compact_path_with("E:\\workspace\\.\\target", "\\"));
/// ```
pub fn compact_path_with(path: &str, separator: &str) -> String {
    if path.starts_with("http:") || path.starts_with("https:") {
        return path.to_string();
    }
    // only normalize if contains a path separator
    if !path.contains(['/', '\\']) {
        return path.to_string();
    }
    let path = normalize_path(path);
    // preserve scheme
    let (scheme, path) = if has_scheme(&path) {
        let pos = path.find(':').unwrap();
        (Some(&path[..pos]), &path[pos + 1..])
    } else {
        (None, path.as_str())
    };
    let ends_with_slash = path.ends_with(['/', '\\']);
    let mut cnt_slashes_at_start = 0;
    if path.starts_with(['/', '\\']) {
        cnt_slashes_at_start += 1;
        // for Windows, preserve up to 2 starting slashes, which is necessary for UNC paths.
        if is_windows() && path[1..].starts_with(['/', '\\']) {
            cnt_slashes_at_start += 1;
        }
    }
    let mut stack: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        let top = stack.last().copied();
        if part == ".."
            && top.is_some()
            && top != Some("..")
            && !(stack.len() == 1 && is_windows_drive(top.unwrap()))
        {
            // only pop if there is a previous path, which is not a ".." path either
            stack.pop();
        } else if part == ".." && stack.len() == 1 && is_windows_drive(top.unwrap()) {
            // cannot go above the root of a windows drive (such as C:)
        } else if part != "." && !part.is_empty() {
            stack.push(part);
        }
    }
    let mut sb = String::with_capacity(path.len());
    if let Some(s) = scheme {
        sb.push_str(s);
        sb.push(':');
    }
    sb.push_str(&separator.repeat(cnt_slashes_at_start));
    sb.push_str(&stack.join(separator));
    if ends_with_slash && !stack.is_empty() {
        sb.push_str(separator);
    }
    sb
}

fn is_windows_drive(part: &str) -> bool {
    let c: Vec<char> = part.chars().collect();
    c.len() == 2 && c[1] == ':' && c[0].is_alphabetic()
}

fn has_scheme(uri: &str) -> bool {
    uri.starts_with("file:") || uri.starts_with("classpath:") || uri.starts_with("http:")
}

/// Creates a new temporary file in the parent directory (which is created if needed).
///
/// # Arguments
///
/// * `prefix` - the file name prefix (default `camel`; a prefix shorter than 3 characters gets
///   `camel` appended)
/// * `suffix` - the file name suffix (default `.tmp`)
/// * `parent_dir` - the directory to create the file in
///
/// # Errors
///
/// Returns an error if the directory or file cannot be created.
pub fn create_temp_file(
    prefix: Option<&str>,
    suffix: Option<&str>,
    parent_dir: &Path,
) -> io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = suffix.unwrap_or(".tmp");
    let prefix = match prefix {
        None => "camel".to_string(),
        Some(p) if p.chars().count() < 3 => format!("{p}camel"),
        Some(p) => p.to_string(),
    };
    fs::create_dir_all(parent_dir)?;
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "{prefix}{}{}{n}{suffix}",
            std::process::id(),
            nanos % 1_000_000_000
        );
        let file = parent_dir.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            Ok(_) => return Ok(file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Removes the directory and everything in it (retrying a failed delete once).
pub fn remove_dir(d: &Path) {
    if let Ok(entries) = fs::read_dir(d) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                remove_dir(&p);
            } else {
                delete(&p, false);
            }
        }
    }
    delete(d, true);
}

fn delete(p: &Path, dir: bool) {
    let rm = |p: &Path| {
        if dir {
            fs::remove_dir(p)
        } else {
            fs::remove_file(p)
        }
    };
    if rm(p).is_err() {
        sleep(Duration::from_millis(RETRY_SLEEP_MILLIS));
        let _ = rm(p);
    }
}

/// Renames a file.
///
/// Some OS such as Windows can have problems doing rename IO operations, so the rename is
/// retried up to three times. If all attempts fail and `copy_and_delete_on_rename_fail` is set,
/// the file is copied and then deleted (for example to move files between file systems).
///
/// # Arguments
///
/// * `from` - the from file
/// * `to` - the to file
/// * `copy_and_delete_on_rename_fail` - whether to fallback and do copy and delete, if rename
///   fails
///
/// # Returns
///
/// `true` if the file was renamed, otherwise `false` (also when `from` does not exist).
///
/// # Errors
///
/// Returns an error if the copy/delete fallback fails.
pub fn rename_file(
    from: &Path,
    to: &Path,
    copy_and_delete_on_rename_fail: bool,
) -> io::Result<bool> {
    if !from.exists() {
        return Ok(false);
    }
    let mut renamed = false;
    let mut count = 0;
    while !renamed && count < 3 {
        renamed = fs::rename(from, to).is_ok();
        if !renamed && count > 0 {
            sleep(Duration::from_millis(1000));
        }
        count += 1;
    }
    if !renamed && copy_and_delete_on_rename_fail {
        renamed = rename_file_using_copy(from, to)?;
    }
    Ok(renamed)
}

/// Renames the file using a copy and delete strategy. This is primarily used in environments
/// where the regular rename operation is unreliable.
///
/// # Returns
///
/// `true` if the file was renamed successfully, `false` if `from` does not exist.
///
/// # Errors
///
/// Returns an error if the copy fails or the source cannot be deleted after the copy.
pub fn rename_file_using_copy(from: &Path, to: &Path) -> io::Result<bool> {
    if !from.exists() {
        return Ok(false);
    }
    copy_file(from, to)?;
    if !delete_file(from) {
        return Err(io::Error::other(format!(
            "Renaming file from '{}' to '{}' failed: Cannot delete file '{}' after copy succeeded",
            from.display(),
            to.display(),
            from.display()
        )));
    }
    Ok(true)
}

/// Copies the file (replacing an existing target).
///
/// # Errors
///
/// Returns an error if an IO error occurs during the copy.
pub fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to).map(|_| ())
}

/// Deletes the file.
///
/// This implementation attempts to delete the file up to three times with one second delay,
/// which can mitigate problems on deleting files on some platforms such as Windows.
///
/// # Returns
///
/// `true` if the file was deleted, `false` if it did not exist or could not be deleted.
pub fn delete_file(file: &Path) -> bool {
    if !file.exists() {
        return false;
    }
    let mut count = 0;
    while count < 3 {
        if fs::remove_file(file).is_ok() {
            return true;
        }
        if count > 0 {
            sleep(Duration::from_millis(1000));
        }
        count += 1;
    }
    false
}

/// Is the given file an absolute file.
///
/// On Windows a path starting with `\` is also considered absolute, which makes the logic
/// consistent across all OS platforms.
pub fn is_absolute(file: &Path) -> bool {
    if is_windows() && file.to_string_lossy().starts_with(MAIN_SEPARATOR) {
        return true;
    }
    file.is_absolute()
}

/// Creates a new (empty) file.
///
/// # Returns
///
/// `true` if a new file was created, `false` if it already existed.
///
/// # Errors
///
/// Returns an error if the file cannot be created.
pub fn create_new_file(file: &Path) -> io::Result<bool> {
    if file.exists() {
        return Ok(false);
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file)
    {
        Ok(_) => Ok(true),
        // the file may have been created but a permission error raised afterwards (some NAS)
        Err(_) if file.exists() => Ok(true),
        Err(e) => Err(e),
    }
}

/// Sets posix file permissions.
///
/// # Arguments
///
/// * `path` - the file
/// * `permissions` - permissions such as `rwxr-xr-x`
///
/// # Returns
///
/// `true` if the permissions were set, or `false` if the file-system does not support posix
/// permissions (such as Windows).
///
/// # Errors
///
/// Returns an error if the permission text is invalid or the permissions cannot be set.
pub fn set_posix_file_permissions(path: &Path, permissions: &str) -> io::Result<bool> {
    let chars: Vec<char> = permissions.chars().collect();
    if chars.len() != 9 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid mode: {permissions}"),
        ));
    }
    let mut mode = 0u32;
    for (i, c) in chars.iter().enumerate() {
        let expected = ['r', 'w', 'x'][i % 3];
        match *c {
            '-' => {}
            ch if ch == expected => mode |= 1 << (8 - i),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid mode: {permissions}"),
                ))
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("camel-util-tests")
            .join("file-util")
            .join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_normalize_path() {
        if is_windows() {
            assert_eq!("foo\\bar", normalize_path("foo/bar"));
            assert_eq!("foo\\bar\\baz", normalize_path("foo/bar\\baz"));
        } else {
            assert_eq!("foo/bar", normalize_path("foo/bar"));
            assert_eq!("foo/bar/baz", normalize_path("foo/bar\\baz"));
            assert_eq!(
                "movefile/sub/sub2/.done/goodday.txt",
                normalize_path("movefile/sub/sub2\\.done\\goodday.txt")
            );
        }
    }

    #[test]
    fn test_strip_leading_separator() {
        assert_eq!("foo", strip_leading_separator("foo"));
        assert_eq!("foo/bar", strip_leading_separator("foo/bar"));
        assert_eq!("foo/", strip_leading_separator("foo/"));
        assert_eq!("foo/bar", strip_leading_separator("/foo/bar"));
        assert_eq!("foo/bar", strip_leading_separator("//foo/bar"));
        assert_eq!("foo/bar", strip_leading_separator("///foo/bar"));
    }

    #[test]
    fn test_has_leading_separator() {
        assert!(!has_leading_separator("foo"));
        assert!(!has_leading_separator("foo/bar"));
        assert!(!has_leading_separator("foo/"));
        assert!(has_leading_separator("/foo/bar"));
        assert!(has_leading_separator("//foo/bar"));
        assert!(has_leading_separator("///foo/bar"));
    }

    #[test]
    fn test_strip_first_leading_separator() {
        assert_eq!("foo", strip_first_leading_separator("foo"));
        assert_eq!("foo/bar", strip_first_leading_separator("foo/bar"));
        assert_eq!("foo/", strip_first_leading_separator("foo/"));
        assert_eq!("foo/bar", strip_first_leading_separator("/foo/bar"));
        assert_eq!("/foo/bar", strip_first_leading_separator("//foo/bar"));
        assert_eq!("//foo/bar", strip_first_leading_separator("///foo/bar"));
    }

    #[test]
    fn test_strip_trailing_separator() {
        let cases = [
            ("foo", "foo"),
            ("foo/bar", "foo/bar"),
            ("foo/", "foo"),
            ("foo/bar/", "foo/bar"),
            ("/foo/bar", "/foo/bar"),
            ("/foo/bar/", "/foo/bar"),
            ("/foo/bar//", "/foo/bar"),
            ("/foo/bar///", "/foo/bar"),
            ("/foo", "/foo"),
            ("/foo/", "/foo"),
            ("/", "/"),
            ("//", "//"),
        ];
        for (input, expected) in cases {
            assert_eq!(expected, strip_trailing_separator(input), "{input}");
        }
    }

    #[test]
    fn test_strip_path() {
        assert_eq!("", strip_path("/"));
        assert_eq!("foo.xml", strip_path("/foo.xml"));
        assert_eq!("foo", strip_path("foo"));
        assert_eq!("bar", strip_path("foo/bar"));
        assert_eq!("bar", strip_path("/foo/bar"));
    }

    #[test]
    fn test_strip_path_with_mixed_separators() {
        assert_eq!("baz", strip_path("foo/bar\\baz"));
        assert_eq!("bar", strip_path("\\foo\\bar"));
        assert_eq!("baz", strip_path("/foo\\bar/baz"));
    }

    #[test]
    fn test_strip_ext() {
        assert_eq!("foo", strip_ext("foo"));
        assert_eq!("foo", strip_ext("foo.xml"));
        assert_eq!("/foo/bar", strip_ext("/foo/bar.xml"));
        assert_eq!(".bashrc", strip_ext(".bashrc"));
        assert_eq!("/home/.bashrc", strip_ext("/home/.bashrc"));
        assert_eq!(".route", strip_ext(".route.yaml"));
        assert_eq!("/home/.route", strip_ext("/home/.route.yaml"));
        assert_eq!("foo.tar", strip_ext_mode("foo.tar.gz", true));
        assert_eq!("foo", strip_ext_mode("foo.tar.gz", false));
    }

    #[test]
    fn test_only_ext() {
        assert_eq!(None, only_ext("foo"));
        assert_eq!(Some("xml"), only_ext("foo.xml"));
        assert_eq!(Some("xml"), only_ext("/foo/bar.xml"));
        assert_eq!(Some("tar.gz"), only_ext("/foo/bigfile.tar.gz"));
        assert_eq!(Some("tar.gz"), only_ext("/foo.bar/bigfile.tar.gz"));
        assert_eq!(None, only_ext(".bashrc"));
        assert_eq!(None, only_ext("/home/.bashrc"));
        assert_eq!(None, only_ext_mode(".bashrc", true));
        assert_eq!(Some("yaml"), only_ext(".route.yaml"));
        assert_eq!(Some("yaml"), only_ext_mode(".route.yaml", true));
        assert_eq!(Some("gz"), only_ext_mode("bigfile.tar.gz", true));
    }

    #[test]
    fn test_only_path() {
        assert_eq!(None, only_path("foo"));
        assert_eq!(None, only_path("foo.xml"));
        assert_eq!(Some("foo"), only_path("foo/bar.xml"));
        assert_eq!(Some("/foo"), only_path("/foo/bar.xml"));
        assert_eq!(Some("/foo/bar"), only_path("/foo/bar/baz.xml"));
        assert_eq!(Some("/"), only_path("/foo.xml"));
        assert_eq!(Some("/bar"), only_path("/bar/foo.xml"));
    }

    #[test]
    fn test_only_path_with_mixed_separators() {
        assert_eq!(Some("foo"), only_path("foo/bar.xml"));
        assert_eq!(Some("/foo"), only_path("/foo\\bar.xml"));
        assert_eq!(Some("\\foo\\bar"), only_path("\\foo\\bar/baz.xml"));
        assert_eq!(Some("\\"), only_path("\\foo.xml"));
        assert_eq!(Some("/bar"), only_path("/bar\\foo.xml"));
    }

    #[test]
    fn test_only_name() {
        assert_eq!("foo", only_name("foo"));
        assert_eq!("foo", only_name("foo.xml"));
        assert_eq!("bar", only_name("foo/bar.xml"));
        assert_eq!("bar", only_name("/foo/bar.xml"));
        assert_eq!("baz", only_name("/foo/bar/baz.xml"));
        assert_eq!("foo", only_name("/foo.xml"));
        assert_eq!("foo", only_name("/bar/foo.xml"));
        assert_eq!("foo.tar", only_name_mode("/a/foo.tar.gz", true));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_compact_path() {
        let cases = [
            ("../foo", "../foo"),
            ("../../foo", "../../foo"),
            ("../../foo/bar", "../../foo/bar"),
            ("../../foo/bar/..", "../../foo"),
            ("foo", "foo"),
            ("foo/../bar", "bar"),
            ("foo/../bar/baz", "bar/baz"),
            ("foo/bar/../baz", "foo/baz"),
            ("foo/bar/../../baz", "baz"),
            ("foo/bar/../../../baz", "../baz"),
            ("../foo/bar", "../foo/bar"),
            ("foo/bar/./baz", "foo/bar/baz"),
            ("foo/bar//baz", "foo/bar/baz"),
            ("/foo/bar/baz", "/foo/bar/baz"),
            // do not preserve multiple slashes at the beginning if not on Windows
            ("//foo/bar/baz", "/foo/bar/baz"),
            ("/", "/"),
            ("\\", "/"),
        ];
        for (input, expected) in cases {
            assert_eq!(expected, compact_path(input), "{input}");
        }
        assert_eq!("/", compact_path_with("/", "/"));
        assert_eq!("/", compact_path_with("\\", "/"));
    }

    #[test]
    fn test_compact_windows_style_path() {
        let path = "E:\\workspace\\foo\\bar\\some-thing\\.\\target\\processes\\2";
        let expected = "E:\\workspace\\foo\\bar\\some-thing\\target\\processes\\2";
        assert_eq!(expected, compact_path_with(path, "\\"));
    }

    #[test]
    fn test_compact_path_separator() {
        assert_eq!("..\\foo", compact_path_with("..\\foo", "\\"));
        assert_eq!("../foo", compact_path_with("../foo", "/"));
        assert_eq!("../foo/bar", compact_path_with("../foo\\bar", "/"));
        assert_eq!("..\\foo\\bar", compact_path_with("../foo\\bar", "\\"));
    }

    #[test]
    fn test_default_temp_file_suffix_and_prefix() {
        let tmp = create_temp_file(Some("tmp-"), Some(".tmp"), &test_directory("tmp")).unwrap();
        assert!(tmp.is_file(), "Should be a file");
        let name = tmp.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("tmp-") && name.ends_with(".tmp"));
    }

    #[test]
    fn test_default_temp_file() {
        let tmp = create_temp_file(None, None, &test_directory("tmp")).unwrap();
        assert!(tmp.is_file(), "Should be a file");
        assert!(tmp
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("camel"));
        let short = create_temp_file(Some("ab"), None, &test_directory("tmp")).unwrap();
        assert!(short
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("abcamel"));
    }

    #[test]
    fn test_default_temp_file_parent() {
        let tmp = create_temp_file(None, None, &test_directory("parent")).unwrap();
        assert!(tmp.is_file(), "Should be a file");
    }

    #[test]
    fn test_create_new_file() {
        let file = test_directory("create").join("foo.txt");
        if file.exists() {
            delete_file(&file);
        }
        assert!(!file.exists());
        assert!(
            create_new_file(&file).unwrap(),
            "A new file should be created"
        );
        assert!(!create_new_file(&file).unwrap());
    }

    #[test]
    fn test_rename_using_delete() {
        let dir = test_directory("rename");
        let file = dir.join("foo.txt");
        if !file.exists() {
            create_new_file(&file).unwrap();
        }
        let target = dir.join("bar.txt");
        assert!(rename_file_using_copy(&file, &target).unwrap());
        assert!(target.exists(), "File not copied");
        assert!(!file.exists(), "File not deleted");
        assert!(!rename_file_using_copy(&file, &target).unwrap());
        assert!(rename_file(&target, &file, false).unwrap());
        assert!(file.exists());
        assert!(!rename_file(&target, &file, true).unwrap());
    }

    #[test]
    fn test_compact_http_path() {
        let input = "http://foo.com/apps/func/schemas/part/myap/dummy-schema.xsd";
        assert_eq!(input, compact_path_with(input, "/"));
    }

    #[test]
    fn test_compact_https_path() {
        let input = "https://foo.com/apps/func/schemas/part/myap/dummy-schema.xsd";
        assert_eq!(input, compact_path_with(input, "/"));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_compact_file_path() {
        let cases = [
            ("file:../foo", "file:../foo"),
            ("file:../../foo", "file:../../foo"),
            ("file:../../foo/bar", "file:../../foo/bar"),
            ("file:../../foo/bar/..", "file:../../foo"),
            ("file:foo", "file:foo"),
            ("file:foo/../bar", "file:bar"),
            ("file:foo/../bar/baz", "file:bar/baz"),
            ("file:foo/bar/../baz", "file:foo/baz"),
            ("file:foo/bar/../../baz", "file:baz"),
            ("file:foo/bar/../../../baz", "file:../baz"),
            ("file:../foo/bar", "file:../foo/bar"),
            ("file:foo/bar/./baz", "file:foo/bar/baz"),
            ("file:foo/bar//baz", "file:foo/bar/baz"),
            ("file:/foo/bar/baz", "file:/foo/bar/baz"),
        ];
        for (input, expected) in cases {
            assert_eq!(expected, compact_path(input), "{input}");
        }
    }

    #[test]
    fn test_remove_dir_and_misc() {
        let dir = test_directory("remove");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("a.txt"), "a").unwrap();
        remove_dir(&dir);
        assert!(!dir.exists());
        assert!(is_absolute(Path::new("/foo")) || is_windows());
        assert!(!is_absolute(Path::new("foo")));
        assert!(get_user_dir().exists());
        assert!(!delete_file(&dir.join("nope")));
    }

    #[test]
    #[cfg(unix)]
    fn test_set_posix_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let file = create_temp_file(None, None, &test_directory("perm")).unwrap();
        assert!(set_posix_file_permissions(&file, "rw-r-----").unwrap());
        assert_eq!(
            0o640,
            fs::metadata(&file).unwrap().permissions().mode() & 0o777
        );
        assert!(set_posix_file_permissions(&file, "bad").is_err());
        assert!(set_posix_file_permissions(&file, "rwxrwxrwz").is_err());
    }
}
