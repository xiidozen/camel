//! Hints that turn a simple parser error into a message that says what to write (Java
//! `SimpleSyntaxHints`, CAMEL-24703). Every consumer of the parser (routes at startup, the
//! catalog validator, camel validate, the TUI and the MCP tools) shows the message, so a hint
//! here reaches humans and AI agents alike.
//!
//! It also holds the rewriting of a predicate written inside `${ }` (`${body != null &&
//! body.size() > 0}`, CAMEL-24920 and CAMEL-24921): [`operators_outside`] and
//! [`wrap_functions`].

use regex::Regex;
use std::sync::OnceLock;

/// The operators of simple, as listed in the error messages.
pub const OPERATORS: &str = "==, =~, !=, <, <=, >, >=, contains, !contains, ~~, !~~, regex, !regex, in, !in, is, !is, range, !range, startsWith, endsWith, && and ||";

/// The value forms an operator accepts on its right hand side.
pub const VALUE_FORMS: &str =
    "a quoted literal 'x', a number, true, false, null, or a function ${...}";

/// Function names a model is likely to write, for the did-you-mean suggestion.
pub const FUNCTIONS: &[&str] = &[
    "body",
    "bodyAs",
    "mandatoryBodyAs",
    "bodyOneLine",
    "prettyBody",
    "originalBody",
    "header",
    "headerAs",
    "headers",
    "exchangeProperty",
    "exchangePropertyAs",
    "variable",
    "variableAs",
    "variables",
    "exception",
    "exchange",
    "camelContext",
    "camelId",
    "routeId",
    "routeGroup",
    "stepId",
    "id",
    "messageTimestamp",
    "threadName",
    "threadId",
    "hostname",
    "date",
    "date-with-timezone",
    "random",
    "skip",
    "collate",
    "join",
    "sum",
    "average",
    "min",
    "max",
    "abs",
    "ceil",
    "floor",
    "replace",
    "substring",
    "substringBefore",
    "substringAfter",
    "substringBetween",
    "contains",
    "pad",
    "concat",
    "val",
    "length",
    "size",
    "empty",
    "newEmpty",
    "iif",
    "hash",
    "convertTo",
    "throwException",
    "assert",
    "load",
    "uuid",
    "env",
    "sys",
    "sysenv",
    "ref",
    "bean",
    "properties",
    "propertiesExist",
    "type",
    "messageAs",
    "messageHistory",
    "logExchange",
    "pretty",
    "toJson",
    "toPrettyJson",
    "jq",
    "jsonpath",
    "xpath",
    "simpleJsonpath",
    "semantic",
    "function",
    "list",
    "map",
    "range",
    "split",
    "sort",
    "distinct",
    "reverse",
    "shuffle",
    "forEach",
    "filter",
    "listAdd",
    "listRemove",
    "mapAdd",
    "mapRemove",
    "setHeader",
    "setVariable",
    "uppercase",
    "lowercase",
    "trim",
    "capitalize",
    "normalizeWhitespace",
    "quote",
    "unquote",
    "safeQuote",
    "escape",
    "isEmpty",
    "isAlpha",
    "isAlphaNumeric",
    "isNumeric",
    "not",
    "kindOfType",
    "file",
    "null",
];

/// Functions that are called with parentheses, so a suggestion without arguments adds them.
const CALLED_WITH_PARENTHESES: &[&str] = &["uppercase", "lowercase", "trim", "size", "average"];

/// Functions that delegate to another language, all of them written `${name(exp)}`. Unlike
/// `bean:` or `date:` they take no colon form, which is the mistake CAMEL-24845 is about.
pub const QUERY_FUNCTIONS: &[&str] = &["jq", "jsonpath", "xpath", "simpleJsonpath", "semantic"];

