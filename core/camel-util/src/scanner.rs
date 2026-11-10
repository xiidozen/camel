//! A streaming tokenizer splitting text by a regular expression delimiter.
//!
//! Port of `org.apache.camel.util.Scanner`, a simplified `java.util.Scanner` used by the
//! tokenize language and the splitter to split large streams without reading them fully into
//! memory.

use regex::Regex;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

const BUFFER_SIZE: usize = 1024;
const CACHE_SIZE: usize = 7;

fn cache_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    static CACHE: OnceLock<Mutex<VecDeque<(String, Regex)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let mut cache = cache.lock().unwrap();
    if let Some((_, re)) = cache.iter().find(|(p, _)| p == pattern) {
        return Ok(re.clone());
    }
    let re = Regex::new(pattern)?;
    if cache.len() >= CACHE_SIZE - 1 {
        cache.pop_front();
    }
    cache.push_back((pattern.to_string(), re.clone()));
    Ok(re)
}

fn check_charset(charset_name: Option<&str>) -> io::Result<()> {
    match charset_name.map(|c| c.to_ascii_lowercase().replace('_', "-")) {
        None => Ok(()),
        Some(c) if c == "utf-8" || c == "utf8" || c == "us-ascii" || c == "ascii" => Ok(()),
        Some(c) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported charset: {c} (the scanner reads UTF-8 text)"),
        )),
    }
}

/// A tokenizer reading text from a source and splitting it by a delimiter pattern (by default
/// whitespace).
///
/// Tokens are produced lazily while reading the source in chunks, and a delimiter crossing a
/// chunk boundary is handled. Consecutive delimiters produce empty tokens, while a trailing
/// delimiter does not.
///
/// The source is read as UTF-8 (the Java version supported any charset; other charsets are
/// converted by the Camel type converters before reaching the scanner).
///
/// # Examples
///
/// ```
/// use camel_util::scanner::Scanner;
/// let tokens: Vec<String> = Scanner::from_str("data1\ndata2\n\ndata3\n", Some("\n")).unwrap().collect();
/// assert_eq!(vec!["data1", "data2", "", "data3"], tokens);
/// let words: Vec<String> = Scanner::from_str("  Hello   Camel ", None).unwrap().collect();
/// assert_eq!(vec!["Hello", "Camel"], words);
/// ```
pub struct Scanner<R: Read> {
    source: Option<R>,
    delim: Regex,
    buf: String,
    pending: Vec<u8>,
    exhausted: bool,
    skipped: bool,
    closed: bool,
    peeked: Option<String>,
    last_error: Option<io::Error>,
}

impl Scanner<io::Cursor<Vec<u8>>> {
    /// Creates a scanner on the text.
    ///
    /// # Arguments
    ///
    /// * `source` - the text
    /// * `pattern` - the delimiter regular expression (`None` for whitespace)
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is not a valid regular expression.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(source: &str, pattern: Option<&str>) -> Result<Self, regex::Error> {
        Scanner::new(io::Cursor::new(source.as_bytes().to_vec()), pattern)
    }
}

impl Scanner<File> {
    /// Creates a scanner reading the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, the charset is not supported or the
    /// pattern is invalid.
    pub fn from_file(
        path: &Path,
        charset_name: Option<&str>,
        pattern: Option<&str>,
    ) -> io::Result<Self> {
        check_charset(charset_name)?;
        let file = File::open(path)?;
        Scanner::new(file, pattern).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
    }
}

impl<R: Read> Scanner<R> {
    /// Creates a scanner reading the source.
    ///
    /// # Arguments
    ///
    /// * `source` - the reader
    /// * `pattern` - the delimiter regular expression (`None` for whitespace)
    ///
    /// # Errors
    ///
    /// Returns an error if the pattern is not a valid regular expression.
    pub fn new(source: R, pattern: Option<&str>) -> Result<Self, regex::Error> {
        let delim = match pattern {
            Some(p) => cache_pattern(p)?,
            None => cache_pattern(r"\s+")?,
        };
        Ok(Scanner {
            source: Some(source),
            delim,
            buf: String::new(),
            pending: Vec::new(),
            exhausted: false,
            skipped: false,
            closed: false,
            peeked: None,
            last_error: None,
        })
    }

