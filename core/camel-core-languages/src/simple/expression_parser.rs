//! The parser of simple expressions (Java `BaseSimpleParser` and `SimpleExpressionParser`):
//! tokenizes the template, builds the nodes (literals, functions, unary operators) and creates
//! the expression concatenating them.

use super::ast::{concat, unary_node, Node};
use super::other_operators::{prepare_chain_expressions, prepare_other_expressions};
use super::tokenizer::{SimpleToken, SimpleTokenType, SimpleTokenizer, TokenType};
use super::{ParseResult, ParserError};
use camel_api::processor::SharedExpression;
use camel_support::builder::constant_expression;

/// The state shared by the expression and predicate parsers (Java `BaseSimpleParser`).
pub struct BaseParser {
    /// The expression as characters.
    pub expression: Vec<char>,
    /// The expression text.
    pub text: String,
    /// The tokenizer.
    pub tokenizer: SimpleTokenizer,
    /// The tokens read so far.
    pub tokens: Vec<SimpleToken>,
    /// The nodes.
    pub nodes: Vec<Node>,
    /// The current token.
    pub token: Option<SimpleToken>,
    /// The position before the current token.
    pub previous_index: usize,
    /// The position after the current token.
    pub index: usize,
    /// Whether escapes (`\n`, `\t`, ...) are allowed.
    pub allow_escape: bool,
}

impl BaseParser {
    /// A parser of the expression.
    pub fn new(expression: &str, allow_escape: bool, tokenizer: SimpleTokenizer) -> Self {
        BaseParser {
            expression: expression.chars().collect(),
            text: expression.to_string(),
            tokenizer,
            tokens: Vec::new(),
            nodes: Vec::new(),
            token: None,
            previous_index: 0,
            index: 0,
            allow_escape,
        }
    }

    /// The current token type.
    pub fn ty(&self) -> &SimpleTokenType {
        &self.token.as_ref().expect("a current token").ty
    }

    /// Reads the next token (accepting only the filter types, all if empty).
    pub fn next_token_filtered(&mut self, filter: &[TokenType]) {
        if self.index < self.expression.len() {
            let next =
                self.tokenizer
                    .next_token(&self.expression, self.index, self.allow_escape, filter);
            self.tokens.push(next.clone());
            self.previous_index = self.index;
            self.index += next.length;
            self.token = Some(next);
        } else {
            self.token = Some(SimpleToken::new(
                SimpleTokenType::new(TokenType::Eol, None),
                self.index,
            ));
        }
    }

    /// Reads the next token.
    pub fn next_token(&mut self) {
        self.next_token_filtered(&[]);
    }

    /// Whether the current token is of the type.
    pub fn accept(&self, ty: TokenType) -> bool {
        self.token.as_ref().map_or(true, |t| t.ty.ty == ty)
    }

    /// Fails unless the current token is of the type.
    ///
    /// # Errors
    ///
    /// Returns the Java `expected symbol ... but was ...` errors.
    pub fn expect(&self, expect: TokenType) -> ParseResult<()> {
        match &self.token {
            Some(t) if t.ty.ty == expect => Ok(()),
            None => {
                if expect == TokenType::FunctionEnd {
                    Err(ParserError::new(
                        format!("expected symbol {expect} but reached eol: missing }} to close the function"),
                        self.previous_index,
                    ))
                } else {
                    Err(ParserError::new(
                        format!("expected symbol {expect} but reached eol"),
                        self.previous_index,
                    ))
                }
            }
            Some(t) => {
                let hint = if expect == TokenType::FunctionEnd && t.ty.is_eol() {
                    ": missing } to close the function"
                } else if expect == TokenType::WhiteSpace && t.ty.ty == TokenType::SingleQuote {
                    ": a literal that contains a single quote must be written with double quotes, e.g. \"it's\""
                } else if expect == TokenType::WhiteSpace {
                    ": operators and values must be separated by a space"
                } else {
                    ""
                };
                Err(ParserError::new(
                    format!("expected symbol {expect} but was {}{hint}", t.ty.ty),
                    self.previous_index,
                ))
            }
        }
    }

    /// Fails unless the current token is one of the types.
    ///
    /// # Errors
    ///
    /// Returns the Java `expected any symbol [...] but was ...` error.
    pub fn expect_any(&self, expect: &[TokenType]) -> ParseResult<()> {
        let list = expect
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        match &self.token {
            None => Err(ParserError::new(
                format!("expected any symbol [{list}] but reached eol"),
                self.previous_index,
            )),
            Some(t) if expect.contains(&t.ty.ty) => Ok(()),
            Some(t) => Err(ParserError::new(
                format!("expected any symbol [{list}] but was {}", t.ty.ty),
                self.previous_index,
            )),
        }
    }

    /// Expects the type, and skips more tokens of the type.
    ///
    /// # Errors
    ///
    /// Returns the error of [`expect`](Self::expect).
    pub fn expect_and_accept_more(&mut self, expect: TokenType) -> ParseResult<()> {
        self.expect(expect)?;
        while !self.ty().is_eol() && self.ty().ty == expect {
            self.next_token();
        }
        Ok(())
    }

