//! The parser of simple predicates (Java `SimplePredicateParser`, `BinaryExpression`,
//! `LogicalExpression` and the operator types): `${header.foo} == 'bar' && ${body} contains
//! 'World'`.

use super::ast::{numeric_value, unary_node, Node};
use super::expression_parser::BaseParser;
use super::tokenizer::{SimpleToken, SimpleTokenizer, TokenType};
use super::{ParseResult, ParserError};
use camel_api::exchange::Exchange;
use camel_api::processor::{ExpressionPredicate, SharedExpression, SharedPredicate};
use camel_support::builder::{and, named_expression, named_predicate, not, ValueBuilder};
use camel_util::value::Value;
use std::sync::Arc;

fn unexpected_token(expression: &[char], index: usize) -> String {
    super::syntax_hints::unexpected_token(&expression.iter().collect::<String>(), index)
}

fn unsupported_operand(kind: &str, operator: &str, expression: &[char], index: usize) -> String {
    super::syntax_hints::unsupported_operand(
        kind,
        operator,
        &expression.iter().collect::<String>(),
        index,
    )
}

/// The binary operators (Java `BinaryOperatorType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum BinaryOperator {
    Eq,
    EqIgnore,
    Gt,
    Gte,
    Lt,
    Lte,
    NotEq,
    NotEqIgnore,
    Contains,
    NotContains,
    ContainsIgnoreCase,
    NotContainsIgnoreCase,
    Regex,
    NotRegex,
    In,
    NotIn,
    Is,
    NotIs,
    Range,
    NotRange,
    StartsWith,
    NotStartsWith,
    EndsWith,
    NotEndsWith,
    Equals,
    NotEquals,
}

impl BinaryOperator {
    /// The operator of the text.
    pub fn of(text: &str) -> Option<Self> {
        use BinaryOperator::*;
        Some(match text {
            "==" => Eq,
            "=~" => EqIgnore,
            ">" => Gt,
            ">=" => Gte,
            "<" => Lt,
            "<=" => Lte,
            "!=" => NotEq,
            "!=~" => NotEqIgnore,
            "contains" => Contains,
            "!contains" | "not contains" => NotContains,
            "~~" => ContainsIgnoreCase,
            "!~~" => NotContainsIgnoreCase,
            "regex" => Regex,
            "!regex" | "not regex" => NotRegex,
            "in" => In,
            "!in" | "not in" => NotIn,
            "is" => Is,
            "!is" | "not is" => NotIs,
            "range" => Range,
            "!range" | "not range" => NotRange,
            "startsWith" | "starts with" => StartsWith,
            "endsWith" | "ends with" => EndsWith,
            "!startsWith" => NotStartsWith,
            "!endsWith" => NotEndsWith,
            "equals" => Equals,
            "!equals" => NotEquals,
            _ => return None,
        })
    }

    /// The text of the operator (Java `getOperatorText`).
    pub fn text(self) -> &'static str {
        use BinaryOperator::*;
        match self {
            Eq => "==",
            EqIgnore => "=~",
            Gt => ">",
            Gte => ">=",
            Lt => "<",
            Lte => "<=",
            NotEq => "!=",
            NotEqIgnore => "!=~",
            Contains => "contains",
            NotContains => "!contains",
            ContainsIgnoreCase => "~~",
            NotContainsIgnoreCase => "!~~",
            Regex => "regex",
            NotRegex => "!regex",
            In => "in",
            NotIn => "!in",
            Is => "is",
            NotIs => "!is",
            Range => "range",
            NotRange => "!range",
            StartsWith => "startsWith",
            NotStartsWith => "!startsWith",
            EndsWith => "endsWith",
            NotEndsWith => "!endsWith",
            Equals => "equals",
            NotEquals => "!equals",
        }
    }

    /// Whether the right operand may only be a literal or a function (Java
    /// `supportedParameterTypes`): `(literal, literal with functions)` support.
    fn parameter_types(self) -> Option<(bool, bool)> {
        use BinaryOperator::*;
        match self {
            Regex | NotRegex => Some((true, false)),
            Is | NotIs | Range | NotRange => Some((false, true)),
            _ => None,
        }
    }
}

fn bool_expression(description: String, predicate: SharedPredicate) -> SharedExpression {
    named_expression(description, move |e| Ok(Value::Bool(predicate.matches(e)?)))
}

