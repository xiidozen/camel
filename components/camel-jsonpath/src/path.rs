//! A JSONPath engine with the semantics of Jayway JsonPath (the library of Java Camel): a
//! definite path (only properties and single indexes) reads one value, any other path (a
//! wildcard, a deep scan, a filter, several names or indexes, a slice) reads the list of the
//! matching values.

use camel_api::error::{CamelError, CamelResult, ErrorKind};
use camel_util::value::{Value, ValueMap};
use std::fmt;

/// The options of the evaluation (Jayway `Option`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathOptions {
    /// A missing leaf of a definite path is null (`DEFAULT_PATH_LEAF_TO_NULL`).
    pub default_path_leaf_to_null: bool,
    /// The result is always a list (`ALWAYS_RETURN_LIST`).
    pub always_return_list: bool,
    /// The errors give null (or an empty list) (`SUPPRESS_EXCEPTIONS`).
    pub suppress_exceptions: bool,
    /// A missing property of an indefinite path is a null value (`REQUIRE_PROPERTIES` is the
    /// opposite: here not supported).
    pub as_path_list: bool,
}

/// An invalid path (Jayway `InvalidPathException`).
fn invalid(message: impl Into<String>) -> CamelError {
    CamelError::new(ErrorKind::IllegalArgument, message.into())
        .with_subtype("com.jayway.jsonpath.InvalidPathException")
}

/// A missing path (Jayway `PathNotFoundException`).
fn not_found(message: impl Into<String>) -> CamelError {
    CamelError::new(ErrorKind::IllegalArgument, message.into())
        .with_subtype("com.jayway.jsonpath.PathNotFoundException")
}

/// A segment of a path.
#[derive(Debug, Clone)]
enum Segment {
    /// `.name` or `['name']`.
    Property(String),
    /// `['a','b']`: a map of the properties.
    Properties(Vec<String>),
    /// `[n]`, `[n,m]`.
    Indexes(Vec<i64>),
    /// `[from:to]`.
    Slice(Option<i64>, Option<i64>),
    /// `*`, `[*]`.
    Wildcard,
    /// `..` followed by the segment.
    Deep(Box<Segment>),
    /// `[?(filter)]`.
    Filter(Box<Filter>),
    /// `.name()` at the end.
    Function(String),
}

/// A compiled path.
#[derive(Debug, Clone)]
pub struct JsonPath {
    text: String,
    segments: Vec<Segment>,
    relative: bool,
}

impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A filter expression.
#[derive(Debug, Clone)]
enum Filter {
    Or(Box<Filter>, Box<Filter>),
    And(Box<Filter>, Box<Filter>),
    Not(Box<Filter>),
    /// An operand alone: whether it exists (and is not false).
    Exists(Operand),
    Compare(Operand, String, Operand),
}

#[derive(Debug, Clone)]
enum Operand {
    /// `@...` (relative) or `$...` (absolute).
    Path(JsonPath),
    Literal(Value),
    Regex(regex::Regex),
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
}