    /// Removes the ignorable tokens, and the whitespace before and after the chain, other and
    /// init operators (Java `SimpleExpressionParser.removeIgnorableWhiteSpaceTokens`).
    pub fn remove_ignorable_white_space_tokens(&mut self) {
        self.tokens.retain(|t| !t.ty.is_ignore());
        let mut remove = vec![false; self.tokens.len()];
        for i in 1..self.tokens.len().saturating_sub(1) {
            let ty = &self.tokens[i].ty;
            if ty.is_other() || ty.is_chain() || ty.is_init() {
                if self.tokens[i - 1].ty.is_whitespace() {
                    remove[i - 1] = true;
                }
                if self.tokens[i + 1].ty.is_whitespace() {
                    remove[i + 1] = true;
                }
            }
        }
        let mut i = 0;
        self.tokens.retain(|_| {
            i += 1;
            !remove[i - 1]
        });
    }

    /// A quoted literal that may contain functions (Java
    /// `singleQuotedLiteralWithFunctionsText` / `doubleQuotedLiteralWithFunctionsText`).
    ///
    /// # Errors
    ///
    /// Returns an error when the closing quote is missing.
    pub fn quoted_with_functions(&mut self, quote: TokenType) -> ParseResult<bool> {
        if !self.accept(quote) {
            return Ok(false);
        }
        let filter = [
            quote,
            TokenType::Eol,
            TokenType::FunctionStart,
            TokenType::FunctionEnd,
        ];
        self.next_token_filtered(&filter);
        while self.ty().ty != quote && !self.ty().is_eol() {
            self.next_token_filtered(&filter);
        }
        self.expect(quote)?;
        Ok(true)
    }

    /// Nests the nodes between block start and end nodes into the blocks (Java
    /// `prepareBlocks`).
    ///
    /// # Errors
    ///
    /// Returns an error for an unbalanced or unacceptable block.
    pub fn prepare_blocks(&mut self) -> ParseResult<()> {
        let mut answer: Vec<Node> = Vec::new();
        let mut stack: Vec<Node> = Vec::new();
        for node in std::mem::take(&mut self.nodes) {
            if node.is_block_start() {
                stack.push(node);
            } else if node.is_block_end() {
                let Some(top) = stack.pop() else {
                    return Err(ParserError::new(
                        format!("{} has no matching start token", node.token().ty.ty),
                        node.token().index,
                    ));
                };
                accept_or_add(&mut answer, &mut stack, top)?;
            } else {
                accept_or_add(&mut answer, &mut stack, node)?;
            }
        }
        // unclosed blocks are kept as they are (Java leaves them on the stack)
        self.nodes = answer;
        Ok(())
    }

    /// Binds the unary operators to their operands (Java `prepareUnaryExpressions`).
    ///
    /// # Errors
    ///
    /// Returns an error for an operator without operand.
    pub fn prepare_unary_expressions(&mut self) -> ParseResult<()> {
        let mut stack: Vec<Node> = Vec::new();
        let mut pending = false;
        for node in std::mem::take(&mut self.nodes) {
            if pending {
                if let Some(Node::Unary { operand, .. }) = stack.last_mut() {
                    *operand = Some(Box::new(node));
                }
                pending = false;
                continue;
            }
            if let Node::Unary {
                operator, token, ..
            } = &node
            {
                if *operator == super::ast::UnaryOperator::Not {
                    pending = true;
                    stack.push(node);
                    continue;
                }
                let Some(previous) = stack.pop() else {
                    return Err(ParserError::new(
                        format!(
                            "Unary operator {} has no left hand side token",
                            token.text()
                        ),
                        token.index,
                    ));
                };
                let mut node = node;
                if let Node::Unary { operand, .. } = &mut node {
                    *operand = Some(Box::new(previous));
                }
                stack.push(node);
                continue;
            }
            stack.push(node);
        }
        if pending {
            let index = stack.last().map(|n| n.token().index).unwrap_or(0);
            return Err(ParserError::new(
                "Unary operator ! has no token to negate on its right hand side",
                index,
            ));
        }
        self.nodes = stack;
        Ok(())
    }
}

fn accept_or_add(answer: &mut Vec<Node>, stack: &mut [Node], node: Node) -> ParseResult<()> {
    match stack.last_mut() {
        Some(block) => {
            let block_type = block.token().ty.ty;
            block.accept_and_add(node).map_err(|n| {
                ParserError::new(
                    format!("{block_type} cannot accept {}", n.token().ty.ty),
                    n.token().index,
                )
            })
        }
        None => {
            answer.push(node);
            Ok(())
        }
    }
}

/// The parser of simple expressions (Java `SimpleExpressionParser`).
pub struct SimpleExpressionParser {
    base: BaseParser,
}

impl SimpleExpressionParser {
    /// A parser of the expression.
    pub fn new(expression: &str, allow_escape: bool) -> Self {
        SimpleExpressionParser {
            base: BaseParser::new(expression, allow_escape, SimpleTokenizer::new(false)),
        }
    }

