//! The tokenizer of the simple language (Java `SimpleTokenizer`, `TokenType`,
//! `SimpleTokenType` and `SimpleToken`).
//!
//! Positions are character indices in the expression (Java used UTF-16 indices; they are the
//! same for text without supplementary characters).

use std::fmt;

/// The kinds of tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum TokenType {
    Ignore,
    WhiteSpace,
    Character,
    BooleanValue,
    NumericValue,
    NullValue,
    SingleQuote,
    DoubleQuote,
    MinusValue,
    Escape,
    FunctionStart,
    FunctionEnd,
    BinaryOperator,
    OtherOperator,
    UnaryOperator,
    LogicalOperator,
    InitOperator,
    InitVariable,
    InitFunctionEnd,
    TernaryOperator,
    ChainOperator,
    Eol,
}

impl fmt::Display for TokenType {
    /// The Java enum constant name (`functionEnd`, `whiteSpace`, ...), used in error messages.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TokenType::Ignore => "ignore",
            TokenType::WhiteSpace => "whiteSpace",
            TokenType::Character => "character",
            TokenType::BooleanValue => "booleanValue",
            TokenType::NumericValue => "numericValue",
            TokenType::NullValue => "nullValue",
            TokenType::SingleQuote => "singleQuote",
            TokenType::DoubleQuote => "doubleQuote",
            TokenType::MinusValue => "minusValue",
            TokenType::Escape => "escape",
            TokenType::FunctionStart => "functionStart",
            TokenType::FunctionEnd => "functionEnd",
            TokenType::BinaryOperator => "binaryOperator",
            TokenType::OtherOperator => "otherOperator",
            TokenType::UnaryOperator => "unaryOperator",
            TokenType::LogicalOperator => "logicalOperator",
            TokenType::InitOperator => "initOperator",
            TokenType::InitVariable => "initVariable",
            TokenType::InitFunctionEnd => "initFunctionEnd",
            TokenType::TernaryOperator => "ternaryOperator",
            TokenType::ChainOperator => "chainOperator",
            TokenType::Eol => "eol",
        };
        f.write_str(s)
    }
}

/// A token type with its text (Java `SimpleTokenType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleTokenType {
    /// The kind.
    pub ty: TokenType,
    /// The text (`None` for end of line).
    pub value: Option<String>,
}

impl SimpleTokenType {
    /// A token type.
    pub fn new(ty: TokenType, value: Option<&str>) -> Self {
        SimpleTokenType {
            ty,
            value: value.map(str::to_string),
        }
    }

    /// The text (empty for end of line).
    pub fn text(&self) -> &str {
        self.value.as_deref().unwrap_or("")
    }

    /// Whether this is a white space.
    pub fn is_whitespace(&self) -> bool {
        self.ty == TokenType::WhiteSpace
    }

    /// Whether this is the end of the expression.
    pub fn is_eol(&self) -> bool {
        self.ty == TokenType::Eol
    }

    /// Whether this is `${` or `$simple{`.
    pub fn is_function_start(&self) -> bool {
        self.ty == TokenType::FunctionStart
    }

    /// Whether this is `}`.
    pub fn is_function_end(&self) -> bool {
        self.ty == TokenType::FunctionEnd
    }

    /// Whether this is a single quote.
    pub fn is_single_quote(&self) -> bool {
        self.ty == TokenType::SingleQuote
    }

    /// Whether this is a double quote.
    pub fn is_double_quote(&self) -> bool {
        self.ty == TokenType::DoubleQuote
    }

    /// Whether this is a unary operator.
    pub fn is_unary(&self) -> bool {
        self.ty == TokenType::UnaryOperator
    }

    /// Whether this is a binary operator.
    pub fn is_binary(&self) -> bool {
        self.ty == TokenType::BinaryOperator
    }

    /// Whether this is a logical operator.
    pub fn is_logical(&self) -> bool {
        self.ty == TokenType::LogicalOperator
    }

