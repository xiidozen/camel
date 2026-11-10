//! Character sets, the Rust counterpart of `java.nio.charset.Charset` as used by Camel.
//!
//! Camel identifies charsets by their Java names (`UTF-8`, `ISO-8859-1`, ...) in headers such
//! as `CamelCharsetName` and in endpoint options. [`Charset::for_name`] accepts the Java
//! canonical names and aliases, and the encode/decode functions follow the Java behaviour:
//! malformed input decodes to U+FFFD and unmappable characters encode to `?`.
//!
//! `ISO-8859-1`, `US-ASCII` and the UTF-16 variants are implemented here, because the WHATWG
//! encoding standard used by `encoding_rs` maps the label `iso-8859-1` to `windows-1252`,
//! which differs from Java. All other charsets are provided by `encoding_rs`.

use encoding_rs::{CoderResult, Decoder, Encoding};
use std::fmt;
use std::io::{self, Read, Write};

/// A character set (see the [module documentation](self)).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Charset {
    /// `UTF-8`
    Utf8,
    /// `ISO-8859-1` (Latin-1: every byte is the code point of the same value)
    Iso8859_1,
    /// `US-ASCII` (7 bit)
    UsAscii,
    /// `UTF-16BE`
    Utf16Be,
    /// `UTF-16LE`
    Utf16Le,
    /// `UTF-16` (decodes with byte order mark detection, big endian by default; encodes big endian
    /// with a byte order mark, as Java does)
    Utf16,
    /// Any other charset supported by `encoding_rs` (such as `windows-1252` or `Shift_JIS`).
    Other(&'static Encoding),
}

/// Error for a charset name that is not supported (Java `UnsupportedCharsetException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCharsetError(pub String);

impl fmt::Display for UnsupportedCharsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UnsupportedCharsetError {}

impl fmt::Debug for Charset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Charset({})", self.name())
    }
}

impl fmt::Display for Charset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Default for Charset {
    fn default() -> Self {
        Charset::default_charset()
    }
}

impl Charset {
    /// The default charset, `UTF-8` (as Java 18+ `Charset.defaultCharset()`).
    pub fn default_charset() -> Charset {
        Charset::Utf8
    }

    /// Returns the charset for the given Java name or alias (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns an error with the name if the charset is not supported.
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_util::charset::Charset;
    /// assert_eq!(Charset::Iso8859_1, Charset::for_name("latin1").unwrap());
    /// assert_eq!("ISO-8859-1", Charset::for_name("iso-8859-1").unwrap().name());
    /// assert_eq!("windows-1252", Charset::for_name("cp1252").unwrap().name());
    /// assert!(Charset::for_name("nope").is_err());
    /// ```
    pub fn for_name(name: &str) -> Result<Charset, UnsupportedCharsetError> {
        let n = name.trim().to_ascii_lowercase().replace('_', "-");
        let cs =
            match n.as_str() {
                "utf-8" | "utf8" | "unicode-1-1-utf-8" => Charset::Utf8,
                "iso-8859-1" | "iso8859-1" | "8859-1" | "latin1" | "l1" | "iso-ir-100"
                | "ibm819" | "cp819" | "csisolatin1" | "819" | "iso-8859-1:1987"
                | "iso8859-1-1987" => Charset::Iso8859_1,
                "us-ascii" | "ascii" | "us" | "iso646-us" | "ascii7" | "646" | "cp367"
                | "ibm367" | "csascii" | "default" | "iso-ir-6" | "ansi-x3.4-1968"
                | "ansi-x3.4-1986" | "iso-646.irv:1991" => Charset::UsAscii,
                "utf-16be" | "utf16be" | "x-utf-16be" | "iso-10646-ucs-2"
                | "unicodebigunmarked" => Charset::Utf16Be,
                "utf-16le" | "utf16le" | "x-utf-16le" | "unicodelittleunmarked" => Charset::Utf16Le,
                "utf-16" | "utf16" | "unicode" | "unicodebig" => Charset::Utf16,
                _ => match Encoding::for_label(n.as_bytes()) {
                    Some(e) if e == encoding_rs::UTF_8 => Charset::Utf8,
                    Some(e) if e == encoding_rs::UTF_16BE => Charset::Utf16Be,
                    Some(e) if e == encoding_rs::UTF_16LE => Charset::Utf16Le,
                    // "replacement" is a WHATWG security device, not a real charset
                    Some(e) if e == encoding_rs::REPLACEMENT => {
                        return Err(UnsupportedCharsetError(name.to_string()))
                    }
                    Some(e) => Charset::Other(e),
                    None => return Err(UnsupportedCharsetError(name.to_string())),
                },
            };
        Ok(cs)
    }

