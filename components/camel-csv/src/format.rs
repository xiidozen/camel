//! The format of CSV texts (Java Apache Commons CSV `CSVFormat`): the delimiter, quote, escape
//! and comment characters, the record separator, the header and how values are quoted when
//! printed, with the predefined formats of Commons CSV.

use camel_api::error::{CamelError, CamelResult};
use camel_util::value::Value;

/// When values are quoted when printed (Java Commons CSV `QuoteMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteMode {
    /// Quotes all the values.
    All,
    /// Quotes all the values that are not null.
    AllNonNull,
    /// Quotes only the values with special characters (the delimiter, the quote or escape
    /// character, a line break, a leading character up to `#` or a trailing space).
    Minimal,
    /// Quotes all the values that are not numbers.
    NonNumeric,
    /// Never quotes, escapes the special characters instead.
    None,
}

impl QuoteMode {
    /// The quote mode of its Java name (`ALL`, `ALL_NON_NULL`, `MINIMAL`, `NON_NUMERIC`,
    /// `NONE`), ignoring the case.
    ///
    /// # Errors
    ///
    /// An unknown name.
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_csv::format::QuoteMode;
    ///
    /// assert_eq!(QuoteMode::AllNonNull, QuoteMode::parse("ALL_NON_NULL").unwrap());
    /// assert!(QuoteMode::parse("SOME").is_err());
    /// ```
    pub fn parse(name: &str) -> CamelResult<Self> {
        Ok(match name.trim().to_ascii_uppercase().as_str() {
            "ALL" => QuoteMode::All,
            "ALL_NON_NULL" => QuoteMode::AllNonNull,
            "MINIMAL" => QuoteMode::Minimal,
            "NON_NUMERIC" => QuoteMode::NonNumeric,
            "NONE" => QuoteMode::None,
            _ => {
                return Err(CamelError::illegal_argument(format!(
                    "No enum constant org.apache.camel.model.dataformat.QuoteMode.{name}"
                )))
            }
        })
    }
}

/// The format of CSV texts (Java Commons CSV `CSVFormat`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvFormat {
    /// The delimiter of the values (`,` by default).
    pub delimiter: String,
    /// The quote character (`"` by default), `None` to never quote.
    pub quote: Option<char>,
    /// When values are quoted when printed (`None`: [`QuoteMode::Minimal`]).
    pub quote_mode: Option<QuoteMode>,
    /// The escape character.
    pub escape: Option<char>,
    /// The character starting the comment lines.
    pub comment_marker: Option<char>,
    /// The separator of the printed records (CRLF by default); parsing accepts any line
    /// break.
    pub record_separator: Option<String>,
    /// The text of the null values: printed for null values, read as null.
    pub null_string: Option<String>,
    /// The names of the columns; an empty list reads them from the first record.
    pub header: Option<Vec<String>>,
    /// Whether the header record is skipped (not printed, or not read as values).
    pub skip_header_record: bool,
    /// Whether the empty lines are ignored when parsing.
    pub ignore_empty_lines: bool,
    /// Whether the spaces around the values are ignored when parsing.
    pub ignore_surrounding_spaces: bool,
    /// Whether the header can have empty column names.
    pub allow_missing_column_names: bool,
    /// Whether the values are trimmed.
    pub trim: bool,
    /// Whether the column names ignore the case.
    pub ignore_header_case: bool,
    /// Whether a delimiter ends each record.
    pub trailing_delimiter: bool,
    /// Whether characters after a quoted value are kept when parsing.
    pub trailing_data: bool,
    /// Whether the end of the text ends an unterminated quoted value.
    pub lenient_eof: bool,
}

impl Default for CsvFormat {
    fn default() -> Self {
        CsvFormat {
            delimiter: ",".to_string(),
            quote: Some('"'),
            quote_mode: None,
            escape: None,
            comment_marker: None,
            record_separator: Some("\r\n".to_string()),
            null_string: None,
            header: None,
            skip_header_record: false,
            ignore_empty_lines: true,
            ignore_surrounding_spaces: false,
            allow_missing_column_names: false,
            trim: false,
            ignore_header_case: false,
            trailing_delimiter: false,
            trailing_data: false,
            lenient_eof: false,
        }
    }
}