    /// Whether this is an other operator (`?:`).
    pub fn is_other(&self) -> bool {
        self.ty == TokenType::OtherOperator
    }

    /// Whether this is a ternary operator.
    pub fn is_ternary(&self) -> bool {
        self.ty == TokenType::TernaryOperator
    }

    /// Whether this is a chain operator.
    pub fn is_chain(&self) -> bool {
        self.ty == TokenType::ChainOperator
    }

    /// Whether this is an init block operator.
    pub fn is_init(&self) -> bool {
        self.ty == TokenType::InitOperator
    }

    /// Whether this is an init block variable.
    pub fn is_init_variable(&self) -> bool {
        self.ty == TokenType::InitVariable
    }

    /// Whether this token is ignored.
    pub fn is_ignore(&self) -> bool {
        self.ty == TokenType::Ignore
    }
}

impl fmt::Display for SimpleTokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

/// A token at a position (Java `SimpleToken`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleToken {
    /// The type and text.
    pub ty: SimpleTokenType,
    /// The position in the expression.
    pub index: usize,
    /// The number of characters consumed (the escaped `\n` is 2 characters for 1 of text).
    pub length: usize,
}

impl SimpleToken {
    /// A token whose length is its text length.
    pub fn new(ty: SimpleTokenType, index: usize) -> Self {
        let length = ty.value.as_ref().map(|v| v.chars().count()).unwrap_or(0);
        SimpleToken { ty, index, length }
    }

    /// A token with an explicit length.
    pub fn with_length(ty: SimpleTokenType, index: usize, length: usize) -> Self {
        SimpleToken { ty, index, length }
    }

    /// The text.
    pub fn text(&self) -> &str {
        self.ty.text()
    }
}

impl fmt::Display for SimpleToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

const NOT_OPERATOR: &str = "!";

/// The known tokens, in the order they are tried.
const KNOWN_TOKENS: &[(TokenType, &str)] = &[
    (TokenType::FunctionStart, "${"),
    (TokenType::FunctionStart, "$simple{"),
    (TokenType::FunctionEnd, "}"),
    (TokenType::WhiteSpace, " "),
    (TokenType::WhiteSpace, "\t"),
    (TokenType::WhiteSpace, "\n"),
    (TokenType::WhiteSpace, "\r"),
    (TokenType::SingleQuote, "'"),
    (TokenType::DoubleQuote, "\""),
    (TokenType::BooleanValue, "true"),
    (TokenType::BooleanValue, "false"),
    (TokenType::NullValue, "null"),
    (TokenType::Escape, "\\"),
    (TokenType::BinaryOperator, "=="),
    (TokenType::BinaryOperator, "=~"),
    (TokenType::BinaryOperator, ">="),
    (TokenType::BinaryOperator, "<="),
    (TokenType::BinaryOperator, ">"),
    (TokenType::BinaryOperator, "<"),
    (TokenType::BinaryOperator, "!=~"),
    (TokenType::BinaryOperator, "!="),
    (TokenType::BinaryOperator, "not is"),
    (TokenType::BinaryOperator, "!is"),
    (TokenType::BinaryOperator, "is"),
    (TokenType::BinaryOperator, "not contains"),
    (TokenType::BinaryOperator, "!contains"),
    (TokenType::BinaryOperator, "contains"),
    (TokenType::BinaryOperator, "!~~"),
    (TokenType::BinaryOperator, "~~"),
    (TokenType::BinaryOperator, "not regex"),
    (TokenType::BinaryOperator, "!regex"),
    (TokenType::BinaryOperator, "regex"),
    (TokenType::BinaryOperator, "not in"),
    (TokenType::BinaryOperator, "!in"),
    (TokenType::BinaryOperator, "in"),
    (TokenType::BinaryOperator, "not range"),
    (TokenType::BinaryOperator, "!range"),
    (TokenType::BinaryOperator, "range"),
    (TokenType::BinaryOperator, "startsWith"),
    (TokenType::BinaryOperator, "starts with"),
    (TokenType::BinaryOperator, "!startsWith"),
    (TokenType::BinaryOperator, "endsWith"),
    (TokenType::BinaryOperator, "ends with"),
    (TokenType::BinaryOperator, "!endsWith"),
    (TokenType::BinaryOperator, "!equals"),
    (TokenType::BinaryOperator, "equals"),
    (TokenType::OtherOperator, "?:"),
    (TokenType::ChainOperator, "~>"),
    (TokenType::ChainOperator, "?~>"),
    (TokenType::UnaryOperator, "++"),
    (TokenType::UnaryOperator, "--"),
    (TokenType::LogicalOperator, "&&"),
    (TokenType::LogicalOperator, "||"),
    (TokenType::TernaryOperator, "?"),
    (TokenType::TernaryOperator, ":"),
    (TokenType::MinusValue, "-"),
    (TokenType::UnaryOperator, "!"),
];