    /// Creates a scanner reading the source with the given charset.
    ///
    /// # Errors
    ///
    /// Returns an error if the charset is not supported or the pattern is invalid.
    pub fn with_charset(
        source: R,
        charset_name: Option<&str>,
        pattern: Option<&str>,
    ) -> io::Result<Self> {
        check_charset(charset_name)?;
        Scanner::new(source, pattern).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
    }

    /// The delimiter pattern.
    pub fn get_delim(&self) -> &str {
        self.delim.as_str()
    }

    /// Whether there is another token.
    pub fn has_next(&mut self) -> bool {
        if self.closed {
            return false;
        }
        if self.peeked.is_none() {
            self.peeked = self.read_token();
        }
        self.peeked.is_some()
    }

    /// Closes the scanner (and drops the source).
    ///
    /// # Errors
    ///
    /// Returns the last IO error that happened while reading, if any.
    pub fn close(&mut self) -> io::Result<()> {
        self.closed = true;
        self.source = None;
        self.peeked = None;
        match self.last_error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Reads the next chunk from the source into the buffer.
    fn read_more(&mut self) {
        let Some(source) = self.source.as_mut() else {
            self.exhausted = true;
            return;
        };
        let mut chunk = [0u8; BUFFER_SIZE];
        match source.read(&mut chunk) {
            Ok(0) => {
                self.exhausted = true;
                if !self.pending.is_empty() {
                    let rest = std::mem::take(&mut self.pending);
                    self.buf.push_str(&String::from_utf8_lossy(&rest));
                }
            }
            Ok(n) => {
                self.pending.extend_from_slice(&chunk[..n]);
                self.decode_pending();
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                self.last_error = Some(e);
                self.exhausted = true;
            }
        }
    }

    /// Moves the complete UTF-8 characters of the pending bytes into the buffer.
    fn decode_pending(&mut self) {
        match std::str::from_utf8(&self.pending) {
            Ok(s) => {
                self.buf.push_str(s);
                self.pending.clear();
            }
            Err(e) => {
                let valid = e.valid_up_to();
                self.buf
                    .push_str(std::str::from_utf8(&self.pending[..valid]).unwrap());
                match e.error_len() {
                    // an incomplete character at the end: wait for more bytes
                    None => {
                        self.pending.drain(..valid);
                    }
                    // an invalid sequence: replace it and continue
                    Some(len) => {
                        self.buf.push('\u{fffd}');
                        self.pending.drain(..valid + len);
                        self.decode_pending();
                    }
                }
            }
        }
    }

    fn read_token(&mut self) -> Option<String> {
        loop {
            if !self.skipped {
                // skip a delimiter in front of the token
                if let Some(m) = self.delim.find(&self.buf) {
                    if m.start() == 0 && m.end() > 0 {
                        if m.end() == self.buf.len() && !self.exhausted {
                            // the delimiter may continue in the input that is not read yet
                            self.read_more();
                            continue;
                        }
                        self.buf.drain(..m.end());
                        self.skipped = true;
                    }
                }
            }
            if self.buf.is_empty() {
                if self.exhausted {
                    return None;
                }
                self.read_more();
                continue;
            }
            // find the next (non-empty) delimiter after the token start
            let found = self
                .delim
                .find_iter(&self.buf)
                .find(|m| m.end() > m.start() || m.start() > 0);
            match found {
                Some(m) if m.end() == self.buf.len() && !self.exhausted => {
                    // the delimiter may continue in the input that is not read yet
                    self.read_more();
                }
                Some(m) => {
                    let token: String = self.buf.drain(..m.start()).collect();
                    self.skipped = false;
                    return Some(token);
                }
                None if self.exhausted => {
                    self.skipped = false;
                    return Some(std::mem::take(&mut self.buf));
                }
                None => self.read_more(),
            }
        }
    }
}

impl<R: Read> Iterator for Scanner<R> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        if self.closed {
            return None;
        }
        self.peeked.take().or_else(|| self.read_token())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_string() {
        let mut s = Scanner::from_str("data1\ndata2\ndata3\n", Some("\n")).unwrap();
        assert!(s.has_next());
        assert_eq!(Some("data1".to_string()), s.next());
        assert!(s.has_next());
        assert_eq!(Some("data2".to_string()), s.next());
        assert!(s.has_next());
        assert_eq!(Some("data3".to_string()), s.next());
        assert!(!s.has_next());
        s.close().unwrap();
    }