/// Creates the expression of a binary operator (Java `BinaryExpression.createExpression`).
pub fn create_binary(
    token: &SimpleToken,
    left: &Node,
    right: &Node,
    expression: &str,
) -> ParseResult<SharedExpression> {
    use BinaryOperator::*;
    let operator = BinaryOperator::of(token.text()).ok_or_else(|| {
        ParserError::new(
            format!("Operator not supported: {}", token.text()),
            token.index,
        )
    })?;
    let l = left.create_expression(expression)?;
    let r = right.create_expression(expression)?;
    let description = format!("{left} {} {right}", token.text());
    let lv = ValueBuilder::new(l.clone());
    let rv = ValueBuilder::new(r.clone());
    let p: SharedPredicate = match operator {
        Eq => lv.is_equal_to(rv),
        EqIgnore => lv.is_equal_to_ignore_case(rv),
        Gt => lv.is_greater_than(rv),
        Gte => lv.is_greater_than_or_equal_to(rv),
        Lt => lv.is_less_than(rv),
        Lte => lv.is_less_than_or_equal_to(rv),
        NotEq => lv.is_not_equal_to(rv),
        NotEqIgnore => not(lv.is_equal_to_ignore_case(rv)),
        Contains => lv.contains(rv),
        NotContains => not(lv.contains(rv)),
        ContainsIgnoreCase => lv.contains_ignore_case(rv),
        NotContainsIgnoreCase => not(lv.contains_ignore_case(rv)),
        StartsWith => lv.starts_with(rv),
        NotStartsWith => not(lv.starts_with(rv)),
        EndsWith => lv.ends_with(rv),
        NotEndsWith => not(lv.ends_with(rv)),
        Equals => equals_string(l, r),
        NotEquals => not(equals_string(l, r)),
        Regex | NotRegex => {
            let negate = operator == NotRegex;
            named_predicate(description.clone(), move |e| {
                let pattern = r.evaluate(e)?;
                let m = if pattern.is_null() {
                    false
                } else {
                    lv.regex(&pattern.to_string()).matches(e)?
                };
                Ok(m != negate)
            })
        }
        In | NotIn => {
            let negate = operator == NotIn;
            named_predicate(description.clone(), move |e| {
                let values = match r.evaluate(e)? {
                    Value::List(l) => l,
                    Value::Null => vec![],
                    Value::Text(t) => t.split(',').map(|s| Value::from(s.trim())).collect(),
                    other => vec![other],
                };
                Ok(lv.is_in(values).matches(e)? != negate)
            })
        }
        Is | NotIs => {
            let negate = operator == NotIs;
            let expr = expression.to_string();
            let index = right.token().index;
            named_predicate(description.clone(), move |e| {
                let name = r.evaluate(e)?;
                if name.is_null() || name.as_str() == Some("null") {
                    return Err(super::illegal_syntax(
                        &expr,
                        &ParserError::new(
                            format!(
                                "{} operator cannot accept null. A class type must be provided.",
                                operator.text()
                            ),
                            index,
                        ),
                    ));
                }
                let name = name.to_string();
                let simple = name
                    .trim_start_matches("java.lang.")
                    .trim_start_matches("java.util.");
                if !matches!(
                    simple,
                    "String"
                        | "Integer"
                        | "Long"
                        | "Double"
                        | "Float"
                        | "Boolean"
                        | "Short"
                        | "Byte"
                        | "Number"
                        | "Object"
                        | "byte[]"
                        | "List"
                        | "Map"
                        | "Collection"
                ) {
                    return Err(super::illegal_syntax(
                        &expr,
                        &ParserError::new(
                            format!(
                                "{} operator cannot find class with name: {name}",
                                operator.text()
                            ),
                            index,
                        ),
                    ));
                }
                let v = lv.evaluate_value(e)?;
                let m = match simple {
                    "Object" => !v.is_null(),
                    "Number" | "Integer" | "Long" | "Short" | "Byte" => {
                        matches!(v, Value::Int(_))
                            || (simple == "Number" && matches!(v, Value::Float(_)))
                    }
                    "Double" | "Float" => matches!(v, Value::Float(_)),
                    "Collection" => matches!(v, Value::List(_)),
                    other => !v.is_null() && v.type_name() == other,
                };
                Ok(m != negate)
            })
        }
        Range | NotRange => {
            let negate = operator == NotRange;
            let expr = expression.to_string();
            let index = right.token().index;
            named_predicate(description.clone(), move |e| {
                let range = r.evaluate(e)?;
                if range.is_null() {
                    return Ok(negate);
                }
                let text = range.to_string();
                let re = regex::Regex::new(r"^(\d+)(\.\.)(\d+)$").expect("valid regex");
                let Some(c) = re.captures(&text) else {
                    return Err(super::illegal_syntax(&expr, &ParserError::new(format!("{} operator is not valid. Valid syntax:'from..to' (where from and to are numbers).", operator.text()), index)));
                };
                let p = and(
                    lv.is_greater_than_or_equal_to(c[1].to_string()),
                    lv.is_less_than_or_equal_to(c[3].to_string()),
                );
                Ok(p.matches(e)? != negate)
            })
        }
    };
    Ok(bool_expression(description, p))
}