    /// Parses the expression.
    ///
    /// # Errors
    ///
    /// Returns a syntax error with its position.
    pub fn parse_expression(mut self) -> ParseResult<SharedExpression> {
        self.parse_tokens()?;
        let text = self.base.text.clone();
        let mut expressions = Vec::new();
        for node in &self.base.nodes {
            expressions.push(node.create_expression(&text)?);
        }
        Ok(if expressions.is_empty() {
            constant_expression("")
        } else {
            concat(expressions, text)
        })
    }

    fn parse_tokens(&mut self) -> ParseResult<()> {
        let b = &mut self.base;
        b.next_token();
        while !b.ty().is_eol() {
            Self::template_text(b);
            Self::function_text(b)?;
            Self::unary_operator(b)?;
            Self::other_operator(b, TokenType::ChainOperator, "Chain")?;
            Self::other_operator(b, TokenType::OtherOperator, "Other")?;
            b.next_token();
        }
        b.remove_ignorable_white_space_tokens();
        self.parse_and_create_ast_model();
        self.base.prepare_blocks()?;
        self.base.prepare_unary_expressions()?;
        let nodes = std::mem::take(&mut self.base.nodes);
        let nodes = prepare_chain_expressions(nodes)?;
        self.base.nodes = prepare_other_expressions(nodes)?;
        Ok(())
    }

    /// The chain (`~>`, `?~>`) and other (`?:`) operators (Java `chainOperator` and
    /// `otherOperator`): whitespace, then a quoted literal, a function, a number, a boolean or
    /// `null`.
    fn other_operator(b: &mut BaseParser, ty: TokenType, kind: &str) -> ParseResult<bool> {
        if !b.accept(ty) {
            return Ok(false);
        }
        let operator = b.ty().text().to_string();
        b.next_token();
        b.expect_and_accept_more(TokenType::WhiteSpace)?;
        if b.quoted_with_functions(TokenType::SingleQuote)?
            || b.quoted_with_functions(TokenType::DoubleQuote)?
            || Self::function_text(b)?
            || b.accept(TokenType::NumericValue)
            || b.accept(TokenType::BooleanValue)
            || b.accept(TokenType::NullValue)
        {
            b.next_token();
            if !b.ty().is_eol() {
                if ty == TokenType::ChainOperator {
                    b.expect_and_accept_more(TokenType::WhiteSpace)?;
                } else {
                    b.expect(TokenType::WhiteSpace)?;
                }
            }
            return Ok(true);
        }
        let index = b.token.as_ref().map(|t| t.index).unwrap_or(0);
        Err(ParserError::new(
            super::syntax_hints::unsupported_operand(kind, &operator, &b.text, index),
            index,
        ))
    }

    fn parse_and_create_ast_model(&mut self) {
        let mut functions = 0;
        let mut image: Option<Node> = None;
        let tokens = std::mem::take(&mut self.base.tokens);
        for token in tokens {
            if token.ty.is_eol() {
                break;
            }
            let node = if token.ty.is_function_start() {
                functions += 1;
                Some(Node::FunctionStart {
                    token: token.clone(),
                    children: vec![],
                })
            } else if functions > 0 && token.ty.is_function_end() {
                functions -= 1;
                Some(Node::FunctionEnd {
                    token: token.clone(),
                })
            } else if token.ty.is_unary() {
                if matches!(self.base.nodes.last(), Some(Node::FunctionEnd { .. })) {
                    Some(unary_node(token.clone()))
                } else {
                    None
                }
            } else if token.ty.is_chain() {
                Some(Node::Chain {
                    token: token.clone(),
                    left: None,
                    right: vec![],
                })
            } else if token.ty.is_other() {
                Some(Node::Other {
                    token: token.clone(),
                    left: None,
                    right: None,
                })
            } else if token.ty.is_init() {
                Some(Node::Unsupported {
                    token: token.clone(),
                })
            } else {
                None
            };
            if let Some(n) = node {
                if let Some(i) = image.take() {
                    self.base.nodes.push(i);
                }
                self.base.nodes.push(n);
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
            self.base.nodes.push(i);
        }
    }

    fn template_text(b: &mut BaseParser) {
        while !b.ty().is_function_start()
            && !b.ty().is_function_end()
            && !b.ty().is_eol()
            && !b.ty().is_other()
            && !b.ty().is_chain()
        {
            b.next_token();
        }
    }

    fn function_text(b: &mut BaseParser) -> ParseResult<bool> {
        if b.accept(TokenType::FunctionStart) {
            b.next_token();
            while !b.ty().is_function_end() && !b.ty().is_eol() {
                if b.ty().is_function_start() {
                    Self::function_text(b)?;
                }
                b.next_token();
            }
            if !b.ty().is_function_start() {
                b.expect(TokenType::FunctionEnd)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn unary_operator(b: &mut BaseParser) -> ParseResult<bool> {
        if b.accept(TokenType::UnaryOperator) {
            b.next_token();
            if !b.ty().is_eol() {
                b.expect(TokenType::WhiteSpace)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
}
