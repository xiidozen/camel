//! The parsing of CSV texts (Java Apache Commons CSV `Lexer`, `CSVParser` and `CSVRecord`):
//! quoted and escaped values, comment lines, empty lines, the header and the null string.

use crate::format::{java_trim, CsvFormat, QuoteMode};
use camel_api::error::{CamelError, CamelResult, ErrorKind};
use std::fmt;
use std::sync::Arc;

/// A parsing error (Java Commons CSV `CSVException`, an `IOException`).
fn io_error(message: String) -> CamelError {
    CamelError::new(ErrorKind::Io, message)
}

const EOF: i32 = -1;
const UNDEFINED: i32 = -2;
const CR: i32 = '\r' as i32;
const LF: i32 = '\n' as i32;

/// The names of the columns and their indexes (Java `CSVParser.Headers`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderMap {
    entries: Vec<(String, usize)>,
    ignore_case: bool,
}

impl HeaderMap {
    fn put(&mut self, name: &str, index: usize) {
        let found = self.entries.iter_mut().find(|(n, _)| {
            if self.ignore_case {
                n.eq_ignore_ascii_case(name) || n.to_lowercase() == name.to_lowercase()
            } else {
                n == name
            }
        });
        match found {
            Some(e) => e.1 = index,
            None => self.entries.push((name.to_string(), index)),
        }
    }

    /// The index of the column of a name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .find(|(n, _)| {
                if self.ignore_case {
                    n.to_lowercase() == name.to_lowercase()
                } else {
                    n == name
                }
            })
            .map(|(_, i)| *i)
    }

    /// The names and their indexes, in the order of the header (sorted ignoring the case
    /// when the case is ignored, as the Java `TreeMap`).
    pub fn entries(&self) -> &[(String, usize)] {
        &self.entries
    }
}

/// A record of values (Java Commons CSV `CSVRecord`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvRecord {
    values: Vec<Option<String>>,
    header: Option<Arc<HeaderMap>>,
    record_number: u64,
    comment: Option<String>,
}

impl CsvRecord {
    /// The values (`None` for the null values).
    pub fn values(&self) -> &[Option<String>] {
        &self.values
    }

    /// The number of values.
    pub fn size(&self) -> usize {
        self.values.len()
    }

    /// The value at an index (`None` for a null value or an index out of the record).
    pub fn get(&self, index: usize) -> Option<&str> {
        self.values.get(index).and_then(|v| v.as_deref())
    }

    /// The value of a column of the header.
    pub fn get_by_name(&self, name: &str) -> Option<&str> {
        self.header
            .as_ref()
            .and_then(|h| h.index_of(name))
            .and_then(|i| self.get(i))
    }

    /// The number of the record (1 for the first record after the header).
    pub fn record_number(&self) -> u64 {
        self.record_number
    }

    /// The comment lines before the record.
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    /// The values by the names of the header (Java `CSVRecord.toMap`): the columns beyond
    /// the values are left out.
    pub fn to_map(&self) -> Vec<(String, Option<String>)> {
        let Some(h) = &self.header else {
            return Vec::new();
        };
        h.entries
            .iter()
            .filter(|(_, i)| *i < self.values.len())
            .map(|(n, i)| (n.clone(), self.values[*i].clone()))
            .collect()
    }
}