fn is_name_char(c: u8) -> bool {
    !matches!(
        c,
        b'.' | b'['
            | b']'
            | b' '
            | b'('
            | b')'
            | b'='
            | b'!'
            | b'<'
            | b'>'
            | b'&'
            | b'|'
            | b','
            | b'\''
            | b'"'
    )
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            s: text.as_bytes(),
            text,
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn starts_with(&self, p: &str) -> bool {
        self.text[self.pos..].starts_with(p)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn error(&self, what: &str) -> CamelError {
        invalid(format!(
            "{what} at position {} in path {}",
            self.pos, self.text
        ))
    }

    /// A path from the current position (after the optional root `$` or `@`).
    fn path(&mut self, nested: bool) -> CamelResult<JsonPath> {
        let start = self.pos;
        let relative = match self.peek() {
            Some(b'$') => {
                self.pos += 1;
                false
            }
            Some(b'@') => {
                self.pos += 1;
                true
            }
            _ => false,
        };
        let mut segments = Vec::new();
        loop {
            match self.peek() {
                Some(b'.') if self.starts_with("..") => {
                    self.pos += 2;
                    let seg = match self.peek() {
                        Some(b'[') => self.bracket()?,
                        Some(b'*') => {
                            self.pos += 1;
                            Segment::Wildcard
                        }
                        _ => Segment::Property(self.name()?),
                    };
                    segments.push(Segment::Deep(Box::new(seg)));
                }
                Some(b'.') => {
                    self.pos += 1;
                    if self.peek() == Some(b'*') {
                        self.pos += 1;
                        segments.push(Segment::Wildcard);
                    } else if self.peek() == Some(b'[') {
                        segments.push(self.bracket()?);
                    } else {
                        let name = self.name()?;
                        if self.starts_with("()") {
                            self.pos += 2;
                            segments.push(Segment::Function(name));
                        } else {
                            segments.push(Segment::Property(name));
                        }
                    }
                }
                Some(b'[') => segments.push(self.bracket()?),
                _ => break,
            }
        }
        if !nested && self.pos < self.s.len() {
            let rest = self.text[self.pos..].trim();
            if !rest.is_empty() {
                return Err(invalid(format!(
                    "Could not parse token starting at position {}. Expected ?, ', 0-9, * or a property name; path: {} (an operator outside of a filter: a comparison must be written in a filter like [?(@.field == 'value')])",
                    self.pos, self.text
                )));
            }
        }
        Ok(JsonPath {
            text: self.text[start..self.pos].to_string(),
            segments,
            relative,
        })
    }

    fn name(&mut self) -> CamelResult<String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if is_name_char(c)) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.error("Path must not end with a '.' or '..'"));
        }
        Ok(self.text[start..self.pos].to_string())
    }

    fn quoted(&mut self) -> CamelResult<String> {
        let quote = self.peek().ok_or_else(|| self.error("Expected a quote"))?;
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error("Unterminated string")),
                Some(b'\\') => {
                    self.pos += 1;
                    let c = self
                        .peek()
                        .ok_or_else(|| self.error("Unterminated string"))?;
                    out.push(match c {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        other => other as char,
                    });
                    self.pos += 1;
                }
                Some(c) if c == quote => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(_) => {
                    let ch = self.text[self.pos..].chars().next().unwrap();
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> CamelResult<i64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.text[start..self.pos]
            .parse()
            .map_err(|_| self.error("Expected a number"))
    }

    fn bracket(&mut self) -> CamelResult<Segment> {
        self.pos += 1; // [
        self.skip_ws();
        let seg = match self.peek() {
            Some(b'?') => {
                self.pos += 1;
                self.skip_ws();
                let parens = self.peek() == Some(b'(');
                if parens {
                    self.pos += 1;
                }
                let f = self.or()?;
                self.skip_ws();
                if parens {
                    if self.peek() != Some(b')') {
                        return Err(self.error("Expected ')' to close the filter"));
                    }
                    self.pos += 1;
                }
                Segment::Filter(Box::new(f))
            }
            Some(b'*') => {
                self.pos += 1;
                Segment::Wildcard
            }
            Some(b'\'' | b'"') => {
                let mut names = vec![self.quoted()?];
                loop {
                    self.skip_ws();
                    if self.peek() != Some(b',') {
                        break;
                    }
                    self.pos += 1;
                    self.skip_ws();
                    names.push(self.quoted()?);
                }
                if names.len() == 1 {
                    Segment::Property(names.remove(0))
                } else {
                    Segment::Properties(names)
                }
            }
            Some(b':') | Some(b'-' | b'0'..=b'9') => {
                let from = if self.peek() == Some(b':') {
                    None
                } else {
                    Some(self.number()?)
                };
                self.skip_ws();
                if self.peek() == Some(b':') {
                    self.pos += 1;
                    self.skip_ws();
                    let to = if matches!(self.peek(), Some(b'-' | b'0'..=b'9')) {
                        Some(self.number()?)
                    } else {
                        None
                    };
                    Segment::Slice(from, to)
                } else {
                    let mut indexes = vec![from.unwrap_or_default()];
                    loop {
                        self.skip_ws();
                        if self.peek() != Some(b',') {
                            break;
                        }
                        self.pos += 1;
                        self.skip_ws();
                        indexes.push(self.number()?);
                    }
                    Segment::Indexes(indexes)
                }
            }
            _ => return Err(self.error("Expected ?, ', 0-9 or *")),
        };
        self.skip_ws();
        if self.peek() != Some(b']') {
            return Err(self.error("Expected ']'"));
        }
        self.pos += 1;
        Ok(seg)
    }

    fn or(&mut self) -> CamelResult<Filter> {
        let mut left = self.and()?;
        loop {
            self.skip_ws();
            if self.starts_with("||") {
                self.pos += 2;
                let right = self.and()?;
                left = Filter::Or(Box::new(left), Box::new(right));
            } else {
                return Ok(left);
            }
        }
    }

    fn and(&mut self) -> CamelResult<Filter> {
        let mut left = self.unary()?;
        loop {
            self.skip_ws();
            if self.starts_with("&&") {
                self.pos += 2;
                let right = self.unary()?;
                left = Filter::And(Box::new(left), Box::new(right));
            } else {
                return Ok(left);
            }
        }
    }

    fn unary(&mut self) -> CamelResult<Filter> {
        self.skip_ws();
        if self.peek() == Some(b'!') && !self.starts_with("!=") {
            self.pos += 1;
            return Ok(Filter::Not(Box::new(self.unary()?)));
        }
        if self.peek() == Some(b'(') {
            self.pos += 1;
            let f = self.or()?;
            self.skip_ws();
            if self.peek() != Some(b')') {
                return Err(self.error("Expected ')'"));
            }
            self.pos += 1;
            return Ok(f);
        }
        let left = self.operand()?;
        self.skip_ws();
        const OPS: [&str; 15] = [
            "==", "!=", "<=", ">=", "=~", "<", ">", "nin", "in", "subsetof", "anyof", "noneof",
            "size", "empty", "contains",
        ];
        for op in OPS {
            if self.starts_with(op) {
                // a word operator is followed by a blank
                let word = op.chars().all(|c| c.is_ascii_alphabetic());
                if word && !matches!(self.s.get(self.pos + op.len()), Some(b' ')) {
                    continue;
                }
                self.pos += op.len();
                self.skip_ws();
                let right = self.operand()?;
                return Ok(Filter::Compare(left, op.to_string(), right));
            }
        }
        Ok(Filter::Exists(left))
    }

    fn operand(&mut self) -> CamelResult<Operand> {
        self.skip_ws();
        match self.peek() {
            Some(b'@' | b'$') => Ok(Operand::Path(self.path(true)?)),
            Some(b'\'' | b'"') => Ok(Operand::Literal(Value::Text(self.quoted()?))),
            Some(b'/') => {
                self.pos += 1;
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == b'\\' {
                        self.pos += 2;
                        continue;
                    }
                    if c == b'/' {
                        break;
                    }
                    self.pos += 1;
                }
                let pattern = &self.text[start..self.pos.min(self.text.len())];
                self.pos += 1;
                let mut flags = String::new();
                while matches!(self.peek(), Some(b'a'..=b'z')) {
                    flags.push(self.peek().unwrap() as char);
                    self.pos += 1;
                }
                let prefix = if flags.contains('i') { "(?i)" } else { "" };
                let re = regex::Regex::new(&format!("{prefix}^(?:{pattern})$"))
                    .map_err(|e| self.error(&format!("Invalid regex: {e}")))?;
                Ok(Operand::Regex(re))
            }
            Some(b'[' | b'{') => {
                // a JSON literal
                let start = self.pos;
                let mut depth = 0;
                let mut in_string: Option<u8> = None;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    match in_string {
                        Some(q) if c == q => in_string = None,
                        Some(_) if c == b'\\' => self.pos += 1,
                        Some(_) => {}
                        None => match c {
                            b'\'' | b'"' => in_string = Some(c),
                            b'[' | b'{' => depth += 1,
                            b']' | b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        },
                    }
                }
                let raw = self.text[start..self.pos].replace('\'', "\"");
                let v = camel_support::json::parse(&raw)
                    .map_err(|_| self.error("Invalid JSON literal"))?;
                Ok(Operand::Literal(v))
            }
            _ => {
                let start = self.pos;
                while matches!(self.peek(), Some(c) if !matches!(c, b' ' | b')' | b'&' | b'|' | b']'))
                {
                    self.pos += 1;
                }
                let word = &self.text[start..self.pos];
                let v = match word {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    "null" => Value::Null,
                    w => match w.parse::<i64>() {
                        Ok(i) => Value::Int(i),
                        Err(_) => match w.parse::<f64>() {
                            Ok(f) => Value::Float(f),
                            Err(_) => return Err(self.error(&format!("Unexpected token: {w}"))),
                        },
                    },
                };
                Ok(Operand::Literal(v))
            }
        }
    }
}

