//! The init block of the simple language (Java `SimpleInitBlockParser`): local variables
//! assigned at the top of an expression or predicate, then used as `$name`.
//!
//! ```text
//! $init{
//!   $sum := ${sum(${header.lines},100)};
//!   $sku := ${body contains 'Camel' ? '123' : '999'};
//! }init$
//! orderId=$sku,total=$sum
//! ```
//!
//! A value is a function (with operators and chains), a quoted text (which may hold
//! functions), a number or a boolean. The statements end with a new line or a `;`; `//`
//! starts a comment. The variables are set on the exchange (as exchange variables) in order
//! when the expression is evaluated, and `$name` in the rest of the expression is
//! `${variable.name}`.
//!
//! An init function is a chain of functions (`$clean ~:= ${trim()} ~> ${uppercase()}`)
//! called as `$clean()` (on the body), `${clean()}` or `${clean(value)}` (on the value), which
//! are `${function(clean)}` and `${function(clean,value)}`.

use super::expression_parser::SimpleExpressionParser;
use super::predicate_parser::SimplePredicateParser;
use super::{illegal_syntax, ParserError};
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, Predicate, SharedExpression, SharedPredicate};
use camel_util::value::Value;
use std::sync::Arc;

/// The start of an init block.
pub const INIT_START: &str = "$init{";
/// The end of an init block.
pub const INIT_END: &str = "}init$";

/// Whether the expression starts with an init block (Java `hasInitBlock`).
pub fn has_init_block(expression: &str) -> bool {
    expression.starts_with(INIT_START) && expression.contains(INIT_END)
}

/// The prefix of the exchange variables holding the init functions while the expression is
/// evaluated.
pub const FUNCTION_VARIABLE_PREFIX: &str = "CamelSimpleInitFunction.";

/// The value of a statement.
enum InitValue {
    /// An init function (`~:=`).
    Function(SharedExpression),
    /// A constant (a number or a boolean).
    Constant(Value),
    /// An expression (a function, or a quoted text with functions).
    Expression(SharedExpression),
}

/// A statement of the init block: `$name := value`.
struct Statement {
    name: String,
    value: InitValue,
}

/// Blanks the comments and ends the statements at `; ` (Java `normalize`), keeping the
/// indexes.
fn normalize(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let (mut single, mut double, mut depth) = (false, false, 0usize);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if c == '\r' && next == '\n' {
            chars[i] = '\n';
            chars[i + 1] = ' ';
        } else if c == '\'' && !double {
            single = !single;
        } else if c == '"' && !single {
            double = !double;
        } else if single || double {
        } else if c == '$' && next == '{' {
            depth += 1;
        } else if c == '}' && depth > 0 {
            depth -= 1;
        } else if depth == 0 && c == '/' && next == '/' {
            while i < chars.len() && chars[i] != '\n' {
                chars[i] = ' ';
                i += 1;
            }
            continue;
        } else if depth == 0 && c == ';' && (next == ' ' || next == '\t') {
            chars[i + 1] = '\n';
        }
        i += 1;
    }
    chars.into_iter().collect()
}

fn error(message: impl Into<String>, index: usize) -> ParserError {
    ParserError::new(message, index + INIT_START.len())
}