    /// Whether the charset name is supported (Java `Charset.isSupported`).
    pub fn is_supported(name: &str) -> bool {
        Charset::for_name(name).is_ok()
    }

    /// The canonical name of the charset (the Java `Charset.name()`).
    pub fn name(&self) -> &'static str {
        match self {
            Charset::Utf8 => "UTF-8",
            Charset::Iso8859_1 => "ISO-8859-1",
            Charset::UsAscii => "US-ASCII",
            Charset::Utf16Be => "UTF-16BE",
            Charset::Utf16Le => "UTF-16LE",
            Charset::Utf16 => "UTF-16",
            Charset::Other(e) => e.name(),
        }
    }

    /// Decodes the bytes to text; malformed input becomes U+FFFD (Java `new String(bytes,
    /// charset)`).
    pub fn decode(&self, bytes: &[u8]) -> String {
        let mut d = StreamDecoder::new(*self);
        let mut out = String::with_capacity(bytes.len());
        d.decode(bytes, &mut out, true);
        out
    }

    /// Encodes the text to bytes; unmappable characters become `?` (Java
    /// `String.getBytes(charset)`).
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_util::charset::Charset;
    /// assert_eq!(vec![0x53, 0xF8], Charset::Iso8859_1.encode("Sø"));
    /// assert_eq!(b"S?".to_vec(), Charset::UsAscii.encode("Sø"));
    /// assert_eq!(vec![0xFE, 0xFF, 0x00, 0x41], Charset::Utf16.encode("A"));
    /// ```
    pub fn encode(&self, text: &str) -> Vec<u8> {
        let mut e = StreamEncoder::new(*self);
        let mut out = Vec::with_capacity(text.len());
        e.encode(text, &mut out);
        out
    }
}

// ---------------------------------------------------------------------------------------
// streaming

/// An incremental decoder from charset bytes to text.
pub struct StreamDecoder {
    charset: Charset,
    inner: Option<Decoder>,
}

impl StreamDecoder {
    /// Creates a decoder for the charset.
    pub fn new(charset: Charset) -> Self {
        let inner = match charset {
            // like Java, a byte order mark is kept as U+FEFF
            Charset::Utf8 => Some(encoding_rs::UTF_8.new_decoder_without_bom_handling()),
            Charset::Utf16Be => Some(encoding_rs::UTF_16BE.new_decoder_without_bom_handling()),
            Charset::Utf16Le => Some(encoding_rs::UTF_16LE.new_decoder_without_bom_handling()),
            // big endian unless a byte order mark says otherwise
            Charset::Utf16 => Some(encoding_rs::UTF_16BE.new_decoder()),
            Charset::Other(e) => Some(e.new_decoder_without_bom_handling()),
            Charset::Iso8859_1 | Charset::UsAscii => None,
        };
        StreamDecoder { charset, inner }
    }

    /// Decodes the bytes, appending the text to `out`. Incomplete sequences at the end are kept
    /// until the next call, unless `last` is set.
    pub fn decode(&mut self, bytes: &[u8], out: &mut String, last: bool) {
        match (&mut self.inner, self.charset) {
            (None, Charset::Iso8859_1) => out.extend(bytes.iter().map(|&b| b as char)),
            (None, _) => {
                out.extend(
                    bytes
                        .iter()
                        .map(|&b| if b < 0x80 { b as char } else { '\u{fffd}' }),
                )
            }
            (Some(d), _) => {
                let mut src = bytes;
                loop {
                    out.reserve(d.max_utf8_buffer_length(src.len()).unwrap_or(src.len() * 3));
                    let (result, read, _) = d.decode_to_string(src, out, last);
                    src = &src[read..];
                    match result {
                        CoderResult::InputEmpty => break,
                        CoderResult::OutputFull => continue,
                    }
                }
            }
        }
    }
}

/// An incremental encoder from text to charset bytes.
pub struct StreamEncoder {
    charset: Charset,
    bom_written: bool,
}

impl StreamEncoder {
    /// Creates an encoder for the charset.
    pub fn new(charset: Charset) -> Self {
        StreamEncoder {
            charset,
            bom_written: false,
        }
    }