/// Whether the expression has `${` or `$simple{`.
pub fn has_function_start_token(expression: &str) -> bool {
    expression.contains("${") || expression.contains("$simple{")
}

/// Whether the expression has an escape (`\n`, `\t`, `\r`, `\}`).
pub fn has_escape_token(expression: &str) -> bool {
    expression.contains("\\n")
        || expression.contains("\\t")
        || expression.contains("\\r")
        || expression.contains("\\}")
}

/// Splits the expression into tokens.
#[derive(Debug, Default, Clone)]
pub struct SimpleTokenizer {
    not_operator: bool,
}

fn starts_with(text: &[char], value: &str) -> bool {
    value
        .chars()
        .enumerate()
        .all(|(i, c)| text.get(i) == Some(&c))
}

fn accept_type(ty: TokenType, filters: &[TokenType]) -> bool {
    filters.is_empty() || filters.contains(&ty)
}

impl SimpleTokenizer {
    /// A tokenizer; the `!` unary operator is only recognized when enabled (by the predicate
    /// parser).
    pub fn new(not_operator: bool) -> Self {
        SimpleTokenizer { not_operator }
    }

    /// The next token at the index, accepting only the token types of the filter (all if
    /// empty).
    pub fn next_token(
        &self,
        expression: &[char],
        index: usize,
        allow_escape: bool,
        filters: &[TokenType],
    ) -> SimpleToken {
        let mut index = index;
        if accept_type(TokenType::NumericValue, filters) {
            let mut sb = String::new();
            index = reposition_index(expression, index, &mut sb);
            if !sb.is_empty() {
                return SimpleToken::new(
                    SimpleTokenType::new(TokenType::NumericValue, Some(&sb)),
                    index,
                );
            }
        }
        if allow_escape && accept_type(TokenType::Escape, filters) {
            let ch = expression[index];
            if ch == '\\' && index < expression.len() - 1 {
                return from_escaped(expression, index, ch);
            }
        }
        let text = &expression[index..];
        for (ty, value) in KNOWN_TOKENS {
            if *value == NOT_OPERATOR && !self.not_operator {
                continue;
            }
            let token = SimpleTokenType::new(*ty, Some(value));
            if accept_type(*ty, filters) && accept_token(&token, text, expression, index) {
                return SimpleToken::new(token, index);
            }
        }
        let ch = expression[index].to_string();
        SimpleToken::new(SimpleTokenType::new(TokenType::Character, Some(&ch)), index)
    }
}

fn reposition_index(expression: &[char], mut index: usize, sb: &mut String) -> usize {
    let mut digit = true;
    while digit && index < expression.len() {
        digit = expression[index].is_ascii_digit();
        if digit {
            sb.push(expression[index]);
            index += 1;
            continue;
        }
        let decimal_separator = expression[index] == '.' || expression[index] == ',';
        if decimal_separator && !sb.is_empty() {
            sb.push(expression[index]);
            index += 1;
            digit = true;
            continue;
        }
    }
    index
}