/// The names of the predefined formats (Java `CSVFormat.Predefined`).
pub const PREDEFINED: [&str; 12] = [
    "DEFAULT",
    "EXCEL",
    "INFORMIX_UNLOAD",
    "INFORMIX_UNLOAD_CSV",
    "MONGODB_CSV",
    "MONGODB_TSV",
    "MYSQL",
    "ORACLE",
    "POSTGRESQL_CSV",
    "POSTGRESQL_TEXT",
    "RFC4180",
    "TDF",
];

impl CsvFormat {
    /// The Commons CSV `DEFAULT` format: `,` delimited, `"` quoted, CRLF separated, empty lines
    /// ignored.
    pub fn default_format() -> Self {
        Self::default()
    }

    /// The Excel format (`EXCEL`): the empty lines kept, missing column names allowed.
    pub fn excel() -> Self {
        CsvFormat {
            ignore_empty_lines: false,
            allow_missing_column_names: true,
            trailing_data: true,
            lenient_eof: true,
            ..Self::default()
        }
    }

    /// The MySQL format (`MYSQL`): tab delimited, `\` escaped, not quoted, LF separated, `\N`
    /// for null.
    pub fn mysql() -> Self {
        CsvFormat {
            delimiter: "\t".to_string(),
            escape: Some('\\'),
            ignore_empty_lines: false,
            quote: None,
            record_separator: Some("\n".to_string()),
            null_string: Some("\\N".to_string()),
            quote_mode: Some(QuoteMode::AllNonNull),
            ..Self::default()
        }
    }

    /// The tab delimited format (`TDF`).
    pub fn tdf() -> Self {
        CsvFormat {
            delimiter: "\t".to_string(),
            ignore_surrounding_spaces: true,
            ..Self::default()
        }
    }

    /// A predefined format by its name (as `EXCEL` or `InformixUnload`: the underscores and
    /// the case are ignored, Java `CsvDataFormat.resolveFormat`).
    ///
    /// # Errors
    ///
    /// An unknown name.
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_csv::format::CsvFormat;
    ///
    /// assert_eq!(CsvFormat::mysql(), CsvFormat::predefined("mysql").unwrap());
    /// assert_eq!("|", CsvFormat::predefined("InformixUnload").unwrap().delimiter);
    /// assert!(CsvFormat::predefined("CUSTOM").is_err());
    /// ```
    pub fn predefined(name: &str) -> CamelResult<Self> {
        let d = Self::default();
        Ok(match name.replace('_', "").to_ascii_uppercase().as_str() {
            "DEFAULT" => d,
            "EXCEL" => Self::excel(),
            "INFORMIXUNLOAD" => CsvFormat {
                delimiter: "|".to_string(),
                escape: Some('\\'),
                record_separator: Some("\n".to_string()),
                ..d
            },
            "INFORMIXUNLOADCSV" => CsvFormat {
                record_separator: Some("\n".to_string()),
                ..d
            },
            "MONGODBCSV" => CsvFormat {
                escape: Some('"'),
                quote_mode: Some(QuoteMode::Minimal),
                ..d
            },
            "MONGODBTSV" => CsvFormat {
                delimiter: "\t".to_string(),
                escape: Some('"'),
                quote_mode: Some(QuoteMode::Minimal),
                ..d
            },
            "MYSQL" => Self::mysql(),
            "ORACLE" => CsvFormat {
                escape: Some('\\'),
                ignore_empty_lines: false,
                null_string: Some("\\N".to_string()),
                trim: true,
                record_separator: Some("\n".to_string()),
                quote_mode: Some(QuoteMode::Minimal),
                ..d
            },
            "POSTGRESQLCSV" => CsvFormat {
                ignore_empty_lines: false,
                record_separator: Some("\n".to_string()),
                null_string: Some(String::new()),
                quote_mode: Some(QuoteMode::AllNonNull),
                ..d
            },
            "POSTGRESQLTEXT" => CsvFormat {
                delimiter: "\t".to_string(),
                escape: Some('\\'),
                ignore_empty_lines: false,
                quote: None,
                record_separator: Some("\n".to_string()),
                null_string: Some("\\N".to_string()),
                quote_mode: Some(QuoteMode::AllNonNull),
                ..d
            },
            "RFC4180" => CsvFormat {
                ignore_empty_lines: false,
                ..d
            },
            "TDF" => Self::tdf(),
            _ => {
                return Err(CamelError::illegal_argument(format!(
                    "Unknown CSV format: {name}"
                )))
            }
        })
    }