/// Parses the statements of the block text.
fn parse_statements(block: &str, allow_escape: bool) -> Result<Vec<Statement>, ParserError> {
    let text = normalize(block);
    let chars: Vec<char> = text.chars().collect();
    let mut statements = Vec::new();
    let mut i = 0;
    loop {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ';') {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] != '$' {
            return Err(error(
                format!(
                    "Unexpected token {} in the init block, expected a $variable",
                    chars[i]
                ),
                i,
            ));
        }
        let start = i + 1;
        i = start;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '-')
        {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        if name.is_empty() {
            return Err(error(
                "Expected a variable name after $ in the init block",
                start,
            ));
        }
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        let rest: String = chars[i..].iter().take(3).collect();
        let function = rest.starts_with("~:=");
        if function {
            i += 3;
        } else if rest.starts_with(":=") {
            i += 2;
        } else {
            return Err(error(
                format!("Expected := after ${name} in the init block"),
                i,
            ));
        }
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        // the value: until a new line or ; outside quotes and functions
        let value_start = i;
        let (mut single, mut double, mut depth) = (false, false, 0usize);
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied().unwrap_or('\0');
            if c == '\'' && !double {
                single = !single;
            } else if c == '"' && !single {
                double = !double;
            } else if !single && !double {
                if c == '$' && next == '{' {
                    depth += 1;
                } else if c == '}' && depth > 0 {
                    depth -= 1;
                } else if depth == 0 && (c == '\n' || c == ';') {
                    break;
                }
            }
            i += 1;
        }
        let value: String = chars[value_start..i].iter().collect();
        let value = value.trim();
        if value.is_empty() {
            return Err(error(
                format!("Expected a value for ${name} in the init block"),
                value_start,
            ));
        }
        // the init functions declared before are called as custom functions
        let functions: Vec<&str> = statements
            .iter()
            .filter(|s: &&Statement| matches!(s.value, InitValue::Function(_)))
            .map(|s| s.name.as_str())
            .collect();
        let value = rewrite_functions(value, &functions);
        let value = value.as_str();
        let parse = |text: &str| {
            SimpleExpressionParser::new(text, allow_escape)
                .parse_expression()
                .map_err(|e| error(e.message, value_start + e.index.unwrap_or(0)))
        };
        let quoted = value.len() >= 2
            && ((value.starts_with('\'') && value.ends_with('\''))
                || (value.starts_with('"') && value.ends_with('"')));
        let value = if function {
            if !value.starts_with("${") {
                return Err(error(
                    format!("The init function ${name} must be functions, was: {value}"),
                    value_start,
                ));
            }
            InitValue::Function(parse(value)?)
        } else if quoted {
            InitValue::Expression(parse(&value[1..value.len() - 1])?)
        } else if value == "true" || value == "false" {
            InitValue::Constant(Value::Bool(value == "true"))
        } else if let Ok(n) = value.parse::<i64>() {
            InitValue::Constant(Value::Int(n))
        } else if let Ok(n) = value.parse::<f64>() {
            InitValue::Constant(Value::Float(n))
        } else if value.starts_with("${") {
            InitValue::Expression(parse(value)?)
        } else {
            return Err(error(
                format!(
                    "The value of ${name} must be a function, a quoted text, a number or a boolean, was: {value}"
                ),
                value_start,
            ));
        };
        statements.push(Statement { name, value });
    }
    Ok(statements)
}

/// The rest of the expression after the block, with `$name` rewritten to
/// `${variable.name}` (Java `rewriteExpressionAfterInitBlock`).
fn rewrite(expression: &str, statements: &[Statement]) -> String {
    let after = &expression[expression.find(INIT_END).unwrap_or(0) + INIT_END.len()..];
    let mut answer = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after)
        .to_string();
    let mut names: Vec<&str> = statements
        .iter()
        .filter(|s| !matches!(s.value, InitValue::Function(_)))
        .map(|s| s.name.as_str())
        .collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    names.dedup();
    for name in names {
        answer = answer.replace(&format!("${name}"), &format!("${{variable.{name}}}"));
    }
    let functions: Vec<&str> = statements
        .iter()
        .filter(|s| matches!(s.value, InitValue::Function(_)))
        .map(|s| s.name.as_str())
        .collect();
    rewrite_functions(&answer, &functions)
}

/// Rewrites the calls of the init functions to `${function(name)}` and
/// `${function(name,arguments)}`.
fn rewrite_functions(text: &str, functions: &[&str]) -> String {
    let mut names = functions.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let mut answer = text.to_string();
    for name in names {
        answer = answer
            .replace(&format!("${name}()"), &format!("${{function({name})}}"))
            .replace(&format!("${{{name}()}}"), &format!("${{function({name})}}"))
            .replace(&format!("${{{name}("), &format!("${{function({name},"));
    }
    answer
}

/// Creates the `${function(name)}` or `${function(name,value)}` function: calls the init
/// function of the expression, else the custom function of the context
/// (`SimpleFunctionRegistry`), on the body or on the value.
///
/// # Errors
///
/// A missing name.
pub fn function_call(remainder: &str, index: usize) -> Result<SharedExpression, ParserError> {
    let inner = remainder
        .rfind(')')
        .map(|i| &remainder[..i])
        .ok_or_else(|| {
            ParserError::new(
                "Valid syntax: ${function(name)} or ${function(name,exp)}",
                index,
            )
        })?;
    let (name, argument) = match inner.split_once(',') {
        Some((n, a)) => (n.trim().to_string(), Some(a.trim().to_string())),
        None => (inner.trim().to_string(), None),
    };
    if name.is_empty() {
        return Err(ParserError::new(
            "Valid syntax: ${function(name)} or ${function(name,exp)}",
            index,
        ));
    }
    let argument = argument.filter(|a| !a.is_empty()).map(|a| {
        let quoted = a.len() >= 2
            && ((a.starts_with('\'') && a.ends_with('\''))
                || (a.starts_with('"') && a.ends_with('"')));
        let text = if quoted {
            a[1..a.len() - 1].to_string()
        } else {
            a
        };
        super::string_functions::Lazy::new(&text)
    });
    let description = format!("function({name})");
    Ok(camel_support::builder::named_expression(
        description,
        move |e| {
            let key = format!("{FUNCTION_VARIABLE_PREFIX}{name}");
            let input = match &argument {
                Some(a) => a.evaluate(e)?,
                None => e.message().body().clone(),
            };
            // an init function of the expression, else a custom function of the context
            if let Some(Value::Object(o)) = e.variable(&key) {
                if let Some(function) = o.downcast_ref::<SharedExpression>().cloned() {
                    let mut copy = e.clone();
                    copy.message_mut().set_body(input);
                    return function.evaluate_mut(&mut copy);
                }
            }
            let registry = camel_api::context::context_plugin::<
                camel_api::spi::simple_function::SimpleFunctionRegistry,
            >(e.context().as_ref());
            match registry.and_then(|r| r.function(&name)) {
                Some(f) => f.apply(e, input),
                None => Err(camel_api::error::CamelError::illegal_argument(format!(
                    "No custom simple function with name: {name}"
                ))),
            }
        },
    ))
}