/// Names from older Camel versions or other languages that a model still writes.
pub const ALIASES: &[(&str, &str)] = &[
    ("property", "exchangeProperty"),
    ("in.body", "body"),
    ("out.body", "body"),
    ("in.header", "header"),
    ("out.header", "header"),
    ("in.headers", "headers"),
    ("var", "variable"),
    ("prop", "exchangeProperty"),
    ("json", "jsonpath"),
    ("upper", "uppercase"),
    ("toUpperCase", "uppercase"),
    ("lower", "lowercase"),
    ("toLowerCase", "lowercase"),
    ("padding", "pad"),
    ("now", "date:now:yyyy-MM-dd'T'HH:mm:ss"),
    ("timestamp", "messageTimestamp"),
    ("avg", "average"),
    ("count", "size"),
];

const OPERATOR_WORDS: &[&str] = &[
    "==",
    "!=",
    ">=",
    "<=",
    ">",
    "<",
    "=~",
    "!=~",
    "~~",
    "!~~",
    "contains",
    "!contains",
    "regex",
    "!regex",
    "in",
    "!in",
    "is",
    "!is",
    "range",
    "!range",
    "startsWith",
    "endsWith",
    "&&",
    "||",
];

/// The comparison operators, with the spaces they must be surrounded by.
const SPACED_OPERATORS: &[&str] = &[
    " >= ",
    " <= ",
    " > ",
    " < ",
    " == ",
    " != ",
    " =~ ",
    " !=~ ",
    " contains ",
    " !contains ",
    " ~~ ",
    " !~~ ",
    " regex ",
    " !regex ",
    " in ",
    " !in ",
    " is ",
    " !is ",
    " range ",
    " !range ",
    " startsWith ",
    " !startsWith ",
    " endsWith ",
    " !endsWith ",
];

fn alias(name: &str) -> Option<&'static str> {
    ALIASES.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

/// The whole word (up to whitespace on both sides) around the char index in the expression.
pub fn word_at(expression: &str, index: usize) -> String {
    let chars: Vec<char> = expression.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let mut start = index.min(chars.len());
    let mut end = start;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    chars[start..end].iter().collect()
}

/// The same negation with its function in braces, or `None` when the text is not a negated
/// name: `!x` is `!${x}`. The parser otherwise reports it as a missing predicate next to the
/// operator (CAMEL-24984).
fn negated_function(word: &str) -> Option<String> {
    if word.chars().count() < 2 || !word.starts_with('!') || word.starts_with("!${") {
        return None;
    }
    let rest = &word[1..];
    // !=, !contains and the other negated operators are words of their own, not a negated
    // function
    if rest.starts_with('=') || rest.starts_with('$') {
        return None;
    }
    if [
        "contains",
        "endsWith",
        "equals",
        "in",
        "is",
        "range",
        "regex",
        "startsWith",
        "~~",
    ]
    .contains(&rest)
    {
        return None;
    }
    Some(format!("!${{{rest}}}"))
}

/// The message for a token the grammar does not know at the given char index.
pub fn unexpected_token(expression: &str, index: usize) -> String {
    let word = word_at(expression, index);
    if word.is_empty() {
        return format!("Unexpected token at location {index}");
    }
    match word.to_lowercase().as_str() {
        "=" | "===" | "=<" | "=>" => {
            return format!("Unknown operator {word}: did you mean ==? Operators are {OPERATORS}")
        }
        "<>" | "=!=" | "!==" | "~=" => {
            return format!("Unknown operator {word}: did you mean !=? Operators are {OPERATORS}")
        }
        "and" | "&" => return format!("Unknown operator {word}: use && for and, and || for or"),
        "or" | "|" => return format!("Unknown operator {word}: use || for or, and && for and"),
        "not" | "!" => {
            return format!(
                "Unknown operator {word}: ! negates a function and is written directly in front of it, e.g. !${{body.isEmpty()}}; a comparison negates its operator instead, e.g. != or !contains"
            )
        }
        _ => {}
    }
    let name = function_name(&word);
    if is_known_function(name) {
        let rewrite = expression.replace(&word, &format!("${{{word}}}"));
        return format!(
            "Unexpected token {word}: text outside ${{...}} is a literal, functions are written as ${{body}}, ${{header.name}}; did you mean {rewrite}?"
        );
    }
    format!(
        "Unexpected token {word}: text outside ${{...}} must be {VALUE_FORMS}, or an operator ({OPERATORS})"
    )
}

