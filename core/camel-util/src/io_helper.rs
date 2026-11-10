//! IO helper functions. Port of `org.apache.camel.util.IOHelper`.
//!
//! Java distinguishes byte streams (`InputStream`) from character streams (`Reader`). In Rust
//! all streams are [`Read`]/[`Write`] of bytes and text is UTF-8; the charset aware helpers
//! transcode with [`crate::charset`]. The Java `close(...)` helpers are not needed: Rust closes
//! streams when they are dropped.

use crate::charset::{
    Charset, DecodingReader, EncodingReader, EncodingWriter, UnsupportedCharsetError,
};
use crate::file_util::only_path;
use crate::string_helper::camel_case_to_dash;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

/// The default buffer size used for copying streams.
pub const DEFAULT_BUFFER_SIZE: usize = 16384;

/// The offset to start reading text from.
pub const INITIAL_OFFSET: usize = 0;

const MAX_BUFFER_SIZE: usize = 262_144;

/// The error message when [`copy_with`] exceeds the maximum size.
pub const MAX_SIZE_EXCEEDED: &str =
    "The InputStream entry being copied exceeds the maximum allowed size";

/// Reads the whole reader (UTF-8 text) into a string, skipping the first `offset` characters.
///
/// # Errors
///
/// Returns an error if reading fails or the text is not valid UTF-8.
pub fn to_string(mut reader: impl Read, offset: usize) -> io::Result<String> {
    let mut s = String::new();
    reader.read_to_string(&mut s)?;
    if offset == 0 {
        return Ok(s);
    }
    Ok(s.chars().skip(offset).collect())
}

/// Copies the data from the input to the output and flushes the output.
///
/// # Returns
///
/// The number of bytes copied.
///
/// # Errors
///
/// Returns an error for I/O errors.
pub fn copy(input: &mut impl Read, output: &mut impl Write) -> io::Result<u64> {
    let n = io::copy(input, output)?;
    output.flush()?;
    Ok(n)
}

/// Copies the data from the input to the output, using a buffer of the given size.
///
/// # Arguments
///
/// * `input` - the input
/// * `output` - the output
/// * `buffer_size` - the size of the buffer used for the copies (capped at 256 KB)
/// * `flush_on_each_write` - whether to flush the output every time data is written
/// * `max_size` - the maximum number of bytes allowed to be copied, or `0` for no limit
///
/// # Returns
///
/// The number of bytes copied. (The Java version returned an `int` capped at
/// `Integer.MAX_VALUE`; Rust returns the real count.)
///
/// # Errors
///
/// Returns an error for I/O errors, or [`MAX_SIZE_EXCEEDED`] if more than `max_size` bytes
/// are copied.
pub fn copy_with(
    input: &mut impl Read,
    output: &mut impl Write,
    buffer_size: usize,
    flush_on_each_write: bool,
    max_size: u64,
) -> io::Result<u64> {
    let mut buffer = vec![0u8; buffer_size.clamp(1, MAX_BUFFER_SIZE)];
    let mut total: u64 = 0;
    loop {
        let n = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        output.write_all(&buffer[..n])?;
        if flush_on_each_write {
            output.flush()?;
        }
        total += n as u64;
        if max_size > 0 && total > max_size {
            return Err(io::Error::other(MAX_SIZE_EXCEEDED));
        }
    }
    if !flush_on_each_write {
        output.flush()?;
    }
    Ok(total)
}

/// Copies the data from the input to the output, consuming (and thereby closing) the input.
///
/// # Errors
///
/// Returns an error for I/O errors.
pub fn copy_and_close_input(
    mut input: impl Read,
    output: &mut impl Write,
    buffer_size: usize,
) -> io::Result<u64> {
    copy_with(&mut input, output, buffer_size, false, 0)
}

/// Forces any updates of the file to be written to the storage device.
///
/// # Errors
///
/// Returns an error if the file cannot be synced.
pub fn force(file: &File) -> io::Result<()> {
    file.sync_all()
}