/// The init block and the rest of the expression.
struct InitBlock {
    statements: Vec<Statement>,
}

impl InitBlock {
    fn parse(expression: &str, allow_escape: bool) -> CamelResult<(Self, String)> {
        let start = INIT_START.len();
        let end = expression.find(INIT_END).unwrap_or(expression.len());
        let statements = parse_statements(&expression[start..end], allow_escape)
            .map_err(|e| illegal_syntax(expression, &e))?;
        let rest = rewrite(expression, &statements);
        Ok((InitBlock { statements }, rest))
    }

    /// Sets the variables of the statements on the exchange, in order.
    fn run(&self, exchange: &mut Exchange) -> CamelResult<()> {
        for s in &self.statements {
            let v = match &s.value {
                InitValue::Function(f) => {
                    exchange.set_variable(
                        &format!("{FUNCTION_VARIABLE_PREFIX}{}", s.name),
                        Value::Object(camel_util::value::ObjectRef::new(f.clone())),
                    );
                    continue;
                }
                InitValue::Constant(v) => v.clone(),
                InitValue::Expression(e) => e.evaluate_mut(exchange)?,
            };
            exchange.set_variable(&s.name, v);
        }
        Ok(())
    }

    /// Removes the init functions from the exchange (they are local to the expression).
    fn clear(&self, exchange: &mut Exchange) {
        for s in &self.statements {
            if matches!(s.value, InitValue::Function(_)) {
                exchange.remove_variable(&format!("{FUNCTION_VARIABLE_PREFIX}{}", s.name));
            }
        }
    }
}

/// An expression with an init block.
struct InitBlockExpression {
    block: InitBlock,
    expression: SharedExpression,
}

impl Expression for InitBlockExpression {
    /// Evaluates on a copy of the exchange (the variables need a mutable exchange).
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        let mut copy = exchange.clone();
        self.evaluate_mut(&mut copy)
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.block.run(exchange)?;
        let result = self.expression.evaluate_mut(exchange);
        self.block.clear(exchange);
        result
    }
}

/// A predicate with an init block.
struct InitBlockPredicate {
    block: InitBlock,
    predicate: SharedPredicate,
}

impl Predicate for InitBlockPredicate {
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        let mut copy = exchange.clone();
        self.block.run(&mut copy)?;
        self.predicate.matches(&copy)
    }
}

/// Parses an expression starting with an init block.
///
/// # Errors
///
/// A `SimpleIllegalSyntaxException` for an invalid block or expression.
pub fn parse_expression(expression: &str, allow_escape: bool) -> CamelResult<SharedExpression> {
    let (block, rest) = InitBlock::parse(expression, allow_escape)?;
    let parsed = SimpleExpressionParser::new(&rest, allow_escape)
        .parse_expression()
        .map_err(|e| illegal_syntax(&rest, &e))?;
    Ok(Arc::new(InitBlockExpression {
        block,
        expression: parsed,
    }))
}

/// Parses a predicate starting with an init block.
///
/// # Errors
///
/// A `SimpleIllegalSyntaxException` for an invalid block or predicate.
pub fn parse_predicate(expression: &str, allow_escape: bool) -> CamelResult<SharedPredicate> {
    let (block, rest) = InitBlock::parse(expression, allow_escape)?;
    let parsed = SimplePredicateParser::new(&rest, allow_escape)
        .parse_predicate()
        .map_err(|e| illegal_syntax(&rest, &e))?;
    Ok(Arc::new(InitBlockPredicate {
        block,
        predicate: parsed,
    }))
}