/// The message when an operator has no usable value next to it.
///
/// # Arguments
///
/// * `kind` - the kind of operator (`Binary`, `Logical`, `Ternary`, `Other`, `Chain`)
/// * `operator` - the operator as written
/// * `expression` - the expression
/// * `index` - the char index of the operand
pub fn unsupported_operand(kind: &str, operator: &str, expression: &str, index: usize) -> String {
    let word = word_at(expression, index);
    if let Some(negated) = negated_function(&word) {
        return format!(
            "! negates a function, which is written as ${{ }}: {word} is written as {negated}"
        );
    }
    if kind == "Logical" {
        let was = if word.is_empty() {
            String::new()
        } else {
            format!("; was: {word}")
        };
        return format!(
            "{kind} operator {operator} needs a predicate on the right hand side, e.g. ${{header.foo}} == 'bar'{was}"
        );
    }
    if word.is_empty() || word == operator {
        return format!(
            "{kind} operator {operator} needs a value on the right hand side: {VALUE_FORMS}"
        );
    }
    let example = if is_known_function(function_name(&word)) {
        format!(", e.g. ${{{word}}}")
    } else {
        format!(", e.g. '{word}'")
    };
    format!(
        "{kind} operator {operator} does not accept {word} on the right hand side: write it as {VALUE_FORMS}{example}"
    )
}

/// Wraps the function references of a predicate written inside `${ }` so that it can be
/// parsed as one: `body != null && body.size() > 0` becomes `${body} != null && ${body.size()}
/// > 0`.
///
/// Each comparison is wrapped, not only the first, so that a compound condition reads the way
/// it looks (CAMEL-24920, CAMEL-24921). An operator counts only when whitespace surrounds it
/// outside quotes, which is what keeps `${header.Content-Length}` and
/// `${date:now:yyyy-MM-dd}` a plain function.
///
/// # Examples
///
/// ```
/// use camel_core_languages::simple::syntax_hints::wrap_functions;
///
/// assert_eq!(
///     "${body} != null && ${body.size()} > 0",
///     wrap_functions("body != null && body.size() > 0")
/// );
/// ```
pub fn wrap_functions(text: &str) -> String {
    let mut answer = String::new();
    let mut from = 0;
    while let Some(at) = logical_operator(text, from) {
        // the operator matched with its trailing space, so the next space is at most two
        // characters away
        let end = text[at + 1..]
            .find(' ')
            .map(|i| i + at + 1)
            .unwrap_or(text.len());
        answer.push_str(&wrap_comparison(text[from..at].trim()));
        answer.push(' ');
        answer.push_str(&text[at..end]);
        answer.push(' ');
        from = (end + 1).min(text.len());
    }
    answer.push_str(&wrap_comparison(text[from..].trim()));
    answer
}

/// The byte index of the next logical operator (`&&` or `||`) outside quotes. Simple has no
/// word forms: `and` and `or` are refused by the parser with a message that says so.
fn logical_operator(text: &str, from: usize) -> Option<usize> {
    let mut single = false;
    let mut double = false;
    for (i, c) in text[from..].char_indices() {
        let i = i + from;
        if c == '\'' && !double {
            single = !single;
        } else if c == '"' && !single {
            double = !double;
        } else if !single && !double && c == ' ' {
            for op in ["&& ", "|| "] {
                if text[i + 1..].starts_with(op) {
                    return Some(i + 1);
                }
            }
        }
    }
    None
}