trait EvaluateValue {
    fn evaluate_value(&self, e: &Exchange) -> camel_api::error::CamelResult<Value>;
}

impl EvaluateValue for ValueBuilder {
    fn evaluate_value(&self, e: &Exchange) -> camel_api::error::CamelResult<Value> {
        self.expression().evaluate(e)
    }
}

/// Java `PredicateBuilder.equalsString`: both values as text are equal.
fn equals_string(l: SharedExpression, r: SharedExpression) -> SharedPredicate {
    let name = format!("{} equals {}", l.describe(), r.describe());
    named_predicate(name, move |e| {
        let (a, b) = (l.evaluate(e)?, r.evaluate(e)?);
        Ok(match (a.is_null(), b.is_null()) {
            (true, true) => true,
            (false, false) => a.to_string() == b.to_string(),
            _ => false,
        })
    })
}

/// Creates the expression of a logical operator (Java `LogicalExpression`).
pub fn create_logical(
    token: &SimpleToken,
    left: &Node,
    right: &Node,
    expression: &str,
) -> ParseResult<SharedExpression> {
    let l: SharedPredicate = Arc::new(ExpressionPredicate(left.create_expression(expression)?));
    let r: SharedPredicate = Arc::new(ExpressionPredicate(right.create_expression(expression)?));
    let description = format!("{left} {} {right}", token.text());
    let p = match token.text() {
        "&&" => camel_support::builder::and(l, r),
        "||" => camel_support::builder::or(l, r),
        other => {
            return Err(ParserError::new(
                format!("Unknown logical operator {other}"),
                token.index,
            ))
        }
    };
    Ok(bool_expression(description, p))
}

/// The parser of simple predicates (Java `SimplePredicateParser`).
pub struct SimplePredicateParser {
    b: BaseParser,
}

impl SimplePredicateParser {
    /// A parser of the predicate.
    pub fn new(expression: &str, allow_escape: bool) -> Self {
        SimplePredicateParser {
            b: BaseParser::new(expression, allow_escape, SimpleTokenizer::new(true)),
        }
    }

    /// Parses the predicate.
    ///
    /// # Errors
    ///
    /// Returns a syntax error with its position.
    pub fn parse_predicate(mut self) -> ParseResult<SharedPredicate> {
        self.parse_tokens()?;
        let text = self.b.text.clone();
        let mut predicates: Vec<SharedPredicate> = Vec::new();
        for node in &self.b.nodes {
            let exp = node.create_expression(&text)?;
            predicates.push(Arc::new(DescribedPredicate {
                exp,
                text: text.clone(),
            }));
        }
        Ok(match predicates.len() {
            0 => named_predicate("false", |_| Ok(false)),
            1 => predicates.pop().unwrap(),
            _ => predicates.into_iter().reduce(and).unwrap(),
        })
    }

    fn parse_tokens(&mut self) -> ParseResult<()> {
        self.b.next_token();
        while !self.b.ty().is_eol() {
            let matched = self.quoted_literal_with_functions(TokenType::SingleQuote)?
                || self.quoted_literal_with_functions(TokenType::DoubleQuote)?
                || self.function_text()?
                || self.unary_operator()?
                || self.binary_operator()?
                || self.other_like_operator(TokenType::TernaryOperator, "Ternary")?
                || self.other_like_operator(TokenType::ChainOperator, "Chain")?
                || self.other_like_operator(TokenType::OtherOperator, "Other")?
                || self.logical_operator()?
                || self.b.accept(TokenType::BooleanValue);
            if !matched && !self.b.ty().is_whitespace() && !self.b.ty().is_eol() {
                let i = self.b.previous_index;
                return Err(ParserError::new(unexpected_token(&self.b.expression, i), i));
            }
            self.b.next_token();
        }
        self.remove_ignorable_white_space_tokens();
        self.parse_tokens_and_create_nodes()?;
        self.b.prepare_blocks()?;
        self.b.prepare_unary_expressions()?;
        self.prepare_binary_expressions(false)?;
        self.prepare_binary_expressions(true)?;
        Ok(())
    }