    #[test]
    fn test_scanner_input_stream() {
        let input = io::Cursor::new(b"data1\ndata2\ndata3\n".to_vec());
        let mut s = Scanner::with_charset(input, Some("UTF-8"), Some("\n")).unwrap();
        for expected in ["data1", "data2", "data3"] {
            assert!(s.has_next());
            assert_eq!(Some(expected.to_string()), s.next());
        }
        assert!(!s.has_next());
    }

    /// A reader returning one small piece per read call (like the Java piped stream test).
    struct Pieces(VecDeque<Vec<u8>>);

    impl Read for Pieces {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front() {
                None => Ok(0),
                Some(p) => {
                    buf[..p.len()].copy_from_slice(&p);
                    Ok(p.len())
                }
            }
        }
    }

    #[test]
    fn test_piped_input_stream() {
        let pieces = Pieces(
            vec![
                b"data1\n".to_vec(),
                b"data2\n".to_vec(),
                b"data3\n".to_vec(),
            ]
            .into(),
        );
        let mut s = Scanner::with_charset(pieces, Some("UTF-8"), Some("\n")).unwrap();
        for expected in ["data1", "data2", "data3"] {
            assert!(s.has_next());
            assert_eq!(Some(expected.to_string()), s.next());
        }
    }

    #[test]
    fn test_delimiter_across_chunks() {
        // the delimiter "--" is split between two reads, and a multi-byte char too
        let pieces = Pieces(
            vec![
                b"a-".to_vec(),
                b"-b\xc3".to_vec(),
                b"\xb8--".to_vec(),
                b"-c".to_vec(),
            ]
            .into(),
        );
        let tokens: Vec<String> = Scanner::new(pieces, Some("--")).unwrap().collect();
        assert_eq!(vec!["a", "b\u{f8}", "-c"], tokens);
        // a growing delimiter (\n+) split between reads is one delimiter
        let pieces = Pieces(vec![b"a\n".to_vec(), b"\n\nb".to_vec()].into());
        let tokens: Vec<String> = Scanner::new(pieces, Some("\n+")).unwrap().collect();
        assert_eq!(vec!["a", "b"], tokens);
    }

    #[test]
    fn test_large_input_and_misc() {
        let text: String = (0..2000).map(|i| format!("line{i}\n")).collect();
        let tokens: Vec<String> = Scanner::from_str(&text, Some("\n")).unwrap().collect();
        assert_eq!(2000, tokens.len());
        assert_eq!("line1999", tokens[1999]);
        let mut s = Scanner::from_str("a b", None).unwrap();
        assert_eq!(r"\s+", s.get_delim());
        s.close().unwrap();
        assert!(!s.has_next());
        assert_eq!(None, s.next());
        assert!(Scanner::with_charset(io::empty(), Some("ISO-8859-1"), None).is_err());
        assert!(Scanner::from_str("x", Some("(")).is_err());
        assert_eq!(0, Scanner::from_str("", None).unwrap().count());
        let dir = std::env::temp_dir()
            .join("camel-util-tests")
            .join("scanner");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("in.txt");
        std::fs::write(&f, "x,y").unwrap();
        let tokens: Vec<String> = Scanner::from_file(&f, None, Some(",")).unwrap().collect();
        assert_eq!(vec!["x", "y"], tokens);
    }
}
