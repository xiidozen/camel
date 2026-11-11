//! The chain (`~>`, `?~>`), other (`?:`, elvis) and ternary (`? :`) operators of the simple
//! language (Java `ChainExpression`, `OtherExpression`, `TernaryExpression` and the
//! `prepareChainExpression`, `prepareOtherExpressions` and `prepareTernaryExpressions` steps
//! of `BaseSimpleParser`).
//!
//! * `${body} ?: 'World'` is the value of the left hand side, unless it is `null`, `false`,
//!   empty or zero, then the value of the right hand side.
//! * `${substringAfter('Hello')} ~> ${trim()} ~> ${uppercase()}` evaluates each function with
//!   the value of the previous one as the message body; `?~>` stops at the first `null`. A
//!   function may refer to the value with `$param` (`${replace('a','b',$param)}`).
//! * `${header.foo} > 0 ? 'positive' : 'negative'` in a predicate.

use super::ast::Node;
use super::tokenizer::SimpleToken;
use super::{ParseResult, ParserError};
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, SharedExpression};
use camel_util::object_helper::{evaluate_value_predicate, is_empty};
use camel_util::string_helper::remove_leading_and_ending_quotes;
use camel_util::value::Value;
use std::sync::Arc;

/// The variable holding the value passed along a chain, which `$param` refers to.
pub const CHAIN_VARIABLE: &str = "CamelSimpleChainParam";

/// Links the chain operators to their operands (Java `prepareChainExpression`): the left hand
/// side, then the right hand sides of the following chain operators of the same chain.
///
/// # Errors
///
/// Returns an error for a chain operator without left or right hand side.
pub fn prepare_chain_expressions(nodes: Vec<Node>) -> ParseResult<Vec<Node>> {
    let mut stack: Vec<Node> = Vec::new();
    let mut iter = nodes.into_iter();
    // whether the top of the stack is a chain the next chain operator continues
    let mut chaining = false;
    while let Some(node) = iter.next() {
        let Node::Chain { token, .. } = &node else {
            chaining = false;
            stack.push(node);
            continue;
        };
        let operator = token.text().to_string();
        let index = token.index;
        if stack.is_empty() {
            return Err(ParserError::new(
                format!("Chain operator {operator} has no left hand side token"),
                index,
            ));
        }
        let Some(right) = iter.next() else {
            return Err(ParserError::new(
                format!("Chain operator {operator} has no right hand side token"),
                index,
            ));
        };
        if chaining {
            // append to the existing chain (with the operator of its first link, as Java)
            if let Some(Node::Chain { right: rights, .. }) = stack.last_mut() {
                rights.push(right);
            }
        } else {
            let left = stack.pop().map(Box::new);
            let mut node = node;
            if let Node::Chain {
                left: l, right: r, ..
            } = &mut node
            {
                *l = left;
                r.push(right);
            }
            stack.push(node);
            chaining = true;
        }
    }
    Ok(stack)
}

/// Links the other operators (`?:`) to their operands (Java `prepareOtherExpressions`);
/// `a ?: b ?: c` is `(a ?: b) ?: c`.
///
/// # Errors
///
/// Returns an error for an operator without left or right hand side.
pub fn prepare_other_expressions(nodes: Vec<Node>) -> ParseResult<Vec<Node>> {
    let mut stack: Vec<Node> = Vec::new();
    let mut iter = nodes.into_iter();
    while let Some(mut node) = iter.next() {
        let Node::Other { token, left, right } = &mut node else {
            stack.push(node);
            continue;
        };
        let Some(l) = stack.pop() else {
            return Err(ParserError::new(
                format!(
                    "Other operator {} has no left hand side token",
                    token.text()
                ),
                token.index,
            ));
        };
        let Some(r) = iter.next() else {
            return Err(ParserError::new(
                format!(
                    "Other operator {} has no right hand side token",
                    token.text()
                ),
                token.index,
            ));
        };
        *left = Some(Box::new(l));
        *right = Some(Box::new(r));
        stack.push(node);
    }
    Ok(stack)
}