/// Validates that the charset is supported.
///
/// # Errors
///
/// Returns an error with the name if the charset is not supported.
pub fn validate_charset(charset: &str) -> Result<(), UnsupportedCharsetError> {
    Charset::for_name(charset).map(|_| ())
}

/// Loads the entire reader (UTF-8 text) into memory as a string, normalizing line terminators
/// to `\n`.
///
/// # Arguments
///
/// * `input` - the input
/// * `new_line` - whether to append a `\n` line terminator at the end of (non-empty) text
///
/// # Errors
///
/// Returns an error if reading fails.
///
/// Warning, don't use for crazy big streams :)
pub fn load_text(input: impl Read, new_line: bool) -> io::Result<String> {
    let mut builder = String::with_capacity(2048);
    for line in java_lines(input)? {
        if !builder.is_empty() {
            builder.push('\n');
        }
        builder.push_str(&line);
    }
    if !builder.is_empty() && new_line {
        builder.push('\n');
    }
    Ok(builder)
}

/// Loads the given (1-based) line number of the reader.
///
/// # Returns
///
/// The line, or `None` if there are fewer lines.
///
/// # Errors
///
/// Returns an error if reading fails.
pub fn load_text_line(input: impl Read, line_number: usize) -> io::Result<Option<String>> {
    Ok(java_lines(input)?.into_iter().nth(line_number.max(1) - 1))
}

/// Splits into lines like Java `BufferedReader.readLine` (`\n`, `\r` or `\r\n`).
fn java_lines(input: impl Read) -> io::Result<Vec<String>> {
    let mut reader = BufReader::new(input);
    let mut text = Vec::new();
    reader.read_to_end(&mut text)?;
    let text = String::from_utf8_lossy(&text);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => lines.push(std::mem::take(&mut current)),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(std::mem::take(&mut current));
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    Ok(lines)
}

/// Appends the text to the file (creating the parent directories if needed).
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn append_text(text: &str, file: &Path) -> io::Result<()> {
    do_write_text(text, file, true)
}

/// Writes the text to the file (creating the parent directories if needed).
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn write_text(text: &str, file: &Path) -> io::Result<()> {
    do_write_text(text, file, false)
}

fn do_write_text(text: &str, file: &Path, append: bool) -> io::Result<()> {
    if !file.exists() {
        if let Some(path) = only_path(&file.to_string_lossy()) {
            fs::create_dir_all(path)?;
        }
    }
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(file)?;
    f.write_all(text.as_bytes())
}

/// Writes the text to the output.
///
/// # Errors
///
/// Returns an error if writing fails.
pub fn write_text_to(text: &str, mut output: impl Write) -> io::Result<()> {
    output.write_all(text.as_bytes())?;
    output.flush()
}