    /// Checks the format (Java `CSVFormat.validate`).
    ///
    /// # Errors
    ///
    /// An empty delimiter or one with a line break, a quote, escape or comment character
    /// equal to the delimiter or to each other, or the quote mode `NONE` without an escape
    /// character.
    pub fn validate(&self) -> CamelResult<()> {
        let err = |m: String| Err(CamelError::illegal_argument(m));
        if self.delimiter.is_empty() {
            return err("Empty delimiter".to_string());
        }
        if self.delimiter.contains(['\r', '\n']) {
            return err("The delimiter cannot be a line break".to_string());
        }
        let in_delimiter = |c: Option<char>| c.is_some_and(|c| self.delimiter.contains(c));
        if in_delimiter(self.quote) {
            return err(format!(
                "The quoteChar character and the delimiter cannot be the same ('{}')",
                self.quote.unwrap()
            ));
        }
        if in_delimiter(self.escape) {
            return err(format!(
                "The escape character and the delimiter cannot be the same ('{}')",
                self.escape.unwrap()
            ));
        }
        if in_delimiter(self.comment_marker) {
            return err(format!(
                "The comment start character and the delimiter cannot be the same ('{}')",
                self.comment_marker.unwrap()
            ));
        }
        if let Some(c) = self.comment_marker {
            if Some(c) == self.quote {
                return err(format!(
                    "The comment start character and the quoteChar cannot be the same ('{c}')"
                ));
            }
            if Some(c) == self.escape {
                return err(format!(
                    "The comment start and the escape character cannot be the same ('{c}')"
                ));
            }
        }
        if self.escape.is_none() && self.quote_mode == Some(QuoteMode::None) {
            return err("Quote mode set to NONE but no escape character is set".to_string());
        }
        Ok(())
    }

    /// Prints the header (when there is one and it is not skipped, Java `CSVPrinter`).
    pub fn print_header(&self, out: &mut String) {
        if let Some(h) = &self.header {
            if !h.is_empty() && !self.skip_header_record {
                let values: Vec<Value> = h.iter().map(|n| Value::from(n.as_str())).collect();
                self.print_record(out, &values);
            }
        }
    }

    /// Prints a record of values followed by the record separator (Java
    /// `CSVPrinter.printRecord`).
    ///
    /// # Examples
    ///
    /// ```
    /// use camel_csv::format::CsvFormat;
    /// use camel_util::value::Value;
    ///
    /// let mut out = String::new();
    /// let values = [Value::from("a"), Value::from("b,c"), Value::Int(1), Value::Null];
    /// CsvFormat::default().print_record(&mut out, &values);
    /// assert_eq!("a,\"b,c\",1,\r\n", out);
    /// ```
    pub fn print_record(&self, out: &mut String, values: &[Value]) {
        for (i, v) in values.iter().enumerate() {
            self.print_value(out, v, i == 0);
        }
        self.println(out);
    }

    /// Ends a record (Java `CSVPrinter.println`).
    pub fn println(&self, out: &mut String) {
        if self.trailing_delimiter {
            out.push_str(&self.delimiter);
        }
        if let Some(s) = &self.record_separator {
            out.push_str(s);
        }
    }

    /// Prints one value of a record (Java `CSVFormat.print`).
    pub fn print_value(&self, out: &mut String, value: &Value, new_record: bool) {
        if !new_record {
            out.push_str(&self.delimiter);
        }
        if value.is_null() {
            match (&self.null_string, self.quote) {
                (None, _) => {}
                (Some(n), Some(q)) if self.quote_mode == Some(QuoteMode::All) => {
                    out.push(q);
                    out.push_str(&self.trimmed(n));
                    out.push(q);
                }
                (Some(n), _) => out.push_str(&self.trimmed(n)),
            }
            return;
        }
        let text = match value {
            Value::Text(t) => self.trimmed(t),
            other => self.trimmed(&other.to_string()),
        };
        if let Some(q) = self.quote {
            self.print_with_quotes(value, &text, out, new_record, q);
        } else if self.escape.is_some() {
            self.print_with_escapes(&text, out);
        } else {
            out.push_str(&text);
        }
    }

    fn trimmed(&self, text: &str) -> String {
        if self.trim {
            java_trim(text).to_string()
        } else {
            text.to_string()
        }
    }