fn from_escaped(expression: &[char], index: usize, ch: char) -> SimpleToken {
    let next = expression[index + 1];
    let (text, special) = match next {
        'n' => ("\n".to_string(), true),
        't' => ("\t".to_string(), true),
        'r' => ("\r".to_string(), true),
        '}' => ("}".to_string(), true),
        _ => (ch.to_string(), false),
    };
    SimpleToken::with_length(
        SimpleTokenType::new(TokenType::Character, Some(&text)),
        index + 1,
        if special { 2 } else { 1 },
    )
}

fn accept_token(token: &SimpleTokenType, text: &[char], expression: &[char], index: usize) -> bool {
    let value = token.text();
    if token.is_unary() && starts_with(text, value) {
        return eval_unary(token, text, expression, index);
    }
    if token.is_binary() || token.is_other() || token.is_ternary() || token.is_chain() {
        return eval_surrounded_by_space(token, text, expression, index);
    }
    starts_with(text, value)
}

fn eval_surrounded_by_space(
    token: &SimpleTokenType,
    text: &[char],
    expression: &[char],
    index: usize,
) -> bool {
    let value: Vec<char> = token.text().chars().collect();
    let len = value.len();
    if index < 2 || len + 1 >= text.len() {
        return false;
    }
    expression[index - 1] == ' ' && text[len] == ' ' && text[..len] == value[..]
}

fn eval_unary(token: &SimpleTokenType, text: &[char], expression: &[char], index: usize) -> bool {
    if token.text() == NOT_OPERATOR {
        if !starts_with(text, "!${") {
            return false;
        }
        return index == 0 || expression[index - 1] == ' ' || expression[index - 1] == '(';
    }
    let len = token.text().chars().count();
    let previous_is_function_end = index >= 1 && expression[index - 1] == '}';
    let after = &text[len..];
    let white_space = after.is_empty() || after[0] == ' ';
    previous_is_function_end && white_space
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(expression: &str, not: bool) -> Vec<(TokenType, String)> {
        let chars: Vec<char> = expression.chars().collect();
        let t = SimpleTokenizer::new(not);
        let mut index = 0;
        let mut out = vec![];
        while index < chars.len() {
            let tok = t.next_token(&chars, index, true, &[]);
            index += tok.length;
            out.push((tok.ty.ty, tok.text().to_string()));
        }
        out
    }

    #[test]
    fn tokenize() {
        let t = tokens("Hello ${body} == 'x'", false);
        assert_eq!((TokenType::FunctionStart, "${".to_string()), t[6]);
        assert_eq!((TokenType::FunctionEnd, "}".to_string()), t[11]);
        assert_eq!((TokenType::BinaryOperator, "==".to_string()), t[13]);
        assert_eq!((TokenType::SingleQuote, "'".to_string()), t[15]);
        let n = tokens("12.5 x", false);
        assert_eq!((TokenType::NumericValue, "12.5".to_string()), n[0]);
        let e = tokens("a\\nb\\x", false);
        assert_eq!(
            vec!["a", "\n", "b", "\\", "x"],
            e.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>()
        );
        let u = tokens("${header.c}++ ", false);
        assert_eq!((TokenType::UnaryOperator, "++".to_string()), u[u.len() - 2]);
        assert_eq!(TokenType::Character, tokens("!${body}", false)[0].0);
        assert_eq!(TokenType::UnaryOperator, tokens("!${body}", true)[0].0);
        assert!(has_function_start_token("$simple{x}") && !has_function_start_token("x"));
        assert!(has_escape_token("a\\tb") && !has_escape_token("ab"));
        assert_eq!("functionEnd", TokenType::FunctionEnd.to_string());
    }
}
