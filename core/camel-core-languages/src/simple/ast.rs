//! The nodes of a parsed simple expression (Java `org.apache.camel.language.simple.ast`), and
//! how they become expressions.

use super::functions::create_function;
use super::tokenizer::{SimpleToken, TokenType};
use super::{ParseResult, ParserError};
use camel_api::error::{CamelError, CamelResult};
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, SharedExpression};
use camel_support::builder::{constant_expression, named_expression};
use camel_util::value::Value;
use std::fmt;
use std::sync::Arc;

/// The unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    /// `++`
    Inc,
    /// `--`
    Dec,
    /// `!`
    Not,
}

impl UnaryOperator {
    fn of(text: &str) -> Option<Self> {
        match text {
            "++" => Some(UnaryOperator::Inc),
            "--" => Some(UnaryOperator::Dec),
            "!" => Some(UnaryOperator::Not),
            _ => None,
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnaryOperator::Inc => "++",
            UnaryOperator::Dec => "--",
            UnaryOperator::Not => "!",
        })
    }
}

/// A node of the parsed expression.
#[derive(Debug, Clone)]
pub enum Node {
    /// Literal text.
    Literal {
        /// The first token.
        token: SimpleToken,
        /// The text.
        text: String,
    },
    /// A function `${...}` with its nested nodes.
    FunctionStart {
        /// The `${` token.
        token: SimpleToken,
        /// The nested nodes.
        children: Vec<Node>,
    },
    /// The `}` closing a function.
    FunctionEnd {
        /// The token.
        token: SimpleToken,
    },
    /// A single quoted literal `'...'` with its nested nodes.
    SingleQuoteStart {
        /// The token.
        token: SimpleToken,
        /// The nested nodes.
        children: Vec<Node>,
    },
    /// The closing single quote.
    SingleQuoteEnd {
        /// The token.
        token: SimpleToken,
    },
    /// A double quoted literal `"..."` with its nested nodes.
    DoubleQuoteStart {
        /// The token.
        token: SimpleToken,
        /// The nested nodes.
        children: Vec<Node>,
    },
    /// The closing double quote.
    DoubleQuoteEnd {
        /// The token.
        token: SimpleToken,
    },
    /// A unary operator with its operand.
    Unary {
        /// The token.
        token: SimpleToken,
        /// The operator.
        operator: UnaryOperator,
        /// The operand.
        operand: Option<Box<Node>>,
    },
    /// A binary operator of a predicate (see the predicate parser).
    Binary {
        /// The token.
        token: SimpleToken,
        /// The left operand.
        left: Option<Box<Node>>,
        /// The right operand.
        right: Option<Box<Node>>,
    },
    /// A logical operator of a predicate.
    Logical {
        /// The token.
        token: SimpleToken,
        /// The left operand.
        left: Option<Box<Node>>,
        /// The right operand.
        right: Option<Box<Node>>,
    },
    /// A numeric value of a predicate.
    Numeric {
        /// The token.
        token: SimpleToken,
        /// The number text.
        text: String,
    },
    /// A boolean value of a predicate.
    Boolean {
        /// The token.
        token: SimpleToken,
    },
    /// `null` in a predicate.
    Null {
        /// The token.
        token: SimpleToken,
    },
    /// An operator that the Rust port does not support yet (chain `~>`, other `?:`, ternary,
    /// init blocks).
    Unsupported {
        /// The token.
        token: SimpleToken,
    },
}

impl Node {
    /// The token of the node.
    pub fn token(&self) -> &SimpleToken {
        match self {
            Node::Literal { token, .. }
            | Node::FunctionStart { token, .. }
            | Node::FunctionEnd { token }
            | Node::SingleQuoteStart { token, .. }
            | Node::SingleQuoteEnd { token }
            | Node::DoubleQuoteStart { token, .. }
            | Node::DoubleQuoteEnd { token }
            | Node::Unary { token, .. }
            | Node::Binary { token, .. }
            | Node::Logical { token, .. }
            | Node::Numeric { token, .. }
            | Node::Boolean { token }
            | Node::Null { token }
            | Node::Unsupported { token } => token,
        }
    }