/// Wraps the left hand side of one comparison with `${ }` when it is a function reference.
fn wrap_comparison(text: &str) -> String {
    // the first operator outside quotes, so ${body == 'a > b'} is compared with ==
    let mut first: Option<(&str, usize)> = None;
    for op in SPACED_OPERATORS {
        if let Some(at) = index_outside_quotes(text, op) {
            if at > 0 && first.map_or(true, |(_, f)| at < f) {
                first = Some((op, at));
            }
        }
    }
    if let Some((op, at)) = first {
        return wrap_comparison_at(text, op, at);
    }
    for op in SPACED_OPERATORS {
        let trimmed = op.trim_end();
        if text.ends_with(trimmed) {
            // the operator ends the text: wrap what is there, so the parser says what is
            // missing after it
            let at = text.len() - trimmed.len();
            if at > 0 {
                return wrap_comparison_at(text, op, at);
            }
        }
    }
    if text.chars().count() > 1 && text.starts_with('!') && !text.starts_with("!${") {
        // a negated function on its own, such as !body.isEmpty(): the predicate parser reads
        // a ! in front of a ${ }, so give it that form (CAMEL-24984)
        return format!("!${{{}}}", text[1..].trim());
    }
    text.to_string()
}

fn wrap_comparison_at(text: &str, op: &str, at: usize) -> String {
    let mut left = text[..at].trim().to_string();
    let right = if at + op.len() <= text.len() {
        text[at + op.len()..].trim()
    } else {
        ""
    };
    if !left.starts_with("${")
        && !left.starts_with('\'')
        && !left.starts_with('"')
        && !is_numeric(&left)
        && !left.eq_ignore_ascii_case("true")
        && !left.eq_ignore_ascii_case("false")
        && !left.eq_ignore_ascii_case("null")
    {
        left = format!("${{{left}}}");
    }
    format!("{left}{op}{right}")
}

/// The byte index of the text outside single and double quotes.
fn index_outside_quotes(text: &str, find: &str) -> Option<usize> {
    let mut single = false;
    let mut double = false;
    for (i, c) in text.char_indices() {
        if c == '\'' && !double {
            single = !single;
        } else if c == '"' && !single {
            double = !double;
        } else if !single && !double && text[i..].starts_with(find) {
            return Some(i);
        }
    }
    None
}

/// Java `Double.parseDouble` succeeds (without the `d`/`f` suffixes and hex forms).
fn is_numeric(text: &str) -> bool {
    !text.is_empty() && text.trim().parse::<f64>().is_ok()
}

/// When an operator is written inside the function (`${body == 'x'}`), the rewrite with the
/// operator outside (`${body} == 'x'`), else `None`.
///
/// # Examples
///
/// ```
/// use camel_core_languages::simple::syntax_hints::operators_outside;
///
/// assert_eq!(Some("${body} == 'x'".to_string()), operators_outside("body == 'x'"));
/// assert_eq!(None, operators_outside("iif(${body} == 'x', 'a', 'b')"));
/// ```
pub fn operators_outside(function: &str) -> Option<String> {
    let mut best: Option<usize> = None;
    for op in OPERATOR_WORDS {
        if let Some(i) = function.find(&format!(" {op} ")) {
            if i > 0 && best.map_or(true, |b| i < b) {
                best = Some(i);
            }
        }
    }
    let best = best?;
    let head = &function[..best];
    if head.contains('\'') || head.contains('"') {
        return None;
    }
    let mut open = 0i32;
    for c in head.chars() {
        match c {
            '(' | '[' => open += 1,
            ')' | ']' => open -= 1,
            _ => {}
        }
    }
    if open > 0 {
        // the operator is inside an argument list that may hold a predicate (iif, filter,
        // forEach), or inside a key such as ${header[order in progress]}
        return None;
    }
    if head.starts_with("properties:") && head.get(11..).is_some_and(|r| r.contains(':')) {
        // the operator word is in the default value: ${properties:msg:value is not set}
        return None;
    }
    Some(format!("${{{head}}}{}", &function[best..]))
}