/// Gets the charset name from the content type string, such as `text/plain; charset=UTF-8`.
///
/// # Arguments
///
/// * `content_type` - the content type
///
/// # Returns
///
/// The charset name, or `UTF-8` if not found (or the charset parameter is empty).
///
/// # Examples
///
/// ```
/// use camel_util::io_helper::get_charset_name_from_content_type;
/// assert_eq!("iso-8859-1", get_charset_name_from_content_type("text/html; charset=iso-8859-1"));
/// assert_eq!("UTF-8", get_charset_name_from_content_type("text/plain; charset=utf-8"));
/// assert_eq!("UTF-8", get_charset_name_from_content_type("text/html"));
/// ```
pub fn get_charset_name_from_content_type(content_type: &str) -> String {
    // try optimized for direct match without using splitting
    if let Some(pos) = content_type.find("charset=") {
        // the parameter must be named charset (and not such as mycharset)
        let prev = content_type[..pos].chars().last();
        if pos == 0 || prev == Some(';') || prev.map(char::is_whitespace).unwrap_or(false) {
            let value_start = pos + 8;
            let rest = &content_type[value_start..];
            // special optimization for utf-8 which is a common charset
            if rest.len() >= 5 && rest[..5].eq_ignore_ascii_case("utf-8") {
                let next = rest[5..].chars().next();
                if next.is_none()
                    || next == Some(';')
                    || next.map(char::is_whitespace).unwrap_or(false)
                {
                    return "UTF-8".to_string();
                }
            }
            let charset = match content_type[pos..].find(';') {
                Some(end) => &content_type[value_start..pos + end],
                None => rest,
            };
            let charset = normalize_charset(charset);
            return if charset.is_empty() {
                "UTF-8".to_string()
            } else {
                charset
            };
        }
    }
    for value in content_type.split(';') {
        let value = crate::lang::java_trim(value);
        if value.len() >= 8 && value[..8].eq_ignore_ascii_case("charset=") {
            let charset = normalize_charset(&value[8..]);
            return if charset.is_empty() {
                "UTF-8".to_string()
            } else {
                charset
            };
        }
    }
    "UTF-8".to_string()
}

/// Takes off the quotes and double quotes of the charset (and the surrounding whitespace).
///
/// # Examples
///
/// ```
/// use camel_util::io_helper::normalize_charset;
/// assert_eq!("UTF-8", normalize_charset("\"UTF-8 \""));
/// assert_eq!("UTF-8", normalize_charset("' UTF-8'"));
/// ```
pub fn normalize_charset(charset: &str) -> String {
    let mut trim = false;
    let mut answer = crate::lang::java_trim(charset);
    if answer.starts_with(['\'', '"']) {
        answer = &answer[1..];
        trim = true;
    }
    if answer.ends_with(['\'', '"']) {
        answer = &answer[..answer.len() - 1];
        trim = true;
    }
    if trim {
        crate::lang::java_trim(answer).to_string()
    } else {
        answer.to_string()
    }
}

/// Looks up the OS environment variable in a safe manner by using upper case keys and
/// underscore instead of dash.
///
/// At first a lookup attempt is made without considering camelCase keys. The second lookup
/// converts camelCase to underscores. For example given an environment variable in either
/// format:
///
/// * `CAMEL_KAMELET_AWS_S3_SOURCE_BUCKETNAMEORARN=myArn`
/// * `CAMEL_KAMELET_AWS_S3_SOURCE_BUCKET_NAME_OR_ARN=myArn`
///
/// then the following keys can look up both formats: `camel.kamelet.awsS3Source.bucketNameOrArn`,
/// `camel.kamelet.aws-s3-source.bucketNameOrArn` and
/// `camel.kamelet.aws-s3-source.bucket-name-or-arn`.
///
/// # Returns
///
/// The value, or `None` if no such environment variable exists.
///
/// # Examples
///
/// ```
/// use camel_util::io_helper::lookup_environment_variable;
/// // PATH is always set
/// assert!(lookup_environment_variable("path").is_some());
/// ```
pub fn lookup_environment_variable(key: &str) -> Option<String> {
    let upper_key = key.to_uppercase();
    std::env::var(&upper_key)
        .ok()
        .or_else(|| std::env::var(normalize_environment_variable(&upper_key)).ok())
        // camelCase keys should use underscore as separator
        .or_else(|| std::env::var(normalize_environment_variable(&camel_case_to_dash(key))).ok())
}

/// Converts the given key into an OS environment variable name: upper case (locale
/// independent), with dashes and dots converted to underscores.
///
/// # Examples
///
/// ```
/// use camel_util::io_helper::normalize_environment_variable;
/// assert_eq!("CAMEL_MAIN_NAME", normalize_environment_variable("camel.main-name"));
/// ```
pub fn normalize_environment_variable(key: &str) -> String {
    key.to_uppercase().replace(['-', '.'], "_")
}