    /// Encodes the text, appending the bytes to `out`.
    pub fn encode(&mut self, text: &str, out: &mut Vec<u8>) {
        match self.charset {
            Charset::Utf8 => out.extend_from_slice(text.as_bytes()),
            Charset::Iso8859_1 => {
                out.extend(
                    text.chars()
                        .map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' }),
                )
            }
            Charset::UsAscii => {
                out.extend(
                    text.chars()
                        .map(|c| if c.is_ascii() { c as u8 } else { b'?' }),
                )
            }
            Charset::Utf16Be | Charset::Utf16 => {
                if self.charset == Charset::Utf16 && !self.bom_written && !text.is_empty() {
                    out.extend_from_slice(&[0xFE, 0xFF]);
                    self.bom_written = true;
                }
                for u in text.encode_utf16() {
                    out.extend_from_slice(&u.to_be_bytes());
                }
            }
            Charset::Utf16Le => {
                for u in text.encode_utf16() {
                    out.extend_from_slice(&u.to_le_bytes());
                }
            }
            Charset::Other(e) => {
                let mut encoder = e.new_encoder();
                let mut src = text;
                loop {
                    let mut buf = vec![
                        0u8;
                        encoder
                            .max_buffer_length_from_utf8_without_replacement(src.len())
                            .unwrap_or(src.len() * 4)
                            + 16
                    ];
                    let (result, read, written) =
                        encoder.encode_from_utf8_without_replacement(src, &mut buf, true);
                    out.extend_from_slice(&buf[..written]);
                    src = &src[read..];
                    match result {
                        encoding_rs::EncoderResult::InputEmpty => break,
                        encoding_rs::EncoderResult::OutputFull => continue,
                        encoding_rs::EncoderResult::Unmappable(_) => out.push(b'?'),
                    }
                }
            }
        }
    }
}