fn regex_of(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// A hint for an unknown function, or `None`.
///
/// # Examples
///
/// ```
/// use camel_core_languages::simple::syntax_hints::unknown_function;
///
/// assert_eq!(Some("did you mean ${pad(3)}?".to_string()), unknown_function("padding(3)"));
/// assert!(unknown_function("Body").unwrap().contains("case sensitive: ${body}"));
/// ```
pub fn unknown_function(function: &str) -> Option<String> {
    let trimmed = function.trim();
    if trimmed != function {
        return Some(format!("remove the spaces: ${{{trimmed}}}"));
    }
    if let Some(rewrite) = operators_outside(function) {
        return Some(format!("operators go outside the function: {rewrite}"));
    }
    let bare = match function.find('(') {
        Some(i) => &function[..i],
        None => function,
    };
    let bare = match bare.find('.') {
        Some(i) => &bare[..i],
        None => bare,
    };
    if bare == "simple" {
        // ${simple} in a log message or a simple: expression: the text is already simple
        return Some("simple is the language, not a function: the text is already a simple expression, write the values with ${body}, ${header.name}, ${date:now:HH:mm:ss} and leave the rest as plain text".to_string());
    }
    static LANGUAGES: OnceLock<Regex> = OnceLock::new();
    if regex_of(
        &LANGUAGES,
        "^(?:groovy|xquery|mvel|ognl|spel|js|python|java|constant|tokenize|method)$",
    )
    .is_match(bare)
    {
        return Some(format!("{bare} is a language, not a simple function: another language cannot be nested inside ${{...}}; write the expression with its own key, for example {bare}: \"...\""));
    }
    // ${jsonpath:$.status}: written the way bean: and date: are, but these functions take
    // parentheses (CAMEL-24845)
    if let Some(colon) = function.find(':') {
        if colon > 0 && QUERY_FUNCTIONS.contains(&&function[..colon]) {
            return Some(parentheses(&function[..colon], &function[colon + 1..]));
        }
    }
    if QUERY_FUNCTIONS.contains(&bare) {
        // ${jsonpath}: a simple function of its own since QueryLanguageFunctionFactory, not a
        // nested language
        return Some(parentheses(bare, "exp"));
    }
    static ARITHMETIC: OnceLock<Regex> = OnceLock::new();
    if regex_of(&ARITHMETIC, r"(?s)^.*\S\s+[-+*/%]\s+\S.*$").is_match(function) {
        // ${exchangeCounter % 3}, ${header.total * 2}: there is no arithmetic in simple
        return Some("simple has no arithmetic operators (+ - * / %): compute the value in a groovy expression (groovy: \"...\") or in a bean, and use the result here".to_string());
    }
    let name = function_name(function);
    let mut rest = function[name.len()..].to_string();
    let mut found = alias(name).map(str::to_string);
    if found.is_none() {
        if let Some(dot) = name.find('.') {
            found = alias(&name[..dot]).map(|a| format!("{a}{}", &name[dot..]));
        }
    }
    if let Some(a) = found {
        if rest.is_empty() && CALLED_WITH_PARENTHESES.contains(&a.as_str()) {
            rest = "()".to_string();
        }
        if let Some(r) = rest.strip_prefix(':') {
            if QUERY_FUNCTIONS.contains(&a.as_str()) {
                // ${json:$.status}: the alias resolves to jsonpath, so the argument moves into
                // parentheses too
                return Some(parentheses(&a, r));
            }
        }
        return Some(format!("did you mean ${{{a}{rest}}}?"));
    }
    for f in FUNCTIONS {
        if f.eq_ignore_ascii_case(name) && *f != name {
            return Some(format!("function names are case sensitive: ${{{f}{rest}}}"));
        }
    }
    if let Some(c) = closest(name) {
        if c != name {
            // a suggestion that repeats the rejected text teaches nothing, and the reader
            // writes it again (CAMEL-24845)
            return Some(format!("did you mean ${{{c}{rest}}}?"));
        }
    }
    Some("the functions are documented on the simple language page (functions)".to_string())
}

/// The did-you-mean for a function whose argument was written after a colon instead of in
/// parentheses.
fn parentheses(name: &str, argument: &str) -> String {
    let argument = if name == "semantic" {
        format!(
            "'{}'",
            if argument == "exp" {
                "evaluationName"
            } else {
                argument
            }
        )
    } else {
        argument.to_string()
    };
    format!("the argument goes in parentheses: did you mean ${{{name}({argument})}}?")
}

/// The name of the function: `header` of `header.foo`, `bodyAs` of `bodyAs(String)`; `in.body`
/// keeps its dot.
///
/// # Examples
///
/// ```
/// use camel_core_languages::simple::syntax_hints::function_name;
///
/// assert_eq!("header", function_name("header.foo"));
/// assert_eq!("bodyAs", function_name("bodyAs(String)"));
/// assert_eq!("in.body", function_name("in.body"));
/// ```
pub fn function_name(function: &str) -> &str {
    let keep_dot = function.starts_with("in.")
        || function.starts_with("out.")
        || function.starts_with("date-with");
    let end = function
        .char_indices()
        .find(|&(_, ch)| {
            matches!(ch, '(' | ':' | '[' | '?') || ch.is_whitespace() || (ch == '.' && !keep_dot)
        })
        .map(|(i, _)| i)
        .unwrap_or(function.len());
    &function[..end]
}

/// Whether the name is a known function (or one of its aliases).
pub fn is_known_function(name: &str) -> bool {
    !name.is_empty() && (FUNCTIONS.contains(&name) || alias(name).is_some())
}

/// The known function closest to the name: the longest known prefix (`bodyxxx` is `body`),
/// else the nearest by edit distance within a third of the length.
pub fn closest(name: &str) -> Option<&'static str> {
    if name.chars().count() < 3 {
        return None;
    }
    // bodyxxx, headerz: a known function name followed by something that is not OGNL
    let mut prefix: Option<&'static str> = None;
    for f in FUNCTIONS {
        if f.len() >= 3 && name.starts_with(f) && prefix.map_or(true, |p| f.len() > p.len()) {
            prefix = Some(f);
        }
    }
    if prefix.is_some() {
        return prefix;
    }
    let mut best: Option<&'static str> = None;
    let mut best_distance = usize::MAX;
    let threshold = 2.max(name.chars().count() / 3);
    let lower = name.to_lowercase();
    for f in FUNCTIONS {
        let d = distance(&lower, &f.to_lowercase());
        if d <= threshold
            && (d < best_distance
                || (d == best_distance && best.is_some_and(|b| f.len() < b.len())))
        {
            best = Some(f);
            best_distance = d;
        }
    }
    best
}

