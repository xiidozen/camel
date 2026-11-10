//! Utility methods for quoting strings. Port of `org.apache.camel.util.StringQuoteHelper`.

use crate::lang::java_trim;

/// Returns the text wrapped in double quotes.
///
/// # Arguments
///
/// * `text` - the text
///
/// # Returns
///
/// The text wrapped in double quotes.
pub fn double_quote(text: &str) -> String {
    quote(text, "\"")
}

/// Returns the text wrapped in single quotes.
///
/// # Arguments
///
/// * `text` - the text
///
/// # Returns
///
/// The text wrapped in single quotes.
pub fn single_quote(text: &str) -> String {
    quote(text, "'")
}

/// Wraps the text in the given quote text.
///
/// # Arguments
///
/// * `text` - the text
/// * `quote` - the quote text
///
/// # Returns
///
/// The text wrapped in the given quotes.
pub fn quote(text: &str, quote: &str) -> String {
    format!("{quote}{text}{quote}")
}

/// Returns the text as a JSON string literal.
///
/// Escapes `"`, `\`, new lines, carriage returns, tabs and any other control characters (as
/// `\uXXXX`).
///
/// # Arguments
///
/// * `text` - the text, `None` is rendered as the JSON literal `null`
///
/// # Returns
///
/// The JSON string literal.
///
/// # Examples
///
/// ```
/// use camel_util::string_quote_helper::json_quote;
/// assert_eq!("\"line1\\nline2\"", json_quote(Some("line1\nline2")));
/// assert_eq!("null", json_quote(None));
/// ```
pub fn json_quote(text: Option<&str>) -> String {
    let Some(text) = text else {
        return "null".to_string();
    };
    let mut sb = String::with_capacity(text.len() + 8);
    sb.push('"');
    for c in text.chars() {
        match c {
            '"' => sb.push_str("\\\""),
            '\\' => sb.push_str("\\\\"),
            '\n' => sb.push_str("\\n"),
            '\r' => sb.push_str("\\r"),
            '\t' => sb.push_str("\\t"),
            c if (c as u32) < 0x20 => sb.push_str(&format!("\\u{:04x}", c as u32)),
            c => sb.push(c),
        }
    }
    sb.push('"');
    sb
}

/// Splits the input safely honoring if values are enclosed in quotes.
///
/// Values are trimmed and quotes are removed. See [`split_safe_quote_full`] for details.
///
/// # Arguments
///
/// * `input` - the input
/// * `separator` - the separator char to split the input
///
/// # Returns
///
/// The input split, or the input as a single value if it does not contain the separator.
///
/// # Examples
///
/// ```
/// use camel_util::string_quote_helper::split_safe_quote;
/// assert_eq!(vec!["Hello,Camel", "Bye,World"], split_safe_quote("'Hello,Camel','Bye,World'", ','));
/// ```
pub fn split_safe_quote(input: &str, separator: char) -> Vec<String> {
    split_safe_quote_full(input, separator, true, false, false)
}

/// Splits the input safely honoring if values are enclosed in quotes (quotes are removed).
///
/// See [`split_safe_quote_full`] for details.
///
/// # Arguments
///
/// * `input` - the input
/// * `separator` - the separator char to split the input
/// * `trim` - whether to trim each split value
///
/// # Returns
///
/// The input split, or the input as a single value if it does not contain the separator.
pub fn split_safe_quote_trim(input: &str, separator: char, trim: bool) -> Vec<String> {
    split_safe_quote_full(input, separator, trim, false, false)
}

/// Splits the input safely honoring if values are enclosed in quotes.
///
/// See [`split_safe_quote_full`] for details.
///
/// # Arguments
///
/// * `input` - the input
/// * `separator` - the separator char to split the input
/// * `trim` - whether to trim each split value
/// * `keep_quotes` - whether to keep the quotes in the split values
///
/// # Returns
///
/// The input split, or the input as a single value if it does not contain the separator.
pub fn split_safe_quote_keep(
    input: &str,
    separator: char,
    trim: bool,
    keep_quotes: bool,
) -> Vec<String> {
    split_safe_quote_full(input, separator, trim, keep_quotes, false)
}