    fn remove_ignorable_white_space_tokens(&mut self) {
        let (mut single, mut double, mut functions) = (false, false, 0);
        self.b.tokens.retain(|t| {
            if t.ty.is_ignore() {
                return false;
            }
            if t.ty.is_single_quote() && !double {
                single = !single;
            } else if t.ty.is_double_quote() && !single {
                double = !double;
            } else if !single && !double {
                if t.ty.is_function_start() {
                    functions += 1;
                } else if t.ty.is_function_end() {
                    if functions > 0 {
                        functions -= 1;
                    }
                } else if t.ty.is_whitespace() && functions == 0 {
                    return false;
                }
            }
            true
        });
    }

    fn parse_tokens_and_create_nodes(&mut self) -> ParseResult<()> {
        let (mut start_single, mut start_double, mut start_function) = (false, false, 0);
        let (mut last_single, mut last_double, mut last_function) = (0, 0, 0);
        let mut image: Option<Node> = None;
        let tokens = std::mem::take(&mut self.b.tokens);
        for token in tokens {
            if token.ty.is_eol() {
                break;
            }
            let ty = token.ty.ty;
            let node = if ty == TokenType::FunctionStart {
                start_function += 1;
                last_function = token.index;
                Some(Node::FunctionStart {
                    token: token.clone(),
                    children: vec![],
                })
            } else if start_function > 0 && ty == TokenType::FunctionEnd {
                start_function -= 1;
                Some(Node::FunctionEnd {
                    token: token.clone(),
                })
            } else if start_function > 0 {
                None
            } else if ty == TokenType::SingleQuote && !start_double {
                start_single = !start_single;
                last_single = token.index;
                Some(if start_single {
                    Node::SingleQuoteStart {
                        token: token.clone(),
                        children: vec![],
                    }
                } else {
                    Node::SingleQuoteEnd {
                        token: token.clone(),
                    }
                })
            } else if ty == TokenType::DoubleQuote && !start_single {
                start_double = !start_double;
                last_double = token.index;
                Some(if start_double {
                    Node::DoubleQuoteStart {
                        token: token.clone(),
                        children: vec![],
                    }
                } else {
                    Node::DoubleQuoteEnd {
                        token: token.clone(),
                    }
                })
            } else if start_single || start_double {
                None
            } else {
                match ty {
                    TokenType::UnaryOperator => Some(unary_node(token.clone())),
                    TokenType::BinaryOperator => Some(Node::Binary {
                        token: token.clone(),
                        left: None,
                        right: None,
                    }),
                    TokenType::LogicalOperator => Some(Node::Logical {
                        token: token.clone(),
                        left: None,
                        right: None,
                    }),
                    TokenType::TernaryOperator
                    | TokenType::OtherOperator
                    | TokenType::ChainOperator => Some(Node::Unsupported {
                        token: token.clone(),
                    }),
                    TokenType::NullValue => Some(Node::Null {
                        token: token.clone(),
                    }),
                    TokenType::BooleanValue => Some(Node::Boolean {
                        token: token.clone(),
                    }),
                    _ => None,
                }
            };
            if let Some(n) = node {
                if let Some(i) = image.take() {
                    self.add_image_token(i);
                }
                self.b.nodes.push(n);
                continue;
            }
            match &mut image {
                Some(Node::Literal { text, .. }) => text.push_str(token.text()),
                _ => {
                    image = Some(Node::Literal {
                        text: token.text().to_string(),
                        token,
                    })
                }
            }
        }
        if let Some(i) = image {
            self.add_image_token(i);
        }
        if start_single {
            return Err(ParserError::new(
                "single quote has no ending quote",
                last_single,
            ));
        }
        if start_double {
            return Err(ParserError::new(
                "double quote has no ending quote",
                last_double,
            ));
        }
        if start_function > 0 {
            return Err(ParserError::new(
                "function has no ending token: missing } to close ${...}",
                last_function,
            ));
        }
        Ok(())
    }

    fn add_image_token(&mut self, image: Node) {
        let quoted = matches!(
            self.b.nodes.last(),
            Some(Node::SingleQuoteStart { .. } | Node::DoubleQuoteStart { .. })
        );
        if let Node::Literal { token, text } = &image {
            if !quoted && is_numeric_value(text) {
                self.b.nodes.push(Node::Numeric {
                    token: token.clone(),
                    text: text.clone(),
                });
                return;
            }
        }
        self.b.nodes.push(image);
    }