/// Opens the file with the given charset as a reader producing UTF-8 text (the Java
/// `toReader` / `EncodingFileReader`). `None` reads the file as UTF-8.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or the charset is not supported.
pub fn to_reader(
    file: &Path,
    charset: Option<&str>,
) -> io::Result<BufReader<DecodingReader<File>>> {
    let cs = resolve_charset(charset)?;
    Ok(BufReader::new(DecodingReader::new(File::open(file)?, cs)))
}

/// Opens the file read with the given charset as an input producing the bytes in the default
/// charset (UTF-8) (the Java `toInputStream` / `EncodingInputStream`).
///
/// # Errors
///
/// Returns an error if the file cannot be opened or the charset is not supported.
pub fn to_input_stream(file: &Path, charset: Option<&str>) -> io::Result<Box<dyn Read + Send>> {
    match charset {
        None => Ok(Box::new(BufReader::new(File::open(file)?))),
        Some(_) => {
            let reader = to_reader(file, charset)?;
            Ok(Box::new(EncodingReader::new(
                reader,
                Charset::default_charset(),
            )))
        }
    }
}

/// Wraps the output so UTF-8 text written to it is encoded in the given charset (the Java
/// `toWriter` / `EncodingFileWriter`).
///
/// # Errors
///
/// Returns an error if the charset is not supported.
pub fn to_writer<W: Write>(
    output: W,
    charset: Option<&str>,
) -> io::Result<BufWriter<EncodingWriter<W>>> {
    let cs = resolve_charset(charset)?;
    Ok(BufWriter::new(EncodingWriter::new(output, cs)))
}

fn resolve_charset(charset: Option<&str>) -> io::Result<Charset> {
    match charset {
        None => Ok(Charset::default_charset()),
        Some(name) => {
            Charset::for_name(name).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
        }
    }
}