    /// Whether this node starts a block (function or quotes).
    pub fn is_block_start(&self) -> bool {
        matches!(
            self,
            Node::FunctionStart { .. }
                | Node::SingleQuoteStart { .. }
                | Node::DoubleQuoteStart { .. }
        )
    }

    /// Whether this node ends a block.
    pub fn is_block_end(&self) -> bool {
        matches!(
            self,
            Node::FunctionEnd { .. } | Node::SingleQuoteEnd { .. } | Node::DoubleQuoteEnd { .. }
        )
    }

    /// Whether this is a literal.
    pub fn is_literal(&self) -> bool {
        matches!(self, Node::Literal { .. })
    }

    /// Adds the node to this block, if the block accepts it (Java `acceptAndAddNode`).
    pub fn accept_and_add(&mut self, node: Node) -> Result<(), Node> {
        match self {
            Node::FunctionStart { children, .. } => {
                if matches!(
                    node,
                    Node::Literal { .. }
                        | Node::FunctionStart { .. }
                        | Node::SingleQuoteStart { .. }
                        | Node::DoubleQuoteStart { .. }
                ) {
                    children.push(node);
                    Ok(())
                } else {
                    Err(node)
                }
            }
            Node::SingleQuoteStart { children, .. } | Node::DoubleQuoteStart { children, .. } => {
                children.push(node);
                Ok(())
            }
            _ => Err(node),
        }
    }

    /// Creates the expression of the node.
    ///
    /// # Errors
    ///
    /// Returns a syntax error (such as an unknown function).
    pub fn create_expression(&self, expression: &str) -> ParseResult<SharedExpression> {
        match self {
            Node::Literal { text, .. } => Ok(constant_expression(text.as_str())),
            Node::FunctionStart { token, children } => {
                create_function_block(token, children, expression)
            }
            Node::SingleQuoteStart { children, .. } | Node::DoubleQuoteStart { children, .. } => {
                create_quoted(children, expression)
            }
            Node::Unary {
                token,
                operator,
                operand,
            } => {
                let operand = operand.as_ref().ok_or_else(|| {
                    ParserError::new(
                        "left node must be specified on: ".to_string() + &self.to_string(),
                        token.index,
                    )
                })?;
                let exp = operand.create_expression(expression)?;
                Ok(create_unary(*operator, exp, operand.to_string()))
            }
            Node::Numeric { text, .. } => Ok(constant_expression(numeric_value(text))),
            Node::Boolean { token } => Ok(constant_expression(token.text() == "true")),
            Node::Null { .. } => Ok(constant_expression(Value::Null)),
            Node::Unsupported { token } => Err(ParserError::new(
                format!(
                    "The operator {} is not supported yet by the Rust port of the simple language",
                    token.text()
                ),
                token.index,
            )),
            Node::FunctionEnd { .. }
            | Node::SingleQuoteEnd { .. }
            | Node::DoubleQuoteEnd { .. } => Ok(constant_expression(Value::Null)),
            Node::Binary { token, left, right } => {
                let (l, r) = operands(token, left, right, "Binary")?;
                super::predicate_parser::create_binary(token, l, r, expression)
            }
            Node::Logical { token, left, right } => {
                let (l, r) = operands(token, left, right, "Logical")?;
                super::predicate_parser::create_logical(token, l, r, expression)
            }
        }
    }
}

fn operands<'a>(
    token: &SimpleToken,
    left: &'a Option<Box<Node>>,
    right: &'a Option<Box<Node>>,
    kind: &str,
) -> ParseResult<(&'a Node, &'a Node)> {
    match (left, right) {
        (Some(l), Some(r)) => Ok((l, r)),
        _ => Err(ParserError::new(
            format!("{kind} operator {} has no operands", token.text()),
            token.index,
        )),
    }
}

/// The value of a numeric literal: an integer, else a decimal (Java `NumericExpression`).
pub fn numeric_value(text: &str) -> Value {
    if let Ok(i) = text.parse::<i64>() {
        return Value::Int(i);
    }
    match text.replace(',', ".").parse::<f64>() {
        Ok(f) => Value::Float(f),
        Err(_) => Value::from(text),
    }
}