    /// Binds binary operators (or logical operators when `logical`) to their operands (Java
    /// `prepareBinaryExpressions` / `prepareLogicalExpressions`).
    fn prepare_binary_expressions(&mut self, logical: bool) -> ParseResult<()> {
        let nodes = std::mem::take(&mut self.b.nodes);
        let mut stack: Vec<Node> = Vec::new();
        let mut iter = nodes.into_iter().peekable();
        let kind = if logical { "Logical" } else { "Binary" };
        while let Some(node) = iter.next() {
            let is_op = if logical {
                matches!(node, Node::Logical { .. })
            } else {
                matches!(node, Node::Binary { .. })
            };
            if !is_op {
                stack.push(node);
                continue;
            }
            let token = node.token().clone();
            let Some(left) = stack.pop() else {
                return Err(ParserError::new(
                    format!(
                        "{kind} operator {} has no left hand side token",
                        token.text()
                    ),
                    token.index,
                ));
            };
            if !logical {
                if let Node::Unary {
                    operator: super::ast::UnaryOperator::Not,
                    ..
                } = &left
                {
                    let negated = negated_operator(token.text());
                    let hint = match negated {
                        Some(n) => format!(": write ${{...}} {n} value"),
                        None => ": compare the other way round".to_string(),
                    };
                    return Err(ParserError::new(
                        format!(
                            "! cannot be compared: it negates a function, not a comparison{hint}"
                        ),
                        token.index,
                    ));
                }
            }
            let Some(right) = iter.next() else {
                return Err(ParserError::new(
                    format!(
                        "{kind} operator {} has no right hand side token",
                        token.text()
                    ),
                    token.index,
                ));
            };
            stack.push(if logical {
                Node::Logical {
                    token,
                    left: Some(Box::new(left)),
                    right: Some(Box::new(right)),
                }
            } else {
                Node::Binary {
                    token,
                    left: Some(Box::new(left)),
                    right: Some(Box::new(right)),
                }
            });
        }
        self.b.nodes = stack;
        Ok(())
    }

    fn quoted_literal_with_functions(&mut self, quote: TokenType) -> ParseResult<bool> {
        self.quoted(quote, true)
    }

    fn quoted(&mut self, quote: TokenType, with_functions: bool) -> ParseResult<bool> {
        if !self.b.accept(quote) {
            return Ok(false);
        }
        let filter: &[TokenType] = if with_functions {
            &[
                quote,
                TokenType::Eol,
                TokenType::FunctionStart,
                TokenType::FunctionEnd,
            ]
        } else {
            &[quote, TokenType::Eol]
        };
        let filter = filter.to_vec();
        self.b.next_token_filtered(&filter);
        while self.b.ty().ty != quote && !self.b.ty().is_eol() {
            self.b.next_token_filtered(&filter);
        }
        self.b.expect(quote)?;
        Ok(true)
    }