/// Splits the input safely honoring if values are enclosed in quotes.
///
/// Though this method does not support double quoting in values, a quoted value must start with
/// the same start and ending quote, which is either a single quote or double quote value.
///
/// Text inside quotes is never trimmed, also when `trim` is enabled; only the whitespace outside
/// the quotes is removed.
///
/// # Arguments
///
/// * `input` - the input
/// * `separator` - the separator char to split the input
/// * `trim` - whether to trim each split value
/// * `keep_quotes` - whether to keep the quotes in the split values
/// * `nested` - whether a separator inside `(...)` or `{...}` should be ignored, so a value such
///   as `${body.substring(0, 3)}` is not split
///
/// # Returns
///
/// The input split, or the input as a single value if it does not contain the separator.
///
/// # Examples
///
/// ```
/// use camel_util::string_quote_helper::split_safe_quote_full;
/// assert_eq!(
///     vec!["${body.substring(0, 3)}", "5"],
///     split_safe_quote_full("${body.substring(0, 3)}, 5", ',', true, true, true)
/// );
/// ```
pub fn split_safe_quote_full(
    input: &str,
    separator: char,
    trim: bool,
    keep_quotes: bool,
    nested: bool,
) -> Vec<String> {
    if !input.contains(separator) {
        let chars: Vec<char> = input.chars().collect();
        let mut result = input.to_string();
        if chars.len() > 1 {
            let ch = chars[0];
            let ch2 = chars[chars.len() - 1];
            let single_quoted = ch == '\'' && ch2 == '\'';
            let double_quoted = ch == '"' && ch2 == '"';
            if !keep_quotes && (single_quoted || double_quoted) {
                result = chars[1..chars.len() - 1].iter().collect();
            } else if trim {
                result = java_trim(input).to_string();
            }
        }
        return vec![result];
    }

    let chars: Vec<char> = input.chars().collect();
    let mut answer: Vec<String> = Vec::new();
    let mut sb: Vec<char> = Vec::with_capacity(chars.len());

    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut separating = false;
    let mut depth = 0;
    let mut quote_start: isize = -1;
    let mut quote_end: isize = -1;

    let to_s = |sb: &Vec<char>| sb.iter().collect::<String>();

    for i in 0..chars.len() {
        let ch = chars[i];
        let prev = if i > 0 { chars[i - 1] } else { '\0' };
        let is_quoting = single_quoted || double_quoted;
        let last = i == chars.len() - 1;

        if !double_quoted && ch == '\'' {
            if single_quoted && prev == ch && sb.is_empty() {
                answer.push(if keep_quotes {
                    "''".into()
                } else {
                    String::new()
                });
            }
            if last {
                if single_quoted && !sb.is_empty() {
                    let text = to_s(&sb);
                    answer.push(if keep_quotes {
                        format!("{text}'")
                    } else {
                        text
                    });
                    sb.clear();
                }
                break;
            }
            single_quoted = !single_quoted;
            if keep_quotes {
                sb.push(ch);
            }
            if single_quoted && quote_start == -1 {
                quote_start = sb.len() as isize;
            } else if !single_quoted {
                quote_end = sb.len() as isize;
            }
            continue;
        } else if !single_quoted && ch == '"' {
            if double_quoted && prev == ch && sb.is_empty() {
                answer.push(if keep_quotes {
                    "\"".into()
                } else {
                    String::new()
                });
            }
            if last {
                if double_quoted && !sb.is_empty() {
                    let text = to_s(&sb);
                    answer.push(if keep_quotes {
                        format!("{text}\"")
                    } else {
                        text
                    });
                    sb.clear();
                }
                break;
            }
            double_quoted = !double_quoted;
            if keep_quotes {
                sb.push(ch);
            }
            if double_quoted && quote_start == -1 {
                quote_start = sb.len() as isize;
            } else if !double_quoted {
                quote_end = sb.len() as isize;
            }
            continue;
        } else if !is_quoting && ch == separator && depth == 0 {
            separating = true;
            if !sb.is_empty() {
                answer.push(if trim {
                    trim_outside_quotes(&sb, quote_start, quote_end)
                } else {
                    to_s(&sb)
                });
                sb.clear();
            }
            quote_start = -1;
            quote_end = -1;
            continue;
        }

        if nested && !is_quoting {
            if ch == '(' || ch == '{' {
                depth += 1;
            } else if (ch == ')' || ch == '}') && depth > 0 {
                depth -= 1;
            }
        }

        if trim && !is_quoting && separating && separator != ' ' && ch == ' ' {
            continue;
        }
        separating = false;
        sb.push(ch);
    }

    if !sb.is_empty() {
        answer.push(if trim {
            trim_outside_quotes(&sb, quote_start, quote_end)
        } else {
            to_s(&sb)
        });
    }
    answer
}