impl fmt::Display for Node {
    /// Java `toString()` of the nodes: functions as `${...}`, literals as their text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Literal { text, .. } => f.write_str(text),
            Node::FunctionStart { children, .. } => {
                f.write_str("${")?;
                for c in children {
                    write!(f, "{c}")?;
                }
                f.write_str("}")
            }
            Node::SingleQuoteStart { children, .. } => {
                f.write_str("'")?;
                for c in children {
                    write!(f, "{c}")?;
                }
                f.write_str("'")
            }
            Node::DoubleQuoteStart { children, .. } => {
                f.write_str("\"")?;
                for c in children {
                    write!(f, "{c}")?;
                }
                f.write_str("\"")
            }
            Node::Unary { token, operand, .. } => match operand {
                Some(o) => write!(f, "{o}{}", token.text()),
                None => f.write_str(token.text()),
            },
            Node::Binary { token, left, right } | Node::Logical { token, left, right } => {
                let l = left.as_ref().map(|n| n.to_string()).unwrap_or_default();
                let r = right.as_ref().map(|n| n.to_string()).unwrap_or_default();
                write!(f, "{l} {} {r}", token.text())
            }
            Node::Numeric { text, .. } => f.write_str(text),
            other => f.write_str(other.token().text()),
        }
    }
}

/// Creates a unary node.
pub fn unary_node(token: SimpleToken) -> Node {
    let operator = UnaryOperator::of(token.text()).unwrap_or(UnaryOperator::Inc);
    Node::Unary {
        token,
        operator,
        operand: None,
    }
}

fn create_unary(
    operator: UnaryOperator,
    exp: SharedExpression,
    operand_text: String,
) -> SharedExpression {
    match operator {
        UnaryOperator::Not => named_expression(format!("!{operand_text}"), move |e| {
            let v = exp.evaluate(e)?;
            Ok(Value::Bool(
                !camel_util::object_helper::evaluate_value_predicate(&v),
            ))
        }),
        UnaryOperator::Inc | UnaryOperator::Dec => {
            let delta: i64 = if operator == UnaryOperator::Inc {
                1
            } else {
                -1
            };
            named_expression(format!("{operand_text}{operator}"), move |e| {
                let value = exp.evaluate(e)?;
                let result = match &value {
                    Value::Int(i) => Some(Value::Int(i + delta)),
                    Value::Float(f) => Some(Value::Float(f + delta as f64)),
                    other => match <i64 as camel_api::convert::FromValue>::from_value(other)
                        .ok()
                        .flatten()
                    {
                        Some(i) => Some(match other {
                            // keep the type of the value, as Java converts back to it
                            Value::Text(_) => Value::from((i + delta).to_string()),
                            _ => Value::Int(i + delta),
                        }),
                        None => <f64 as camel_api::convert::FromValue>::from_value(other)
                            .ok()
                            .flatten()
                            .map(|f| match other {
                                Value::Text(_) => Value::from(
                                    camel_util::value::java_double_to_string(f + delta as f64),
                                ),
                                _ => Value::Float(f + delta as f64),
                            }),
                    },
                };
                result.ok_or_else(|| {
                    CamelError::wrap_runtime(CamelError::camel_exchange(
                        &format!("Cannot evaluate {} as a number", exp.describe()),
                        e,
                    ))
                })
            })
        }
    }
}

fn create_quoted(children: &[Node], expression: &str) -> ParseResult<SharedExpression> {
    let mut parts = Vec::new();
    for c in children {
        parts.push(c.create_expression(expression)?);
    }
    Ok(concat(
        parts,
        children.iter().map(|c| c.to_string()).collect::<String>(),
    ))
}

/// Concatenates the values of the expressions as text, skipping `Null` (Java
/// `ExpressionBuilder.concatExpression`); one expression is returned as is.
pub fn concat(mut parts: Vec<SharedExpression>, description: String) -> SharedExpression {
    match parts.len() {
        0 => constant_expression(""),
        1 => parts.pop().unwrap(),
        _ => Arc::new(Concat { parts, description }),
    }
}