    fn function_text(&mut self) -> ParseResult<bool> {
        if self.b.accept(TokenType::FunctionStart) {
            self.b.next_token();
            while !self.b.ty().is_function_end() && !self.b.ty().is_eol() {
                if self.b.ty().is_function_start() {
                    self.function_text()?;
                }
                self.b.next_token();
            }
            if !self.b.ty().is_function_start() {
                self.b.expect(TokenType::FunctionEnd)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn unary_operator(&mut self) -> ParseResult<bool> {
        if self.b.accept(TokenType::UnaryOperator) {
            if self.b.ty().text() == "!" {
                return Ok(true);
            }
            self.b.next_token();
            self.b.expect(TokenType::WhiteSpace)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn operand(
        &mut self,
        literal: bool,
        literal_with_functions: bool,
        any: bool,
    ) -> ParseResult<bool> {
        Ok(
            (literal_with_functions && self.quoted(TokenType::SingleQuote, true)?)
                || (literal_with_functions && self.quoted(TokenType::DoubleQuote, true)?)
                || (literal && self.quoted(TokenType::SingleQuote, false)?)
                || (literal && self.quoted(TokenType::DoubleQuote, false)?)
                || self.function_text()?
                || (any && self.b.accept(TokenType::NumericValue))
                || (any && self.b.accept(TokenType::BooleanValue))
                || (any && self.b.accept(TokenType::NullValue))
                || (any && {
                    // lenient on purpose, as routes may compare with unquoted text such as
                    // ${header.version} == v2
                    self.b.next_token();
                    self.b.accept(TokenType::NumericValue)
                }),
        )
    }

    fn binary_operator(&mut self) -> ParseResult<bool> {
        if !self.b.accept(TokenType::BinaryOperator) {
            return Ok(false);
        }
        let op_text = self.b.ty().text().to_string();
        let operator = BinaryOperator::of(&op_text);
        self.b.next_token();
        self.b.expect_and_accept_more(TokenType::WhiteSpace)?;
        let (literal, with_functions, any) =
            match operator.and_then(BinaryOperator::parameter_types) {
                Some((l, lf)) => (l, lf, false),
                None => (false, true, true),
            };
        if self.operand(literal, with_functions, any)? {
            self.b.next_token();
            if !self.b.ty().is_eol() {
                self.b.expect(TokenType::WhiteSpace)?;
            }
        } else {
            let index = self.b.token.as_ref().map(|t| t.index).unwrap_or(0);
            let shown = operator.map(|o| o.text().to_string()).unwrap_or(op_text);
            return Err(ParserError::new(
                unsupported_operand("Binary", &shown, &self.b.expression, index),
                index,
            ));
        }
        Ok(true)
    }

    fn other_like_operator(&mut self, ty: TokenType, kind: &str) -> ParseResult<bool> {
        if !self.b.accept(ty) {
            return Ok(false);
        }
        let op_text = self.b.ty().text().to_string();
        self.b.next_token();
        self.b.expect_and_accept_more(TokenType::WhiteSpace)?;
        if self.operand(false, true, true)? {
            self.b.next_token();
            if !self.b.ty().is_eol() {
                self.b.expect(TokenType::WhiteSpace)?;
            }
        } else {
            let index = self.b.token.as_ref().map(|t| t.index).unwrap_or(0);
            return Err(ParserError::new(
                unsupported_operand(kind, &op_text, &self.b.expression, index),
                index,
            ));
        }
        Ok(true)
    }

    fn logical_operator(&mut self) -> ParseResult<bool> {
        if !self.b.accept(TokenType::LogicalOperator) {
            return Ok(false);
        }
        let op_text = self.b.ty().text().to_string();
        self.b.next_token();
        self.b.expect_and_accept_more(TokenType::WhiteSpace)?;
        // a negated function: && !${...}
        if self.b.accept(TokenType::UnaryOperator) && self.b.ty().text() == "!" {
            self.b.next_token();
        }
        if self.quoted(TokenType::SingleQuote, true)?
            || self.quoted(TokenType::DoubleQuote, true)?
            || self.function_text()?
            || self.b.accept(TokenType::NumericValue)
            || self.b.accept(TokenType::BooleanValue)
            || self.b.accept(TokenType::NullValue)
        {
            self.b.next_token();
            if !self.b.ty().is_eol() {
                self.b.expect(TokenType::WhiteSpace)?;
            }
        } else {
            let index = self.b.token.as_ref().map(|t| t.index).unwrap_or(0);
            return Err(ParserError::new(
                unsupported_operand("Logical", &op_text, &self.b.expression, index),
                index,
            ));
        }
        Ok(true)
    }
}

fn negated_operator(text: &str) -> Option<String> {
    const NEGATED: [&str; 11] = [
        "==",
        "=~",
        "~~",
        "contains",
        "endsWith",
        "equals",
        "in",
        "is",
        "range",
        "regex",
        "startsWith",
    ];
    if !NEGATED.contains(&text) {
        return None;
    }
    Some(if text == "==" {
        "!=".to_string()
    } else {
        format!("!{text}")
    })
}

/// Whether the text is a numeric value (Java `NumericExpression.isNumericValue`).
pub fn is_numeric_value(text: &str) -> bool {
    if text.contains('.') {
        camel_support::object_helper::is_floating_number(text)
    } else {
        camel_support::object_helper::is_number(text) && text.parse::<i64>().is_ok()
    }
}

/// A predicate of the value of a node expression (Java `ExpressionToPredicateAdapter`), with
/// the predicate text as its description.
struct DescribedPredicate {
    exp: SharedExpression,
    text: String,
}

impl camel_api::processor::Predicate for DescribedPredicate {
    fn matches(&self, exchange: &Exchange) -> camel_api::error::CamelResult<bool> {
        Ok(camel_util::object_helper::evaluate_value_predicate(
            &self.exp.evaluate(exchange)?,
        ))
    }

    fn describe(&self) -> String {
        self.text.clone()
    }
}

#[allow(dead_code)]
fn numeric(text: &str) -> Value {
    numeric_value(text)
}