    /// Whether a value needs quotes in the minimal mode.
    fn needs_quotes(&self, text: &str, new_record: bool, quote: char) -> bool {
        let Some(first) = text.chars().next() else {
            // an empty first value is quoted, as it may be the only one of the line
            return new_record;
        };
        if first <= '#' {
            return true;
        }
        let escape = self.escape.unwrap_or(quote);
        for (i, c) in text.char_indices() {
            if c == '\n'
                || c == '\r'
                || c == quote
                || c == escape
                || text[i..].starts_with(&self.delimiter)
            {
                return true;
            }
        }
        text.chars().last().is_some_and(|c| c <= ' ')
    }

    fn print_with_quotes(
        &self,
        value: &Value,
        text: &str,
        out: &mut String,
        new_record: bool,
        quote: char,
    ) {
        let quoted = match self.quote_mode.unwrap_or(QuoteMode::Minimal) {
            QuoteMode::All | QuoteMode::AllNonNull => true,
            QuoteMode::NonNumeric => !matches!(value, Value::Int(_) | Value::Float(_)),
            QuoteMode::None => {
                self.print_with_escapes(text, out);
                return;
            }
            QuoteMode::Minimal => self.needs_quotes(text, new_record, quote),
        };
        if !quoted {
            out.push_str(text);
            return;
        }
        let escape = self.escape.unwrap_or(quote);
        out.push(quote);
        for c in text.chars() {
            if c == quote || c == escape {
                out.push(escape);
            }
            out.push(c);
        }
        out.push(quote);
    }

    fn print_with_escapes(&self, text: &str, out: &mut String) {
        let Some(escape) = self.escape else {
            out.push_str(text);
            return;
        };
        let mut chars = text.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if text[i..].starts_with(&self.delimiter) {
                for d in self.delimiter.chars() {
                    out.push(escape);
                    out.push(d);
                }
                for _ in 1..self.delimiter.chars().count() {
                    chars.next();
                }
            } else if c == '\r' || c == '\n' || c == escape {
                out.push(escape);
                out.push(match c {
                    '\r' => 'r',
                    '\n' => 'n',
                    c => c,
                });
            } else {
                out.push(c);
            }
        }
    }
}

/// The text without its leading and trailing characters up to the space (Java
/// `String.trim`).
pub(crate) fn java_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(f: &CsvFormat, values: &[Value]) -> String {
        let mut out = String::new();
        f.print_record(&mut out, values);
        out
    }

    #[test]
    fn test_print() {
        let d = CsvFormat::default();
        assert_eq!("\"\"\r\n", print(&d, &[Value::from("")]));
        assert_eq!("a,\r\n", print(&d, &[Value::from("a"), Value::from("")]));
        assert_eq!(
            "\"a\"\"b\",\" x\",\"y \",\"#c\"\r\n",
            print(
                &d,
                &[
                    Value::from("a\"b"),
                    Value::from(" x"),
                    Value::from("y "),
                    Value::from("#c")
                ]
            )
        );
        let all = CsvFormat {
            quote_mode: Some(QuoteMode::All),
            null_string: Some("NULL".into()),
            ..CsvFormat::default()
        };
        assert_eq!(
            "\"1\",\"NULL\"\r\n",
            print(&all, &[Value::Int(1), Value::Null])
        );
        let nn = CsvFormat {
            quote_mode: Some(QuoteMode::NonNumeric),
            ..CsvFormat::default()
        };
        assert_eq!(
            "1,\"a\"\r\n",
            print(&nn, &[Value::Int(1), Value::from("a")])
        );
        let mysql = CsvFormat::mysql();
        assert_eq!(
            "a\\\tb\t\\N\tc\\nd\n",
            print(
                &mysql,
                &[Value::from("a\tb"), Value::Null, Value::from("c\nd")]
            )
        );
        let none = CsvFormat {
            quote_mode: Some(QuoteMode::None),
            escape: Some('!'),
            ..CsvFormat::default()
        };
        assert_eq!(
            "a!,b,c!!\r\n",
            print(&none, &[Value::from("a,b"), Value::from("c!")])
        );
        assert!(CsvFormat {
            quote_mode: Some(QuoteMode::None),
            ..CsvFormat::default()
        }
        .validate()
        .is_err());
        assert!(CsvFormat {
            quote: Some(','),
            ..CsvFormat::default()
        }
        .validate()
        .is_err());
    }
}