/// The concatenation of the parts of a template; [`Expression::evaluate_mut`] evaluates the
/// parts in order on the exchange they may change, so a part sees the changes of the parts
/// before it (such as `${setVariable(..)}` followed by `${variable.x}`).
struct Concat {
    parts: Vec<SharedExpression>,
    description: String,
}

impl Concat {
    fn join(
        &self,
        mut eval: impl FnMut(&SharedExpression) -> CamelResult<Value>,
    ) -> CamelResult<Value> {
        let mut sb = String::new();
        for p in &self.parts {
            let v = eval(p)?;
            if !v.is_null() {
                sb.push_str(&v.to_string());
            }
        }
        Ok(Value::from(sb))
    }
}

impl Expression for Concat {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.join(|p| p.evaluate(exchange))
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.join(|p| p.evaluate_mut(exchange))
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

fn create_function_block(
    token: &SimpleToken,
    children: &[Node],
    expression: &str,
) -> ParseResult<SharedExpression> {
    if children.len() == 1 {
        if let Node::Literal { text, .. } = &children[0] {
            if let Some(t) = try_parse_ternary(text, token.index)? {
                return Ok(t);
            }
            return create_function(text, token.index);
        }
    }
    if children.is_empty() {
        return create_function("", token.index);
    }
    // a composite function such as ${header.${header.name}}: the nested functions are kept as
    // text (Java lazy evaluation) or evaluated (quotes, ${type:..}), then the function is
    // created at runtime from the resulting text
    Ok(Arc::new(CompositeFunction {
        children: children.to_vec(),
        index: token.index,
        expression: expression.to_string(),
    }))
}

/// A function containing nested functions, created when evaluated from the text of its
/// parts (see `create_function_block`).
struct CompositeFunction {
    children: Vec<Node>,
    index: usize,
    expression: String,
}

impl CompositeFunction {
    /// The function of the text with the nested parts evaluated (read only, as Java).
    fn function(&self, e: &Exchange) -> CamelResult<SharedExpression> {
        let mut sb = String::new();
        for child in &self.children {
            match child {
                Node::Literal { text, .. } => sb.push_str(text),
                Node::FunctionStart { .. } if !child.to_string().starts_with("${type:") => {
                    sb.push_str(&child.to_string())
                }
                _ => {
                    let nested = child
                        .create_expression(&self.expression)
                        .map_err(|err| super::illegal_syntax(&self.expression, &err))?;
                    let v = nested.evaluate(e)?;
                    if !v.is_null() {
                        sb.push_str(&v.to_string());
                    }
                }
            }
        }
        let syntax = |err: ParserError| super::illegal_syntax(&self.expression, &err);
        if let Some(t) = try_parse_ternary(&sb, self.index).map_err(syntax)? {
            return Ok(t);
        }
        create_function(&sb, self.index).map_err(syntax)
    }
}

/// The error of a nested parser (the condition or a value of a ternary, a predicate in
/// braces): its full message, without a position in the outer expression (Java rethrows the
/// nested `SimpleIllegalSyntaxException`).
pub(crate) fn nested_error(text: &str, err: &ParserError) -> ParserError {
    ParserError {
        message: super::illegal_syntax(text, err)
            .message()
            .unwrap_or_default()
            .to_string(),
        index: None,
    }
}

/// Whether the character at the byte index has whitespace on both sides.
fn surrounded_by_whitespace(text: &str, index: usize) -> bool {
    index > 0
        && text[..index]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
        && text[index + 1..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
}

/// The byte index of the ternary operator character, skipping nested `${}` and quotes; the
/// operator must have whitespace around it, so `${bean:svc?method=at(10:30)}` is not a
/// ternary.
fn find_ternary_operator(text: &str, operator: char) -> Option<usize> {
    let mut depth = 0;
    let (mut single, mut double) = (false, false);
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !single && !double {
            if c == '$' && chars.peek().is_some_and(|&(_, n)| n == '{') {
                depth += 1;
                chars.next();
                continue;
            }
            if c == '}' && depth > 0 {
                depth -= 1;
                continue;
            }
            if c == '\'' && depth == 0 {
                single = true;
                continue;
            }
            if c == '"' && depth == 0 {
                double = true;
                continue;
            }
            if c == operator && depth == 0 && surrounded_by_whitespace(text, i) {
                return Some(i);
            }
        } else if single && c == '\'' {
            single = false;
        } else if double && c == '"' {
            double = false;
        }
    }
    None
}

/// The ternary operator in a function, `${header.foo > 0 ? 'positive' : 'negative'}` (Java
/// `SimpleFunctionStart.tryParseTernaryExpression`), or `None` when the text is not one.
///
/// The condition is a predicate (its function references are wrapped with `${ }` as for a
/// predicate in braces), the values are quoted literals, numbers, booleans, `null`, nested
/// ternaries or functions.
///
/// # Errors
///
/// A syntax error of the condition or of a value.
pub fn try_parse_ternary(text: &str, index: usize) -> ParseResult<Option<SharedExpression>> {
    let Some(question) = find_ternary_operator(text, '?') else {
        return Ok(None);
    };
    let Some(colon) = find_ternary_operator(&text[question + 1..], ':') else {
        return Ok(None);
    };
    let colon = question + 1 + colon;
    let condition = text[..question].trim();
    let true_text = text[question + 1..colon].trim();
    let false_text = text[colon + 1..].trim();
    if condition.is_empty() || true_text.is_empty() || false_text.is_empty() {
        return Ok(None);
    }
    let predicate_text = if condition.contains("${") {
        condition.to_string()
    } else {
        super::syntax_hints::wrap_functions(condition)
    };
    let predicate = super::predicate_parser::SimplePredicateParser::new(&predicate_text, true)
        .parse_predicate()
        .map_err(|e| nested_error(&predicate_text, &e))?;
    let when_true = parse_value_expression(true_text, index)?;
    let when_false = parse_value_expression(false_text, index)?;
    let description = format!("{condition} ? {true_text} : {false_text}");
    Ok(Some(Arc::new(Ternary {
        predicate,
        when_true,
        when_false,
        description,
    })))
}

/// A value of a ternary (Java `SimpleFunctionStart.parseValueExpression`).
fn parse_value_expression(text: &str, index: usize) -> ParseResult<SharedExpression> {
    let quoted = text.len() >= 2
        && ((text.starts_with('\'') && text.ends_with('\''))
            || (text.starts_with('"') && text.ends_with('"')));
    if quoted {
        return Ok(constant_expression(&text[1..text.len() - 1]));
    }
    if text == "null" || text == "${null}" {
        return Ok(constant_expression(Value::Null));
    }
    // a number or boolean on its own, not a function (CAMEL-24826)
    if super::predicate_parser::is_numeric_value(text) {
        return Ok(constant_expression(numeric_value(text)));
    }
    if text == "true" || text == "false" {
        return Ok(constant_expression(text == "true"));
    }
    if let Some(t) = try_parse_ternary(text, index)? {
        return Ok(t);
    }
    let exp = if text.starts_with("${") {
        text.to_string()
    } else {
        format!("${{{text}}}")
    };
    super::expression_parser::SimpleExpressionParser::new(&exp, true)
        .parse_expression()
        .map_err(|e| nested_error(&exp, &e))
}

/// `condition ? whenTrue : whenFalse`.
struct Ternary {
    predicate: camel_api::processor::SharedPredicate,
    when_true: SharedExpression,
    when_false: SharedExpression,
    description: String,
}

impl Expression for Ternary {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        if self.predicate.matches(exchange)? {
            self.when_true.evaluate(exchange)
        } else {
            self.when_false.evaluate(exchange)
        }
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        if self.predicate.matches(exchange)? {
            self.when_true.evaluate_mut(exchange)
        } else {
            self.when_false.evaluate_mut(exchange)
        }
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

impl Expression for CompositeFunction {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.function(exchange)?.evaluate(exchange)
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.function(exchange)?.evaluate_mut(exchange)
    }

    fn describe(&self) -> String {
        self.expression.clone()
    }
}

/// Whether the token type ends a block of the node (for `prepare_blocks`).
pub fn token_type(node: &Node) -> TokenType {
    node.token().ty.ty
}
