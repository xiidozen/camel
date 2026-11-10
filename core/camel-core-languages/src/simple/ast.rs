//! The nodes of a parsed simple expression (Java `org.apache.camel.language.simple.ast`), and
//! how they become expressions.

use super::functions::create_function;
use super::tokenizer::{SimpleToken, TokenType};
use super::{ParseResult, ParserError};
use camel_api::error::CamelError;
use camel_api::exchange::Exchange;
use camel_api::processor::SharedExpression;
use camel_support::builder::{constant_expression, named_expression};
use camel_util::value::Value;
use std::fmt;

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
        _ => named_expression(description, move |e| {
            let mut sb = String::new();
            for p in &parts {
                let v = p.evaluate(e)?;
                if !v.is_null() {
                    sb.push_str(&v.to_string());
                }
            }
            Ok(Value::from(sb))
        }),
    }
}

fn create_function_block(
    token: &SimpleToken,
    children: &[Node],
    expression: &str,
) -> ParseResult<SharedExpression> {
    if children.len() == 1 {
        if let Node::Literal { text, .. } = &children[0] {
            return create_function(text, token.index);
        }
    }
    if children.is_empty() {
        return create_function("", token.index);
    }
    // a composite function such as ${header.${header.name}}: the nested functions are kept as
    // text (Java lazy evaluation) or evaluated (quotes, ${type:..}), then the function is
    // created at runtime from the resulting text
    let children = children.to_vec();
    let index = token.index;
    let expression = expression.to_string();
    let description = expression.clone();
    Ok(named_expression(description, move |e: &Exchange| {
        let mut sb = String::new();
        for child in &children {
            match child {
                Node::Literal { text, .. } => sb.push_str(text),
                Node::FunctionStart { .. } if !child.to_string().starts_with("${type:") => {
                    sb.push_str(&child.to_string())
                }
                _ => {
                    let nested = child
                        .create_expression(&expression)
                        .map_err(|err| super::illegal_syntax(&expression, &err))?;
                    let v = nested.evaluate(e)?;
                    if !v.is_null() {
                        sb.push_str(&v.to_string());
                    }
                }
            }
        }
        let function =
            create_function(&sb, index).map_err(|err| super::illegal_syntax(&expression, &err))?;
        function.evaluate(e)
    }))
}

/// Whether the token type ends a block of the node (for `prepare_blocks`).
pub fn token_type(node: &Node) -> TokenType {
    node.token().ty.ty
}