/// A reader that decodes bytes in a charset from the inner reader and produces UTF-8 bytes
/// (the Java `InputStreamReader`, as the Rust side of text is always UTF-8).
pub struct DecodingReader<R: Read> {
    inner: R,
    decoder: StreamDecoder,
    text: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl<R: Read> DecodingReader<R> {
    /// Creates the reader.
    pub fn new(inner: R, charset: Charset) -> Self {
        DecodingReader {
            inner,
            decoder: StreamDecoder::new(charset),
            text: Vec::new(),
            pos: 0,
            eof: false,
        }
    }

    /// Returns the inner reader.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for DecodingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.text.len() {
            if self.eof {
                return Ok(0);
            }
            let mut chunk = [0u8; 8192];
            let n = self.inner.read(&mut chunk)?;
            let mut s = String::new();
            if n == 0 {
                self.eof = true;
                self.decoder.decode(&[], &mut s, true);
            } else {
                self.decoder.decode(&chunk[..n], &mut s, false);
            }
            self.text = s.into_bytes();
            self.pos = 0;
        }
        let n = buf.len().min(self.text.len() - self.pos);
        buf[..n].copy_from_slice(&self.text[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// A reader that reads UTF-8 text from the inner reader and produces its bytes in a charset
/// (the Java `IOHelper.ReaderInputStream`), a chunk at a time so the content is never held in
/// memory as a whole.
pub struct EncodingReader<R: Read> {
    inner: R,
    encoder: StreamEncoder,
    pending: Vec<u8>,
    bytes: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl<R: Read> EncodingReader<R> {
    /// Creates the reader.
    ///
    /// # Arguments
    ///
    /// * `inner` - the reader of UTF-8 text
    /// * `charset` - the charset to encode the characters with
    pub fn new(inner: R, charset: Charset) -> Self {
        EncodingReader {
            inner,
            encoder: StreamEncoder::new(charset),
            pending: Vec::new(),
            bytes: Vec::new(),
            pos: 0,
            eof: false,
        }
    }
}

impl<R: Read> Read for EncodingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.pos >= self.bytes.len() {
            if self.eof {
                return Ok(0);
            }
            let mut chunk = [0u8; 4096];
            let n = self.inner.read(&mut chunk)?;
            self.bytes.clear();
            self.pos = 0;
            if n == 0 {
                self.eof = true;
                let rest = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                self.encoder.encode(&rest, &mut self.bytes);
                continue;
            }
            self.pending.extend_from_slice(&chunk[..n]);
            // characters that are not complete yet (a split UTF-8 sequence) wait for the next chunk
            let valid = match std::str::from_utf8(&self.pending) {
                Ok(_) => self.pending.len(),
                Err(e) if e.error_len().is_none() => e.valid_up_to(),
                Err(_) => self.pending.len(),
            };
            let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
            self.pending.drain(..valid);
            self.encoder.encode(&text, &mut self.bytes);
        }
        let n = buf.len().min(self.bytes.len() - self.pos);
        buf[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// A writer that accepts UTF-8 text and writes it to the inner writer in a charset (the Java
/// `OutputStreamWriter` / `IOHelper.EncodingFileWriter`).
pub struct EncodingWriter<W: Write> {
    inner: W,
    encoder: StreamEncoder,
    pending: Vec<u8>,
}

impl<W: Write> EncodingWriter<W> {
    /// Creates the writer.
    pub fn new(inner: W, charset: Charset) -> Self {
        EncodingWriter {
            inner,
            encoder: StreamEncoder::new(charset),
            pending: Vec::new(),
        }
    }

    /// Writes the text.
    ///
    /// # Errors
    ///
    /// Returns an error if writing to the inner writer fails.
    pub fn write_str(&mut self, text: &str) -> io::Result<()> {
        let mut out = Vec::with_capacity(text.len());
        self.encoder.encode(text, &mut out);
        self.inner.write_all(&out)
    }

    /// Returns the inner writer.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for EncodingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
        self.pending.drain(..valid);
        self.write_str(&text)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let rest = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned();
            self.write_str(&rest)?;
        }
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for (alias, name) in [
            ("UTF-8", "UTF-8"),
            ("utf8", "UTF-8"),
            ("iso-8859-1", "ISO-8859-1"),
            ("ISO8859_1", "ISO-8859-1"),
            ("latin1", "ISO-8859-1"),
            ("ascii", "US-ASCII"),
            ("UTF-16", "UTF-16"),
            ("UTF-16LE", "UTF-16LE"),
            ("windows-1252", "windows-1252"),
            ("Shift_JIS", "Shift_JIS"),
            ("iso-8859-15", "ISO-8859-15"),
        ] {
            assert_eq!(name, Charset::for_name(alias).unwrap().name(), "{alias}");
        }
        assert!(!Charset::is_supported("foo"));
        assert!(Charset::is_supported("utf-8"));
        assert_eq!(Charset::Utf8, Charset::default());
        assert_eq!("UTF-8", Charset::Utf8.to_string());
    }

    #[test]
    fn round_trips() {
        let text = "Hello S\u{f8}ren \u{20ac} \u{1F600}";
        for cs in [
            Charset::Utf8,
            Charset::Utf16,
            Charset::Utf16Be,
            Charset::Utf16Le,
        ] {
            assert_eq!(text, cs.decode(&cs.encode(text)), "{cs}");
        }
        assert_eq!(
            "Hello S\u{f8}ren ? ?",
            Charset::Iso8859_1.decode(&Charset::Iso8859_1.encode(text))
        );
        let cp = Charset::for_name("windows-1252").unwrap();
        assert_eq!("Hello S\u{f8}ren \u{20ac} ?", cp.decode(&cp.encode(text)));
        assert_eq!("\u{fffd}", Charset::UsAscii.decode(&[0xF8]));
        assert_eq!("\u{fffd}", Charset::Utf8.decode(&[0xFF]));
        // UTF-16 with a little endian byte order mark
        assert_eq!("A", Charset::Utf16.decode(&[0xFF, 0xFE, 0x41, 0x00]));
        // ISO-8859-1 is real Latin-1 (0x80 is a C1 control, not the euro sign)
        assert_eq!("\u{80}", Charset::Iso8859_1.decode(&[0x80]));
    }

    #[test]
    fn streams() {
        // decode Latin-1 from a reader into UTF-8
        let mut r = DecodingReader::new(&[0x53u8, 0xF8][..], Charset::Iso8859_1);
        let mut s = String::new();
        r.read_to_string(&mut s).unwrap();
        assert_eq!("S\u{f8}", s);
        // UTF-16 split across chunks
        let bytes = Charset::Utf16Le.encode(&"x\u{1F600}".repeat(5000));
        let mut s = String::new();
        DecodingReader::new(&bytes[..], Charset::Utf16Le)
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!("x\u{1F600}".repeat(5000), s);
        // encode UTF-8 into UTF-16 with a single byte order mark
        let text = "\u{f8}".repeat(3000);
        let mut out = Vec::new();
        EncodingReader::new(text.as_bytes(), Charset::Utf16)
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(Charset::Utf16.encode(&text), out);
        assert_eq!(2 + 3000 * 2, out.len());
        // writer with a multi-byte char split between writes
        let mut w = EncodingWriter::new(Vec::new(), Charset::Iso8859_1);
        let b = "S\u{f8}".as_bytes();
        w.write_all(&b[..2]).unwrap();
        w.write_all(&b[2..]).unwrap();
        w.flush().unwrap();
        assert_eq!(vec![0x53, 0xF8], w.into_inner());
    }
}