/// Links the ternary operators of a predicate to the condition and the two values (Java
/// `prepareTernaryExpressions`): `condition ? whenTrue : whenFalse`.
///
/// # Errors
///
/// Returns the Java errors for a `?` without condition, values or `:`.
pub fn prepare_ternary_expressions(nodes: Vec<Node>) -> ParseResult<Vec<Node>> {
    let mut answer: Vec<Node> = Vec::new();
    let mut iter = nodes.into_iter();
    while let Some(node) = iter.next() {
        let is_question = matches!(&node, Node::Ternary { token, .. } if token.text() == "?");
        if !is_question {
            answer.push(node);
            continue;
        }
        let index = node.token().index;
        let err = |message: &str| ParserError::new(message, index);
        let condition = answer
            .pop()
            .ok_or_else(|| err("Ternary operator ? has no condition"))?;
        let true_value = iter
            .next()
            .ok_or_else(|| err("Ternary operator ? has no true value"))?;
        let colon = iter
            .next()
            .ok_or_else(|| err("Ternary operator ? has no : operator"))?;
        if !matches!(&colon, Node::Ternary { token, .. } if token.text() == ":") {
            return Err(err("Ternary operator ? must be followed by :"));
        }
        let false_value = iter
            .next()
            .ok_or_else(|| err("Ternary operator : has no false value"))?;
        let mut node = node;
        if let Node::Ternary {
            condition: c,
            when_true,
            when_false,
            ..
        } = &mut node
        {
            *c = Some(Box::new(condition));
            *when_true = Some(Box::new(true_value));
            *when_false = Some(Box::new(false_value));
        }
        answer.push(node);
    }
    Ok(answer)
}

/// The expression of a node, without the quotes of a literal (the expression parser keeps the
/// quotes of `'World'` in the literal text).
fn unquoted_expression(node: &Node, expression: &str) -> ParseResult<SharedExpression> {
    match node {
        Node::Literal { token, text } => {
            let changed = remove_leading_and_ending_quotes(text);
            Node::Literal {
                token: token.clone(),
                text: changed,
            }
            .create_expression(expression)
        }
        other => other.create_expression(expression),
    }
}

fn missing(token: &SimpleToken, what: &str) -> ParserError {
    ParserError::new(format!("{what} must be specified on: {token}"), token.index)
}

/// Creates the elvis expression of `left ?: right` (Java `OtherExpression`).
///
/// # Errors
///
/// Returns an error if an operand is missing or cannot be created.
pub fn create_other(
    token: &SimpleToken,
    left: Option<&Node>,
    right: Option<&Node>,
    expression: &str,
) -> ParseResult<SharedExpression> {
    let left = left.ok_or_else(|| missing(token, "left node"))?;
    let right = right.ok_or_else(|| missing(token, "right node"))?;
    if token.text() != "?:" {
        return Err(ParserError::new(
            format!("Unknown other operator {}", token.text()),
            token.index,
        ));
    }
    Ok(Arc::new(Elvis {
        left: left.create_expression(expression)?,
        right: unquoted_expression(right, expression)?,
        description: format!("{left} ?: {right}"),
    }))
}

/// Whether the value is a number equal to zero (`0`, `0.0`, `-0.0`; not `NaN`).
fn is_zero(value: &Value) -> bool {
    match value {
        Value::Int(i) => *i == 0,
        Value::Float(f) => *f == 0.0,
        _ => false,
    }
}

/// `left ?: right`.
struct Elvis {
    left: SharedExpression,
    right: SharedExpression,
    description: String,
}

impl Elvis {
    /// Whether the right hand side is used for the value of the left hand side.
    fn use_right(value: &Value) -> bool {
        matches!(value, Value::Bool(false)) || is_empty(value) || is_zero(value)
    }
}