fn trim_outside_quotes(sb: &[char], quote_start: isize, quote_end: isize) -> String {
    if quote_start == -1 {
        return java_trim(&sb.iter().collect::<String>()).to_string();
    }
    let quote_start = quote_start as usize;
    let quote_end = if quote_end < quote_start as isize {
        // an unterminated quote runs to the end of the value
        sb.len()
    } else {
        quote_end as usize
    };
    let mut begin = 0;
    while begin < quote_start && sb[begin] <= ' ' {
        begin += 1;
    }
    let mut end = sb.len();
    while end > quote_end && sb[end - 1] <= ' ' {
        end -= 1;
    }
    sb[begin..end].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // --- camel-util StringQuoteHelperTest ---

    #[test]
    fn test_json_quote_escapes_special_characters() {
        assert_eq!("\"He said \\\"hi\\\"\"", json_quote(Some("He said \"hi\"")));
        assert_eq!("\"line1\\nline2\"", json_quote(Some("line1\nline2")));
        assert_eq!("\"a\\\\b\"", json_quote(Some("a\\b")));
        assert_eq!("null", json_quote(None));
        assert_eq!("\"\\u0001\"", json_quote(Some("\u{1}")));
    }

    #[test]
    fn test_split_bean_parameters_trim() {
        let arr = split_safe_quote_keep("String.class ${body}, String.class Mars", ',', true, true);
        assert_eq!(arr, v(&["String.class ${body}", "String.class Mars"]));
        let arr = split_safe_quote_keep(
            "  String.class ${body}  , String.class Mars   ",
            ',',
            true,
            true,
        );
        assert_eq!(arr, v(&["String.class ${body}", "String.class Mars"]));
    }

    #[test]
    fn test_split_trim_keeps_spaces_inside_quotes() {
        assert_eq!(
            split_safe_quote_keep("' a ', ' b '", ',', true, false),
            v(&[" a ", " b "])
        );
        assert_eq!(
            split_safe_quote_keep("  \" a \"  ,  \" b \"  ,c", ',', true, false),
            v(&[" a ", " b ", "c"])
        );
        assert_eq!(
            split_safe_quote_keep("x, ' b '  ", ',', true, false),
            v(&["x", " b "])
        );
        assert_eq!(
            split_safe_quote_keep(" ' a ' , ' b ' ", ',', true, true),
            v(&["' a '", "' b '"])
        );
        assert_eq!(
            split_safe_quote_keep("'  a  ' b", ' ', true, false),
            v(&["  a  ", "b"])
        );
    }

    #[test]
    fn test_split_bean_parameters_no_trim() {
        let arr =
            split_safe_quote_keep("String.class ${body}, String.class Mars", ',', false, true);
        assert_eq!(arr, v(&["String.class ${body}", " String.class Mars"]));
        let arr = split_safe_quote_keep(
            "  String.class ${body}  , String.class Mars   ",
            ',',
            false,
            true,
        );
        assert_eq!(
            arr,
            v(&["  String.class ${body}  ", " String.class Mars   "])
        );
    }

    #[test]
    fn test_split_nested() {
        let s = |i, t, k, n| split_safe_quote_full(i, ',', t, k, n);
        assert_eq!(
            s("${body.substring(0, 3)}", true, true, true),
            v(&["${body.substring(0, 3)}"])
        );
        assert_eq!(
            s("${body}, ${header.v.replace('a', 'b')}", true, true, true),
            v(&["${body}", "${header.v.replace('a', 'b')}"])
        );
        assert_eq!(
            s(
                "${body.substring(0, 4).substring(1, 3)} , ${replace(a,z,${header.v})}",
                true,
                true,
                true
            ),
            v(&[
                "${body.substring(0, 4).substring(1, 3)}",
                "${replace(a,z,${header.v})}"
            ])
        );
        assert_eq!(
            s("String.class ${body.substring(0, 3)}, 5", true, true, true),
            v(&["String.class ${body.substring(0, 3)}", "5"])
        );
        assert_eq!(s("{a, b}, c", true, true, true), v(&["{a, b}", "c"]));
        assert_eq!(
            s("${body.substring(0, 3)}", true, true, false),
            v(&["${body.substring(0", "3)}"])
        );
        assert_eq!(s("'a(b', c", true, true, true), v(&["'a(b'", "c"]));
        assert_eq!(s("\"{\", c", true, true, true), v(&["\"{\"", "c"]));
        assert_eq!(
            s("a), b, c}, d", true, true, true),
            v(&["a)", "b", "c}", "d"])
        );
        assert_eq!(
            s("f(a, b), 'x, y'", false, true, true),
            v(&["f(a, b)", " 'x, y'"])
        );
        assert_eq!(
            s("f(a, b), 'x, y'", true, false, true),
            v(&["f(a, b)", "x, y"])
        );
    }

    #[test]
    fn test_split_nested_same_as_not_nested_without_comma_in_brackets() {
        let inputs = [
            "${body}, ${header.foo}",
            "'a,b', 5",
            "${body}, ${header.foo?['key']}",
            "*, true",
            "'World'",
            "String.class ${body}, String.class Mars",
            "  String.class ${body}  , String.class Mars   ",
            "null, 'a,b'",
            "'', ${body}",
            "\"\", 'x'",
            "${body.substring(1)}, ${header.foo.toUpperCase()}",
            "a,,b",
            ", a,",
            "'it''s', b",
        ];
        for input in inputs {
            for trim in [true, false] {
                for keep in [true, false] {
                    assert_eq!(
                        split_safe_quote_keep(input, ',', trim, keep),
                        split_safe_quote_full(input, ',', trim, keep, true),
                        "{input}"
                    );
                }
            }
        }
    }

    // --- camel-core StingQuoteHelperTest ---

    #[test]
    fn test_split_safe_quote() {
        assert_eq!(split_safe_quote("", ','), v(&[""]));
        assert_eq!(split_safe_quote("   ", ','), v(&[""]));
        assert_eq!(split_safe_quote_trim("   ", ',', false), v(&["   "]));
        assert_eq!(split_safe_quote("Camel", ','), v(&["Camel"]));
        assert_eq!(split_safe_quote("Hello Camel", ','), v(&["Hello Camel"]));
        assert_eq!(split_safe_quote("Hello,Camel", ','), v(&["Hello", "Camel"]));
        assert_eq!(
            split_safe_quote("Hello,Camel,Bye,World", ','),
            v(&["Hello", "Camel", "Bye", "World"])
        );
        assert_eq!(
            split_safe_quote("'Hello,Camel','Bye,World'", ','),
            v(&["Hello,Camel", "Bye,World"])
        );
        assert_eq!(
            split_safe_quote("'Hello,Camel',\"Bye,World\"", ','),
            v(&["Hello,Camel", "Bye,World"])
        );
        assert_eq!(
            split_safe_quote("\"Hello,Camel\",'Bye,World'", ','),
            v(&["Hello,Camel", "Bye,World"])
        );
        assert_eq!(
            split_safe_quote("\"Hello,Camel\",\"Bye,World\"", ','),
            v(&["Hello,Camel", "Bye,World"])
        );
        assert_eq!(
            split_safe_quote("'Hello Camel', 'Bye World'", ','),
            v(&["Hello Camel", "Bye World"])
        );
        assert_eq!(
            split_safe_quote_trim("'Hello Camel',' Bye World'", ',', false),
            v(&["Hello Camel", " Bye World"])
        );
        assert_eq!(
            split_safe_quote_trim("'http:',' '", ',', false),
            v(&["http:", " "])
        );
        assert_eq!(
            split_safe_quote_trim("'http:',''", ',', false),
            v(&["http:", ""])
        );
        assert_eq!(
            split_safe_quote_trim("'Hello Camel',5,true", ',', false),
            v(&["Hello Camel", "5", "true"])
        );
        assert_eq!(
            split_safe_quote_trim("   'Hello Camel',  5   ,  true   ", ',', true),
            v(&["Hello Camel", "5", "true"])
        );
        assert_eq!(
            split_safe_quote_trim("*, '', 'arg3'", ',', true),
            v(&["*", "", "arg3"])
        );
        assert_eq!(split_safe_quote_trim("'Hello'", ',', true), v(&["Hello"]));
        assert_eq!(split_safe_quote_trim(" Hello ", ',', true), v(&["Hello"]));
        assert_eq!(
            split_safe_quote_trim("' Hello '", ',', true),
            v(&[" Hello "])
        );
        assert_eq!(
            split_safe_quote_trim("' Hello '", ',', false),
            v(&[" Hello "])
        );
        assert_eq!(
            split_safe_quote_trim("'Hello', 'World'", ',', true),
            v(&["Hello", "World"])
        );
        assert_eq!(split_safe_quote_trim("\"Hello\"", ',', true), v(&["Hello"]));
        assert_eq!(
            split_safe_quote_trim("\" Hello \"", ',', true),
            v(&[" Hello "])
        );
        assert_eq!(
            split_safe_quote_trim("\" Hello \"", ',', false),
            v(&[" Hello "])
        );
        assert_eq!(
            split_safe_quote_trim("\"Hello\", \"World\"", ',', true),
            v(&["Hello", "World"])
        );
    }

    #[test]
    fn test_last_is_quote() {
        assert_eq!(
            split_safe_quote_trim(" ${body}, 5, 'Hello World'", ',', true),
            v(&["${body}", "5", "Hello World"])
        );
        assert_eq!(
            split_safe_quote_trim(" ${body}, 5, \"Hello World\"", ',', true),
            v(&["${body}", "5", "Hello World"])
        );
    }

    #[test]
    fn test_single_in_double_quote() {
        assert_eq!(
            split_safe_quote_trim("\"Hello O'Connor\", 5, 'foo bar'", ',', true),
            v(&["Hello O'Connor", "5", "foo bar"])
        );
        assert_eq!(
            split_safe_quote_trim("\"Hello O'Connor O'Bannon\", 5, 'foo bar'", ',', true),
            v(&["Hello O'Connor O'Bannon", "5", "foo bar"])
        );
    }

    #[test]
    fn test_double_in_single_quote() {
        assert_eq!(
            split_safe_quote_trim("'Hello O\"Connor', 5, 'foo bar'", ',', true),
            v(&["Hello O\"Connor", "5", "foo bar"])
        );
        assert_eq!(
            split_safe_quote_trim("'Hello O\"Connor O\"Bannon', 5, 'foo bar'", ',', true),
            v(&["Hello O\"Connor O\"Bannon", "5", "foo bar"])
        );
    }

    #[test]
    fn test_space_separator() {
        assert_eq!(
            split_safe_quote(
                "dependency=mvn:org.my:application:1.0 dependency=mvn:com.foo:myapp:2.1",
                ' '
            ),
            v(&[
                "dependency=mvn:org.my:application:1.0",
                "dependency=mvn:com.foo:myapp:2.1"
            ])
        );
    }

    #[test]
    fn test_space_separator_quote() {
        assert_eq!(
            split_safe_quote(
                "dependency=mvn:org.my:application:1.0 property=hi='Hello World' dependency=mvn:com.foo:myapp:2.1",
                ' '
            ),
            v(&[
                "dependency=mvn:org.my:application:1.0",
                "property=hi=Hello World",
                "dependency=mvn:com.foo:myapp:2.1"
            ])
        );
    }

    #[test]
    fn test_keep_quotes() {
        assert_eq!(
            split_safe_quote_keep("'body'", ',', false, true),
            v(&["'body'"])
        );
        assert_eq!(
            split_safe_quote_keep("'body',123", ',', false, true),
            v(&["'body'", "123"])
        );
        assert_eq!(
            split_safe_quote_keep("'body',\"world\"", ',', false, true),
            v(&["'body'", "\"world\""])
        );
        assert_eq!(
            split_safe_quote_keep("'body',\"world\",123", ',', false, true),
            v(&["'body'", "\"world\"", "123"])
        );
    }
}