impl fmt::Display for CsvRecord {
    /// As the Java `CSVRecord.toString`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let values: Vec<&str> = self
            .values
            .iter()
            .map(|v| v.as_deref().unwrap_or("null"))
            .collect();
        write!(
            f,
            "CSVRecord [comment='{}', recordNumber={}, values=[{}]]",
            self.comment.as_deref().unwrap_or("null"),
            self.record_number,
            values.join(", ")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenType {
    Invalid,
    Token,
    Eof,
    EoRecord,
    Comment,
}

struct Token {
    kind: TokenType,
    content: String,
    ready: bool,
    quoted: bool,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    last: i32,
    line: u64,
    delimiter: Vec<char>,
    quote: i32,
    escape: i32,
    comment: i32,
    ignore_surrounding_spaces: bool,
    ignore_empty_lines: bool,
    trailing_data: bool,
    lenient_eof: bool,
    last_token_delimiter: bool,
}

fn code(c: Option<char>) -> i32 {
    c.map_or(UNDEFINED, |c| c as i32)
}

fn to_char(c: i32) -> char {
    char::from_u32(c as u32).unwrap_or('\u{fffd}')
}

fn is_whitespace(c: i32) -> bool {
    c >= 0 && to_char(c).is_whitespace()
}

impl Lexer {
    fn read(&mut self) -> i32 {
        let c = match self.chars.get(self.pos) {
            Some(&c) => {
                self.pos += 1;
                c as i32
            }
            None => EOF,
        };
        if c == CR || (c == LF && self.last != CR) {
            self.line += 1;
        }
        self.last = c;
        c
    }

    fn peek(&self) -> i32 {
        self.chars.get(self.pos).map_or(EOF, |&c| c as i32)
    }

    fn read_end_of_line(&mut self, c: i32) -> bool {
        if c == CR && self.peek() == LF {
            self.read();
        }
        c == LF || c == CR
    }

    fn is_delimiter(&mut self, c: i32) -> bool {
        self.last_token_delimiter = false;
        if c != self.delimiter[0] as i32 {
            return false;
        }
        let n = self.delimiter.len();
        if n > 1 {
            if self.chars.len() < self.pos + n - 1
                || self.chars[self.pos..self.pos + n - 1] != self.delimiter[1..]
            {
                return false;
            }
            self.pos += n - 1;
            self.last = self.delimiter[n - 1] as i32;
        }
        self.last_token_delimiter = true;
        true
    }

    /// The escape char followed by the delimiter (consumed).
    fn is_escape_delimiter(&mut self) -> bool {
        let n = self.delimiter.len();
        if self.chars.len() >= self.pos + n
            && self.chars[self.pos..self.pos + n] == self.delimiter[..]
        {
            self.pos += n;
            self.last = self.delimiter[n - 1] as i32;
            true
        } else {
            false
        }
    }

    fn is_meta_char(&self, c: i32) -> bool {
        c == self.delimiter[0] as i32 || c == self.escape || c == self.quote || c == self.comment
    }

    fn read_escape(&mut self) -> CamelResult<i32> {
        let c = self.read();
        Ok(match c {
            0x72 => CR,   // 'r'
            0x6e => LF,   // 'n'
            0x74 => 0x09, // 't'
            0x62 => 0x08, // 'b'
            0x66 => 0x0c, // 'f'
            0x0d | 0x0a | 0x0c | 0x09 | 0x08 => c,
            EOF => {
                return Err(io_error(
                    "EOF whilst processing escape sequence".to_string(),
                ))
            }
            _ if self.is_meta_char(c) => c,
            // not an escape sequence: the escape character and the character are kept
            _ => EOF,
        })
    }

    fn append_escaped(&mut self, content: &mut String, c: i32) -> CamelResult<()> {
        if self.is_escape_delimiter() {
            content.extend(self.delimiter.iter());
        } else {
            let unescaped = self.read_escape()?;
            if unescaped == EOF {
                content.push(to_char(c));
                content.push(to_char(self.last));
            } else {
                content.push(to_char(unescaped));
            }
        }
        Ok(())
    }

    fn read_line(&mut self) -> String {
        let mut line = String::new();
        loop {
            let c = self.read();
            if c == CR && self.peek() == LF {
                self.read();
            }
            if c == EOF || c == LF || c == CR {
                return line;
            }
            line.push(to_char(c));
        }
    }

    fn next_token(&mut self, token: &mut Token) -> CamelResult<()> {
        let mut last = self.last;
        let mut c = self.read();
        let mut eol = self.read_end_of_line(c);
        if self.ignore_empty_lines {
            while eol && (last == CR || last == LF || last == UNDEFINED) {
                last = c;
                c = self.read();
                eol = self.read_end_of_line(c);
                if c == EOF {
                    token.kind = TokenType::Eof;
                    return Ok(());
                }
            }
        }
        if last == EOF || (!self.last_token_delimiter && c == EOF) {
            token.kind = TokenType::Eof;
            return Ok(());
        }
        if (last == CR || last == LF || last == UNDEFINED) && c != UNDEFINED && c == self.comment {
            let line = self.read_line();
            token.content.push_str(line.trim());
            token.kind = TokenType::Comment;
            return Ok(());
        }
        while token.kind == TokenType::Invalid {
            if self.ignore_surrounding_spaces {
                while is_whitespace(c) && c != self.delimiter[0] as i32 && !eol {
                    c = self.read();
                    eol = self.read_end_of_line(c);
                }
            }
            if self.is_delimiter(c) {
                token.kind = TokenType::Token;
            } else if eol {
                token.kind = TokenType::EoRecord;
            } else if c == self.quote {
                self.parse_encapsulated_token(token)?;
            } else if c == EOF {
                token.kind = TokenType::Eof;
                token.ready = true;
            } else {
                self.parse_simple_token(token, c)?;
            }
        }
        Ok(())
    }

    fn parse_simple_token(&mut self, token: &mut Token, mut c: i32) -> CamelResult<()> {
        loop {
            if self.read_end_of_line(c) {
                token.kind = TokenType::EoRecord;
                break;
            }
            if c == EOF {
                token.kind = TokenType::Eof;
                token.ready = true;
                break;
            }
            if self.is_delimiter(c) {
                token.kind = TokenType::Token;
                break;
            }
            if c == self.escape {
                self.append_escaped(&mut token.content, c)?;
            } else {
                token.content.push(to_char(c));
            }
            c = self.read();
        }
        if self.ignore_surrounding_spaces {
            let n = token.content.trim_end_matches(char::is_whitespace).len();
            token.content.truncate(n);
        }
        Ok(())
    }

    fn parse_encapsulated_token(&mut self, token: &mut Token) -> CamelResult<()> {
        token.quoted = true;
        let start_line = self.line;
        loop {
            let c = self.read();
            if c == self.quote {
                if self.peek() == self.quote {
                    let c = self.read();
                    token.content.push(to_char(c));
                } else {
                    // the end of the quoted value: only spaces until the delimiter
                    loop {
                        let c = self.read();
                        if self.is_delimiter(c) {
                            token.kind = TokenType::Token;
                            return Ok(());
                        }
                        if c == EOF {
                            token.kind = TokenType::Eof;
                            token.ready = true;
                            return Ok(());
                        }
                        if self.read_end_of_line(c) {
                            token.kind = TokenType::EoRecord;
                            return Ok(());
                        }
                        if self.trailing_data {
                            token.content.push(to_char(c));
                        } else if !is_whitespace(c) {
                            return Err(io_error(format!(
                                "Invalid character between encapsulated token and delimiter at line: {}, position: {}",
                                self.line, self.pos
                            )));
                        }
                    }
                }
            } else if c == self.escape {
                self.append_escaped(&mut token.content, c)?;
            } else if c == EOF {
                if self.lenient_eof {
                    token.kind = TokenType::Eof;
                    token.ready = true;
                    return Ok(());
                }
                return Err(io_error(format!(
                    "EOF reached before encapsulated token finished at line {start_line}"
                )));
            } else {
                token.content.push(to_char(c));
            }
        }
    }
}

/// The parser of a CSV text (Java Commons CSV `CSVParser`).
pub struct CsvParser {
    lexer: Lexer,
    format: CsvFormat,
    header: Option<Arc<HeaderMap>>,
    header_names: Option<Vec<String>>,
    record_number: u64,
    values: Vec<Option<String>>,
}

impl CsvParser {
    /// The parser of the text, the header read when the format asks for it.
    ///
    /// # Errors
    ///
    /// An invalid format, an invalid text or a header with missing names.
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_csv::format::CsvFormat;
    /// use camel_csv::parser::CsvParser;
    ///
    /// let format = CsvFormat { header: Some(vec![]), ..CsvFormat::default() };
    /// let mut p = CsvParser::new(&format, "a,b\r\n1,\"x,y\"\r\n").unwrap();
    /// assert_eq!(Some(&["a".to_string(), "b".to_string()][..]), p.header_names());
    /// let records = p.records().unwrap();
    /// assert_eq!(Some("x,y"), records[0].get_by_name("b"));
    /// ```
    pub fn new(format: &CsvFormat, text: &str) -> CamelResult<Self> {
        format.validate()?;
        let lexer = Lexer {
            chars: text.chars().collect(),
            pos: 0,
            last: UNDEFINED,
            line: 0,
            delimiter: format.delimiter.chars().collect(),
            quote: code(format.quote),
            escape: code(format.escape),
            comment: code(format.comment_marker),
            ignore_surrounding_spaces: format.ignore_surrounding_spaces,
            ignore_empty_lines: format.ignore_empty_lines,
            trailing_data: format.trailing_data,
            lenient_eof: format.lenient_eof,
            last_token_delimiter: false,
        };
        let mut p = CsvParser {
            lexer,
            format: format.clone(),
            header: None,
            header_names: None,
            record_number: 0,
            values: Vec::new(),
        };
        p.create_headers()?;
        Ok(p)
    }

    fn create_headers(&mut self) -> CamelResult<()> {
        let Some(format_header) = self.format.header.clone() else {
            return Ok(());
        };
        let header_record: Option<Vec<Option<String>>> = if format_header.is_empty() {
            self.next_record()?.map(|r| r.values)
        } else {
            if self.format.skip_header_record {
                self.next_record()?;
            }
            Some(format_header.into_iter().map(Some).collect())
        };
        let mut map = HeaderMap {
            entries: Vec::new(),
            ignore_case: self.format.ignore_header_case,
        };
        let mut names = Vec::new();
        if let Some(record) = header_record {
            for (i, h) in record.iter().enumerate() {
                let blank = h.as_deref().map_or(true, |h| h.trim().is_empty());
                if blank && !self.format.allow_missing_column_names {
                    let shown: Vec<&str> = record
                        .iter()
                        .map(|h| h.as_deref().unwrap_or("null"))
                        .collect();
                    return Err(CamelError::illegal_argument(format!(
                        "A header name is missing in [{}]",
                        shown.join(", ")
                    )));
                }
                if let Some(h) = h {
                    map.put(h, i);
                    names.push(h.clone());
                }
            }
        }
        if map.ignore_case {
            map.entries.sort_by_key(|(n, _)| n.to_lowercase());
        }
        self.header = Some(Arc::new(map));
        self.header_names = Some(names);
        Ok(())
    }

    /// The names of the header (`None` without a header).
    pub fn header_names(&self) -> Option<&[String]> {
        self.header_names.as_deref()
    }

    fn add_value(&mut self, content: String, quoted: bool, last: bool) {
        let input = if self.format.trim {
            java_trim(&content).to_string()
        } else {
            content
        };
        if last && input.is_empty() && self.format.trailing_delimiter {
            return;
        }
        // Java CSVParser.handleNull
        let strict = matches!(
            self.format.quote_mode,
            Some(QuoteMode::AllNonNull | QuoteMode::NonNumeric)
        );
        let value = match &self.format.null_string {
            Some(n) if *n == input => {
                if strict && quoted {
                    Some(input)
                } else {
                    None
                }
            }
            None if strict && input.is_empty() && !quoted => None,
            _ => Some(input),
        };
        self.values.push(value);
    }

    /// The next record, `None` at the end of the text (Java `CSVParser.nextRecord`).
    ///
    /// # Errors
    ///
    /// An invalid text.
    pub fn next_record(&mut self) -> CamelResult<Option<CsvRecord>> {
        self.values.clear();
        let mut comment: Option<String> = None;
        loop {
            let mut token = Token {
                kind: TokenType::Invalid,
                content: String::new(),
                ready: false,
                quoted: false,
            };
            self.lexer.next_token(&mut token)?;
            match token.kind {
                TokenType::Token => self.add_value(token.content, token.quoted, false),
                TokenType::EoRecord => {
                    self.add_value(token.content, token.quoted, true);
                    break;
                }
                TokenType::Eof => {
                    if token.ready {
                        self.add_value(token.content, token.quoted, true);
                    }
                    break;
                }
                TokenType::Comment => {
                    let c = comment.get_or_insert_with(String::new);
                    if !c.is_empty() {
                        c.push('\n');
                    }
                    c.push_str(&token.content);
                }
                TokenType::Invalid => return Err(io_error("invalid parse sequence".to_string())),
            }
        }
        if self.values.is_empty() {
            return Ok(None);
        }
        self.record_number += 1;
        Ok(Some(CsvRecord {
            values: std::mem::take(&mut self.values),
            header: self.header.clone(),
            record_number: self.record_number,
            comment,
        }))
    }

    /// The remaining records.
    ///
    /// # Errors
    ///
    /// An invalid text.
    pub fn records(&mut self) -> CamelResult<Vec<CsvRecord>> {
        let mut records = Vec::new();
        while let Some(r) = self.next_record()? {
            records.push(r);
        }
        Ok(records)
    }
}

/// The records of a text.
///
/// # Errors
///
/// An invalid format or text.
pub fn parse(format: &CsvFormat, text: &str) -> CamelResult<Vec<CsvRecord>> {
    CsvParser::new(format, text)?.records()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(format: &CsvFormat, text: &str) -> Vec<Vec<Option<String>>> {
        parse(format, text)
            .unwrap()
            .into_iter()
            .map(|r| r.values)
            .collect()
    }

    fn s(v: &[&str]) -> Vec<Option<String>> {
        v.iter().map(|s| Some(s.to_string())).collect()
    }

    #[test]
    fn test_parse() {
        let d = CsvFormat::default();
        assert_eq!(
            vec![s(&["A", "B", "C"]), s(&["1", "2", "3"])],
            rows(&d, "A,B,C\r1,2,3")
        );
        assert_eq!(vec![s(&["a", ""]), s(&["b"])], rows(&d, "a,\n\n\nb\n"));
        assert_eq!(vec![s(&["x\"y", "1\n2"])], rows(&d, "\"x\"\"y\",\"1\n2\""));
        assert_eq!(
            vec![s(&["a"]), s(&[""]), s(&["b"])],
            rows(&CsvFormat::predefined("RFC4180").unwrap(), "a\n\nb")
        );
        let comments = CsvFormat {
            comment_marker: Some('#'),
            ..CsvFormat::default()
        };
        let r = parse(&comments, "# one\na\n#two\nb").unwrap();
        assert_eq!(2, r.len());
        assert_eq!(Some("one"), r[0].comment());
        assert_eq!(
            "CSVRecord [comment='two', recordNumber=2, values=[b]]",
            r[1].to_string()
        );
        let mysql = CsvFormat::mysql();
        assert_eq!(
            vec![vec![
                Some("a\tb".to_string()),
                None,
                Some("c\nd".to_string())
            ]],
            rows(&mysql, "a\\tb\t\\N\tc\\nd\n")
        );
        assert!(parse(&d, "\"abc").is_err());
        assert!(parse(&d, "\"a\"b,c").is_err());
        assert_eq!(vec![s(&["ab", "c"])], rows(&CsvFormat::excel(), "\"a\"b,c"));
        let spaces = CsvFormat {
            ignore_surrounding_spaces: true,
            ..CsvFormat::default()
        };
        assert_eq!(
            vec![s(&["a", "b c", "d"])],
            rows(&spaces, "  a , b c ,  \"d\"  ")
        );
        let multi = CsvFormat {
            delimiter: "[|]".into(),
            ..CsvFormat::default()
        };
        assert_eq!(vec![s(&["a", "b|c", "d"])], rows(&multi, "a[|]b|c[|]d"));
        let missing = CsvFormat {
            header: Some(vec![]),
            ..CsvFormat::default()
        };
        assert!(CsvParser::new(&missing, "a,,c\n1,2,3").is_err());
    }
}