/// The Levenshtein distance of the two texts.
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (cur[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helpers() {
        assert_eq!("x", word_at("${body} == x", 12));
        assert_eq!("and", word_at("${body} == 'x' and ${header.y} == 1", 15));
        assert_eq!("header", function_name("header.foo"));
        assert_eq!("bodyAs", function_name("bodyAs(String)"));
        assert_eq!("in.body", function_name("in.body"));
        assert_eq!(None, operators_outside("iif(${body} == 'x', 'a', 'b')"));
        assert_eq!(
            Some("${body} == 'x'".to_string()),
            operators_outside("body == 'x'")
        );
    }

    #[test]
    fn test_a_suggestion_never_repeats_the_rejected_text() {
        // CAMEL-24845: bean and date do take a colon, so there is no parenthesis form to
        // suggest for them, and the did-you-mean must not degenerate into the input
        assert!(!unknown_function("bean:myBean")
            .unwrap()
            .contains("${bean:myBean}"));
        assert!(!unknown_function("date:now:HH:mm")
            .unwrap()
            .contains("${date:now:HH:mm}"));
        assert!(!unknown_function("jsonpath:$.status")
            .unwrap()
            .contains("${jsonpath:$.status}"));
    }

    #[test]
    fn test_wrap_functions() {
        assert_eq!(
            "${body} != null && ${body.size()} > 0",
            wrap_functions("body != null && body.size() > 0")
        );
        assert_eq!("${body} == 'a > b'", wrap_functions("body == 'a > b'"));
        assert_eq!("!${body.isEmpty()}", wrap_functions("!body.isEmpty()"));
        assert_eq!("${body} == ", wrap_functions("body =="));
        assert_eq!("${header.foo} > 0", wrap_functions("${header.foo} > 0"));
    }

    #[test]
    fn test_distance_and_closest() {
        assert_eq!(1, distance("bdy", "body"));
        assert_eq!(Some("body"), closest("bodyy"));
        assert_eq!(Some("uppercase"), closest("uppercse"));
        assert_eq!(None, closest("ab"));
    }
}