impl Expression for Elvis {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        // evaluated only once, as the left hand side may have side effects
        let value = self.left.evaluate(exchange)?;
        if Self::use_right(&value) {
            self.right.evaluate(exchange)
        } else {
            Ok(value)
        }
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        let value = self.left.evaluate_mut(exchange)?;
        if Self::use_right(&value) {
            self.right.evaluate_mut(exchange)
        } else {
            Ok(value)
        }
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// Replaces `$param` in the literal text of the node by the chain variable (Java
/// `ChainExpression.prepareChainNode`), returning whether the node refers to it.
fn prepare_chain_node(node: &mut Node) -> bool {
    let replacement = format!("${{variable.{CHAIN_VARIABLE}}}");
    let mut param = false;
    let mut replace = |text: &mut String| {
        if text.contains("$param") {
            *text = text.replace("$param", &replacement);
            param = true;
        }
    };
    match node {
        Node::Literal { text, .. } => replace(text),
        Node::FunctionStart { children, .. } => {
            for child in children {
                if let Node::Literal { text, .. } = child {
                    replace(text);
                }
            }
        }
        _ => {}
    }
    param
}

/// Creates the chain expression of `left ~> right ~> ...` (Java `ChainExpression`).
///
/// # Errors
///
/// Returns an error if an operand is missing or cannot be created.
pub fn create_chain(
    token: &SimpleToken,
    left: Option<&Node>,
    right: &[Node],
    expression: &str,
) -> ParseResult<SharedExpression> {
    let left = left.ok_or_else(|| missing(token, "left node"))?;
    if right.is_empty() {
        return Err(missing(token, "right node"));
    }
    let null_safe = match token.text() {
        "~>" => false,
        "?~>" => true,
        other => {
            return Err(ParserError::new(
                format!("Unknown chain operator {other}"),
                token.index,
            ))
        }
    };
    let mut left = left.clone();
    prepare_chain_node(&mut left);
    let left_exp = left.create_expression(expression)?;
    let mut links = Vec::new();
    for r in right {
        let mut r = r.clone();
        let param = prepare_chain_node(&mut r);
        links.push((r.create_expression(expression)?, param));
    }
    let description = std::iter::once(left.to_string())
        .chain(right.iter().map(|r| r.to_string()))
        .collect::<Vec<_>>()
        .join(&format!(" {} ", token.text()));
    Ok(Arc::new(Chain {
        left: left_exp,
        links,
        null_safe,
        description,
    }))
}

/// `left ~> right ~> ...`: the value of each part is the message body of the next.
struct Chain {
    left: SharedExpression,
    /// The right hand sides, and whether they refer to `$param`.
    links: Vec<(SharedExpression, bool)>,
    null_safe: bool,
    description: String,
}

impl Chain {
    fn run(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        let value = self.left.evaluate_mut(exchange)?;
        if value.is_null() && self.null_safe {
            return Ok(Value::Null);
        }
        let original = exchange.message_mut().take_body();
        let result = self.links(exchange, value);
        // restore the original body to avoid side effects
        exchange.message_mut().set_body(original);
        result
    }

    fn links(&self, exchange: &mut Exchange, input: Value) -> CamelResult<Value> {
        exchange.message_mut().set_body(input);
        let mut value = Value::Null;
        for (exp, param) in &self.links {
            value = if *param {
                let body = exchange.message().body().clone();
                exchange.set_variable(CHAIN_VARIABLE, body);
                let v = exp.evaluate_mut(exchange);
                exchange.remove_variable(CHAIN_VARIABLE);
                v?
            } else {
                exp.evaluate_mut(exchange)?
            };
            if value.is_null() && self.null_safe {
                return Ok(Value::Null);
            }
            exchange.message_mut().set_body(value.clone());
        }
        Ok(value)
    }
}

impl Expression for Chain {
    /// Evaluates the chain on a copy of the exchange, as the chain sets the message body of
    /// the exchange while evaluating (see [`Expression::evaluate_mut`]).
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.run(&mut exchange.clone())
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.run(exchange)
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

/// Creates the expression of the ternary operator of a predicate (Java `TernaryExpression`).
///
/// # Errors
///
/// Returns the Java errors for a missing part, or the error of creating a part.
pub fn create_ternary(
    token: &SimpleToken,
    condition: Option<&Node>,
    when_true: Option<&Node>,
    when_false: Option<&Node>,
    expression: &str,
) -> ParseResult<SharedExpression> {
    let index = token.index;
    let err = |message: &str| ParserError::new(format!("{message} at index {index}"), index);
    let condition = condition.ok_or_else(|| err("Ternary operator ? has no condition"))?;
    let when_true = when_true.ok_or_else(|| err("Ternary operator ? has no true value"))?;
    let when_false = when_false.ok_or_else(|| err("Ternary operator : has no false value"))?;
    Ok(Arc::new(TernaryNode {
        condition: condition.create_expression(expression)?,
        when_true: unquoted_expression(when_true, expression)?,
        when_false: unquoted_expression(when_false, expression)?,
        description: format!("{condition} ? {when_true} : {when_false}"),
    }))
}

/// `condition ? whenTrue : whenFalse` of a predicate.
struct TernaryNode {
    condition: SharedExpression,
    when_true: SharedExpression,
    when_false: SharedExpression,
    description: String,
}

impl Expression for TernaryNode {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        if evaluate_value_predicate(&self.condition.evaluate(exchange)?) {
            self.when_true.evaluate(exchange)
        } else {
            self.when_false.evaluate(exchange)
        }
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        if evaluate_value_predicate(&self.condition.evaluate_mut(exchange)?) {
            self.when_true.evaluate_mut(exchange)
        } else {
            self.when_false.evaluate_mut(exchange)
        }
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}