impl JsonPath {
    /// Compiles the path (Jayway `JsonPath.compile`); a path without root is relative to it
    /// (`$.` is prepended).
    ///
    /// # Errors
    ///
    /// An invalid path.
    pub fn compile(text: &str) -> CamelResult<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(invalid("Path may not be null empty"));
        }
        let full = if trimmed.starts_with('$') || trimmed.starts_with('@') {
            trimmed.to_string()
        } else if trimmed.starts_with('[') || trimmed.starts_with('.') {
            format!("${trimmed}")
        } else {
            format!("$.{trimmed}")
        };
        let mut p = Parser::new(&full);
        let mut path = p.path(false)?;
        path.text = full.clone();
        Ok(path)
    }

    /// Whether the path reads one value (only properties and single indexes).
    pub fn is_definite(&self) -> bool {
        self.segments.iter().all(|s| match s {
            Segment::Property(_) | Segment::Function(_) => true,
            Segment::Indexes(i) => i.len() == 1,
            _ => false,
        })
    }

    /// Reads the value of the path in the JSON (Jayway `read`).
    ///
    /// # Errors
    ///
    /// A missing value of a definite path (`PathNotFoundException`), or a function on a value
    /// of the wrong kind.
    pub fn read(&self, json: &Value, options: PathOptions) -> CamelResult<Value> {
        match self.do_read(json, json, options) {
            Ok(v) => Ok(v),
            Err(_) if options.suppress_exceptions => {
                Ok(if options.always_return_list || !self.is_definite() {
                    Value::List(Vec::new())
                } else {
                    Value::Null
                })
            }
            Err(e) => Err(e),
        }
    }

    fn do_read(&self, root: &Value, current: &Value, options: PathOptions) -> CamelResult<Value> {
        let start = if self.relative { current } else { root };
        let definite = self.is_definite();
        let mut nodes = vec![start.clone()];
        let mut trail = String::from("$");
        let (segments, function) = match self.segments.last() {
            Some(Segment::Function(f)) => {
                (&self.segments[..self.segments.len() - 1], Some(f.clone()))
            }
            _ => (&self.segments[..], None),
        };
        for (i, seg) in segments.iter().enumerate() {
            let last = i + 1 == segments.len();
            let mut next = Vec::new();
            for node in &nodes {
                apply(seg, node, root, &mut next, definite, last, options, &trail)?;
            }
            if let Segment::Property(p) = seg {
                trail.push_str(&format!("['{p}']"));
            }
            nodes = next;
        }
        // a function applies to each node reaching it (Jayway FunctionPathToken)
        if let Some(f) = function {
            nodes = nodes
                .iter()
                .map(|n| function_of(&f, n))
                .collect::<CamelResult<_>>()?;
        }
        Ok(if definite && !options.always_return_list {
            nodes.into_iter().next().unwrap_or(Value::Null)
        } else {
            Value::List(nodes)
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn apply(
    seg: &Segment,
    node: &Value,
    root: &Value,
    out: &mut Vec<Value>,
    definite: bool,
    last: bool,
    options: PathOptions,
    trail: &str,
) -> CamelResult<()> {
    match seg {
        Segment::Property(name) => match node {
            Value::Map(m) => match m.get(name) {
                Some(v) => out.push(v.clone()),
                None if definite => {
                    if last && options.default_path_leaf_to_null {
                        out.push(Value::Null);
                    } else {
                        return Err(not_found(format!("No results for path: {trail}['{name}']")));
                    }
                }
                None => {}
            },
            Value::List(l) if !definite => {
                // a property of the elements of an array
                for item in l {
                    if let Value::Map(m) = item {
                        if let Some(v) = m.get(name) {
                            out.push(v.clone());
                        }
                    }
                }
            }
            other if definite => {
                return Err(not_found(format!(
                    "Expected to find an object with property {trail}['{name}'] in path $ but found '{}'. This is not a json object according to the JsonProvider",
                    java_type(other)
                )))
            }
            _ => {}
        },
        Segment::Properties(names) => {
            if let Value::Map(m) = node {
                let sub: ValueMap = names
                    .iter()
                    .filter_map(|n| m.get(n).map(|v| (n.clone(), v.clone())))
                    .collect();
                out.push(Value::Map(sub));
            }
        }
        Segment::Indexes(indexes) => match node {
            Value::List(l) => {
                for &i in indexes {
                    let idx = if i < 0 { l.len() as i64 + i } else { i };
                    match l.get(idx.max(0) as usize).filter(|_| idx >= 0) {
                        Some(v) => out.push(v.clone()),
                        None if definite => {
                            return Err(not_found(format!("Index out of bounds when evaluating path {trail}[{i}]")))
                        }
                        None => {}
                    }
                }
            }
            other if definite => {
                return Err(not_found(format!(
                    "Filter: [{}] can only be applied to arrays. Current context is: {other}",
                    indexes.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
                )))
            }
            _ => {}
        },
        Segment::Slice(from, to) => {
            if let Value::List(l) = node {
                let len = l.len() as i64;
                let norm = |v: i64| if v < 0 { (len + v).max(0) } else { v.min(len) };
                let a = from.map(norm).unwrap_or(0);
                let b = to.map(norm).unwrap_or(len);
                if a < b {
                    out.extend(l[a as usize..b as usize].iter().cloned());
                }
            }
        }
        Segment::Wildcard => match node {
            Value::Map(m) => out.extend(m.values().cloned()),
            Value::List(l) => out.extend(l.iter().cloned()),
            _ => {}
        },
        Segment::Deep(sub) => {
            let mut all = Vec::new();
            descendants(node, &mut all);
            for d in &all {
                match sub.as_ref() {
                    // a deep filter applies to the elements of the arrays
                    Segment::Filter(_) => {
                        if let Value::List(_) = d {
                            apply(sub, d, root, out, false, last, options, trail)?;
                        }
                    }
                    Segment::Property(name) => {
                        if let Value::Map(m) = d {
                            if let Some(v) = m.get(name) {
                                out.push(v.clone());
                            }
                        }
                    }
                    other => apply(other, d, root, out, false, last, options, trail)?,
                }
            }
        }
        Segment::Filter(f) => match node {
            Value::List(l) => {
                for item in l {
                    if eval_filter(f, root, item, options)? {
                        out.push(item.clone());
                    }
                }
            }
            Value::Map(_) if eval_filter(f, root, node, options)? => out.push(node.clone()),
            _ => {}
        },
        Segment::Function(_) => {}
    }
    Ok(())
}

/// The node and its descendants, depth first.
fn descendants<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(node);
    match node {
        Value::Map(m) => {
            for v in m.values() {
                if matches!(v, Value::Map(_) | Value::List(_)) {
                    descendants(v, out);
                }
            }
        }
        Value::List(l) => {
            for v in l {
                if matches!(v, Value::Map(_) | Value::List(_)) {
                    descendants(v, out);
                }
            }
        }
        _ => {}
    }
}

fn java_type(v: &Value) -> &'static str {
    match v {
        Value::List(_) => "java.util.ArrayList",
        Value::Map(_) => "java.util.LinkedHashMap",
        Value::Text(_) => "java.lang.String",
        Value::Int(_) => "java.lang.Integer",
        Value::Float(_) => "java.lang.Double",
        Value::Bool(_) => "java.lang.Boolean",
        _ => "null",
    }
}

/// The value of an operand: `None` when a path has no value.
fn operand_value(
    o: &Operand,
    root: &Value,
    current: &Value,
    options: PathOptions,
) -> Option<Value> {
    match o {
        Operand::Literal(v) => Some(v.clone()),
        Operand::Regex(_) => None,
        Operand::Path(p) => {
            let definite = p.is_definite();
            match p.do_read(
                root,
                current,
                PathOptions {
                    suppress_exceptions: false,
                    ..options
                },
            ) {
                Ok(Value::List(l)) if !definite && l.is_empty() => None,
                Ok(v) => Some(v),
                Err(_) => None,
            }
        }
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Jayway equality: numbers numerically, texts and the other values as is.
fn equals(a: &Value, b: &Value) -> bool {
    match (as_f64(a), as_f64(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
        _ => as_f64(a)?.partial_cmp(&as_f64(b)?),
    }
}

fn eval_filter(
    f: &Filter,
    root: &Value,
    current: &Value,
    options: PathOptions,
) -> CamelResult<bool> {
    Ok(match f {
        Filter::Or(a, b) => {
            eval_filter(a, root, current, options)? || eval_filter(b, root, current, options)?
        }
        Filter::And(a, b) => {
            eval_filter(a, root, current, options)? && eval_filter(b, root, current, options)?
        }
        Filter::Not(a) => !eval_filter(a, root, current, options)?,
        Filter::Exists(o) => match operand_value(o, root, current, options) {
            None => false,
            Some(Value::Bool(b)) => b,
            Some(_) => true,
        },
        Filter::Compare(l, op, r) => {
            if let (Some(left), Operand::Regex(re)) = (operand_value(l, root, current, options), r)
            {
                return Ok(op == "=~" && matches!(&left, Value::Text(t) if re.is_match(t)));
            }
            let (Some(left), Some(right)) = (
                operand_value(l, root, current, options),
                operand_value(r, root, current, options),
            ) else {
                return Ok(op == "!=" && operand_value(l, root, current, options).is_some());
            };
            match op.as_str() {
                "==" => equals(&left, &right),
                "!=" => !equals(&left, &right),
                "<" => compare(&left, &right) == Some(std::cmp::Ordering::Less),
                "<=" => matches!(
                    compare(&left, &right),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ),
                ">" => compare(&left, &right) == Some(std::cmp::Ordering::Greater),
                ">=" => matches!(
                    compare(&left, &right),
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                ),
                "in" => matches!(&right, Value::List(l) if l.iter().any(|v| equals(v, &left))),
                "nin" => !matches!(&right, Value::List(l) if l.iter().any(|v| equals(v, &left))),
                "contains" => match &left {
                    Value::Text(t) => matches!(&right, Value::Text(s) if t.contains(s.as_str())),
                    Value::List(l) => l.iter().any(|v| equals(v, &right)),
                    _ => false,
                },
                "subsetof" => match (&left, &right) {
                    (Value::List(a), Value::List(b)) => {
                        a.iter().all(|v| b.iter().any(|w| equals(v, w)))
                    }
                    _ => false,
                },
                "anyof" => match (&left, &right) {
                    (Value::List(a), Value::List(b)) => {
                        a.iter().any(|v| b.iter().any(|w| equals(v, w)))
                    }
                    _ => false,
                },
                "noneof" => match (&left, &right) {
                    (Value::List(a), Value::List(b)) => {
                        !a.iter().any(|v| b.iter().any(|w| equals(v, w)))
                    }
                    _ => false,
                },
                "size" => {
                    let n = match &left {
                        Value::List(l) => l.len(),
                        Value::Text(t) => t.chars().count(),
                        _ => return Ok(false),
                    };
                    as_f64(&right) == Some(n as f64)
                }
                "empty" => {
                    let empty = match &left {
                        Value::List(l) => l.is_empty(),
                        Value::Text(t) => t.is_empty(),
                        _ => return Ok(false),
                    };
                    matches!(right, Value::Bool(b) if b == empty)
                }
                _ => false,
            }
        }
    })
}

/// A function at the end of a path (Jayway path functions).
fn function_of(name: &str, value: &Value) -> CamelResult<Value> {
    let numbers = || -> CamelResult<Vec<f64>> {
        match value {
            Value::List(l) => Ok(l.iter().filter_map(as_f64).collect()),
            _ => Err(invalid(
                "Aggregation function attempted to calculate value using empty array",
            )),
        }
    };
    Ok(match name {
        "length" | "size" => match value {
            Value::List(l) => Value::Int(l.len() as i64),
            Value::Map(m) => Value::Int(m.len() as i64),
            Value::Text(t) => Value::Int(t.chars().count() as i64),
            _ => return Err(invalid(format!("Invalid function: {name}() on {value}"))),
        },
        "min" => numbers()?
            .into_iter()
            .reduce(f64::min)
            .map(Value::Float)
            .unwrap_or_default(),
        "max" => numbers()?
            .into_iter()
            .reduce(f64::max)
            .map(Value::Float)
            .unwrap_or_default(),
        "sum" => Value::Float(numbers()?.into_iter().sum()),
        "avg" => {
            let n = numbers()?;
            if n.is_empty() {
                Value::Null
            } else {
                Value::Float(n.iter().sum::<f64>() / n.len() as f64)
            }
        }
        "keys" => match value {
            Value::Map(m) => Value::List(m.keys().map(|k| Value::from(k.as_str())).collect()),
            _ => return Err(invalid("keys() can only be applied to objects")),
        },
        "first" => match value {
            Value::List(l) => l.first().cloned().unwrap_or_default(),
            other => other.clone(),
        },
        "last" => match value {
            Value::List(l) => l.last().cloned().unwrap_or_default(),
            other => other.clone(),
        },
        other => {
            return Err(invalid(format!(
                "Function with name: {other} does not exist."
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Value {
        camel_support::json::parse(
            r#"{"store":{"book":[
              {"category":"reference","author":"Nigel Rees","title":"Sayings of the Century","price":8.95},
              {"category":"fiction","author":"Evelyn Waugh","title":"Sword of Honour","price":12.99,"isbn":"0-553-21311-3"},
              {"category":"fiction","author":"John O'Niel","title":"Moby Dick","price":30}],
              "bicycle":{"color":"red","price":19.95}}}"#,
        )
        .unwrap()
    }

    fn read(path: &str) -> CamelResult<Value> {
        JsonPath::compile(path)?.read(&store(), PathOptions::default())
    }

    #[test]
    fn test_paths() {
        assert_eq!(Value::Float(19.95), read("$.store.bicycle.price").unwrap());
        assert_eq!(Value::Float(19.95), read("store.bicycle.price").unwrap());
        assert_eq!(
            Value::from("Nigel Rees"),
            read("$.store.book[0].author").unwrap()
        );
        assert_eq!(
            Value::from("John O'Niel"),
            read("$.store.book[-1].author").unwrap()
        );
        assert_eq!(
            3,
            read("$.store.book[*].author")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            1,
            read("$.store.book[?(@.price < 10)]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            1,
            read("$..book[?(@.price < 10)]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            1,
            read("$.store.book[?(@.isbn)]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            2,
            read("$.store.book[?(@.category == 'fiction')]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            1,
            read("$.store.book[?(@.author == 'John O\\'Niel')]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            1,
            read("$.store.book[?(@.author == \"John O'Niel\")]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            2,
            read("$.store.book[?(@.price > 10 && @.category == 'fiction')]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(
            1,
            read("$.store.book[?(@.author =~ /.*o.*/i)]")
                .unwrap()
                .as_list()
                .unwrap()
                .len()
        );
        assert_eq!(4, read("$..price").unwrap().as_list().unwrap().len());
        assert_eq!(
            2,
            read("$.store.book[0:2]").unwrap().as_list().unwrap().len()
        );
        assert_eq!(Value::Int(3), read("$.store.book.length()").unwrap());
        assert!(read("$.foo").is_err());
        assert_eq!(
            Value::Null,
            JsonPath::compile("$.foo")
                .unwrap()
                .read(
                    &store(),
                    PathOptions {
                        suppress_exceptions: true,
                        ..Default::default()
                    }
                )
                .unwrap()
        );
        assert!(JsonPath::compile("$.store.book.price < 10").is_err());
    }
}