/// Reads the file, strips the lines starting with `comment_prefix` and optionally also the
/// blank lines. Line terminators are normalized to `\n`.
///
/// # Arguments
///
/// * `path` - the path of the file to read
/// * `comment_prefix` - the leading character sequence of comment lines
/// * `strip_blank_lines` - whether lines that are blank are removed
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn strip_line_comments(
    path: &Path,
    comment_prefix: &str,
    strip_blank_lines: bool,
) -> io::Result<String> {
    let reader = BufReader::new(File::open(path)?);
    let mut result = String::with_capacity(2048);
    for line in reader.lines() {
        let line = line?;
        let line = line.strip_suffix('\r').unwrap_or(&line);
        if strip_blank_lines && line.trim().is_empty() {
            continue;
        }
        if line.starts_with(comment_prefix) {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIGABYTE: u64 = 1024 * 1024 * 1024;

    /// Returns the given number of zero bytes without holding them in memory.
    struct ZeroInputStream(u64);

    impl Read for ZeroInputStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Ok(0);
            }
            let n = (buf.len() as u64).min(self.0) as usize;
            buf[..n].fill(0);
            self.0 -= n as u64;
            Ok(n)
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("camel-util-tests")
            .join("io-helper")
            .join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- camel-util IOHelperTest ---

    #[test]
    fn test_lookup_environment_variable() {
        // the Java build set these for the test JVM
        std::env::set_var("FOO_SERVICE_PORT", "8081");
        std::env::set_var("CAMEL_KAMELET_AWS_S3_SOURCE_BUCKET_NAME_OR_ARN", "mys3arn");
        assert_eq!(
            Some("8081".into()),
            lookup_environment_variable("FOO_SERVICE_PORT")
        );
        assert_eq!(
            Some("8081".into()),
            lookup_environment_variable("foo-service.port")
        );
        assert_eq!(
            Some("8081".into()),
            lookup_environment_variable("foo-service-port")
        );
        assert_eq!(
            Some("8081".into()),
            lookup_environment_variable("foo.service.port")
        );
        assert_eq!(
            Some("mys3arn".into()),
            lookup_environment_variable("camel.kamelet.aws-s3-source.bucketNameOrArn")
        );
        assert_eq!(
            Some("mys3arn".into()),
            lookup_environment_variable("camel.kamelet.aws-s3-source.bucket-name-or-arn")
        );
        assert_eq!(
            Some("mys3arn".into()),
            lookup_environment_variable("camel.kamelet.awsS3Source.bucketNameOrArn")
        );
        assert_eq!(
            None,
            lookup_environment_variable("camel.util.io-helper.not.there")
        );
    }

    #[test]
    fn test_charset() {
        assert_eq!("UTF-8", get_charset_name_from_content_type("charset=utf-8"));
        assert_eq!("UTF-8", get_charset_name_from_content_type("charset=UTF-8"));
        assert_eq!(
            "UTF-8",
            get_charset_name_from_content_type("text/plain; charset=UTF-8")
        );
        assert_eq!(
            "UTF-8",
            get_charset_name_from_content_type("application/json; charset=utf-8")
        );
        assert_eq!(
            "iso-8859-1",
            get_charset_name_from_content_type("application/json; charset=iso-8859-1")
        );
    }

    #[test]
    fn test_charset_empty() {
        for ct in [
            "text/plain; charset=",
            "text/plain; charset=\"\"",
            "text/plain; charset= ",
            "text/plain; charset=; format=flowed",
            "text/plain; Charset=",
        ] {
            assert_eq!("UTF-8", get_charset_name_from_content_type(ct), "{ct}");
        }
    }

    #[test]
    fn test_copy_max_size() {
        let data = [0u8; 100];
        let mut bos = Vec::new();
        assert_eq!(
            100,
            copy_with(&mut &data[..], &mut bos, DEFAULT_BUFFER_SIZE, false, 100).unwrap()
        );
        assert_eq!(100, bos.len());
        let e = copy_with(
            &mut &data[..],
            &mut Vec::new(),
            DEFAULT_BUFFER_SIZE,
            false,
            99,
        )
        .unwrap_err();
        assert_eq!(MAX_SIZE_EXCEEDED, e.to_string());
    }

    #[test]
    fn test_copy_max_size_of_two_gigabytes_or_more() {
        // the count of copied bytes must not wrap around once more than i32::MAX bytes are copied
        for max_size in [
            i32::MAX as u64 - 100,
            i32::MAX as u64,
            2 * GIGABYTE,
            5 * GIGABYTE,
        ] {
            let mut is = ZeroInputStream(max_size + GIGABYTE);
            let e = copy_with(
                &mut is,
                &mut io::sink(),
                DEFAULT_BUFFER_SIZE,
                false,
                max_size,
            )
            .unwrap_err();
            assert_eq!(
                MAX_SIZE_EXCEEDED,
                e.to_string(),
                "maxSize {max_size} was not enforced"
            );
        }
    }

    #[test]
    fn test_copy_more_than_two_gigabytes() {
        let mut is = ZeroInputStream(3 * GIGABYTE);
        assert_eq!(
            3 * GIGABYTE,
            copy_with(&mut is, &mut io::sink(), DEFAULT_BUFFER_SIZE, false, 0).unwrap()
        );
    }

    #[test]
    fn test_normalize_environment_variable_turkish_locale() {
        // Rust case mapping is locale independent (the Java test set the tr-TR locale)
        assert_eq!("FILE_INDEX", normalize_environment_variable("file.index"));
    }

    // --- camel-core IOHelperTest ---

    #[test]
    fn test_copy_and_close_input() {
        let mut os = Vec::new();
        copy_and_close_input(&b"Hello"[..], &mut os, 256).unwrap();
        assert_eq!(b"Hello".to_vec(), os);
        let mut os2 = Vec::new();
        assert_eq!(5, copy(&mut &b"Hello"[..], &mut os2).unwrap());
    }

    #[test]
    fn test_charset_normalize() {
        assert_eq!("UTF-8", normalize_charset("'UTF-8'"));
        assert_eq!("UTF-8", normalize_charset("\"UTF-8\""));
        assert_eq!("UTF-8", normalize_charset("\"UTF-8 \""));
        assert_eq!("UTF-8", normalize_charset("' UTF-8'"));
    }

    fn assert_read_as_written(name: &str, text: &str, compare: &str) {
        let file = temp_dir("lines").join(name);
        fs::write(&file, text).unwrap();
        assert_eq!(
            compare,
            load_text(File::open(&file).unwrap(), true).unwrap()
        );
    }

    #[test]
    fn test_lines() {
        assert_read_as_written("line1", "line1", "line1\n");
        assert_read_as_written("line1LF", "line1\n", "line1\n");
        assert_read_as_written("line2", "line1\nline2", "line1\nline2\n");
        assert_read_as_written("line2LF", "line1\nline2\n", "line1\nline2\n");
        assert_eq!("a\nb", load_text(&b"a\r\nb\r"[..], false).unwrap());
        assert_eq!(
            Some("b".to_string()),
            load_text_line(&b"a\nb\nc"[..], 2).unwrap()
        );
        assert_eq!(None, load_text_line(&b"a"[..], 5).unwrap());
    }

    #[test]
    fn test_get_charset_name_from_content_type() {
        assert_eq!(
            "iso-8859-1",
            get_charset_name_from_content_type("text/html; charset=iso-8859-1")
        );
        assert_eq!("UTF-8", get_charset_name_from_content_type("text/html"));
        // mycharset is not the charset parameter
        assert_eq!(
            "UTF-8",
            get_charset_name_from_content_type("text/html;mycharset=x")
        );
    }

    // --- additional coverage ---

    #[test]
    fn test_files_and_encodings() {
        let dir = temp_dir("files");
        let f = dir.join("sub").join("a.txt");
        let _ = fs::remove_file(&f);
        write_text("S\u{f8}ren", &f).unwrap();
        append_text("!", &f).unwrap();
        assert_eq!("S\u{f8}ren!", fs::read_to_string(&f).unwrap());
        assert_eq!("ren!", to_string(File::open(&f).unwrap(), 2).unwrap());

        // write Latin-1 and read it back as UTF-8
        let latin = dir.join("latin.txt");
        {
            let mut w = to_writer(File::create(&latin).unwrap(), Some("ISO-8859-1")).unwrap();
            w.write_all("S\u{f8}ren".as_bytes()).unwrap();
            w.flush().unwrap();
        }
        assert_eq!(
            vec![b'S', 0xF8, b'r', b'e', b'n'],
            fs::read(&latin).unwrap()
        );
        let mut s = String::new();
        to_reader(&latin, Some("ISO-8859-1"))
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!("S\u{f8}ren", s);
        let mut bytes = Vec::new();
        to_input_stream(&latin, Some("ISO-8859-1"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!("S\u{f8}ren".as_bytes(), &bytes[..]);
        assert!(to_reader(&latin, Some("nope")).is_err());
        assert!(validate_charset("nope").is_err());
        assert!(validate_charset("utf-8").is_ok());

        let comments = dir.join("c.txt");
        fs::write(&comments, "# header\nkey=1\r\n\n  \n# x\nkey2=2\n").unwrap();
        assert_eq!(
            "key=1\nkey2=2\n",
            strip_line_comments(&comments, "#", true).unwrap()
        );
        assert_eq!(
            "key=1\n\n  \nkey2=2\n",
            strip_line_comments(&comments, "#", false).unwrap()
        );
        force(&File::open(&comments).unwrap()).unwrap();
        let mut out = Vec::new();
        write_text_to("x", &mut out).unwrap();
        assert_eq!(b"x".to_vec(), out);
    }
}
