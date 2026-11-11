//! Building expressions and predicates in Rust code (Java `org.apache.camel.support.builder`:
//! `ExpressionBuilder`, `PredicateBuilder` and `ValueBuilder`).
//!
//! The functions [`header`], [`body`], [`constant`], [`exchange_property`] and [`variable`]
//! return a [`ValueBuilder`], whose methods build predicates:
//!
//! ```
//! use camel_api::context::StandaloneContext;
//! use camel_api::exchange::Exchange;
//! use camel_support::builder::{body, header};
//!
//! let p = header("foo").is_equal_to(123);
//! assert_eq!("header(foo) == 123", p.describe());
//! let mut e = Exchange::with_body(StandaloneContext::shared(), "Hello World");
//! e.message_mut().set_header("foo", "123");
//! assert!(p.matches(&e).unwrap());
//! assert!(body().contains("World").matches(&e).unwrap());
//! assert_eq!(Some("123 == 456".to_string()), header("foo").is_equal_to(456).failure_text(&e).unwrap());
//! ```

use crate::object_helper::{type_coerce_compare, type_coerce_contains, type_coerce_equals};
use camel_api::context::CamelContext;
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::processor::{Expression, Predicate, SharedExpression, SharedPredicate};
use camel_util::value::Value;
use std::cmp::Ordering;
use std::sync::Arc;

/// An expression from a closure with a text form.
struct NamedExpression<F> {
    name: String,
    f: F,
}

impl<F> Expression for NamedExpression<F>
where
    F: Fn(&Exchange) -> CamelResult<Value> + Send + Sync,
{
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        (self.f)(exchange)
    }

    fn describe(&self) -> String {
        self.name.clone()
    }
}

/// Creates an expression from a closure with its text form (used in log and error messages).
pub fn named_expression<F>(name: impl Into<String>, f: F) -> SharedExpression
where
    F: Fn(&Exchange) -> CamelResult<Value> + Send + Sync + 'static,
{
    Arc::new(NamedExpression {
        name: name.into(),
        f,
    })
}

/// The expression of the header (Java `ExpressionBuilder.headerExpression`), `header(name)`.
pub fn header_expression(name: &str) -> SharedExpression {
    let n = name.to_string();
    named_expression(format!("header({name})"), move |e| {
        Ok(e.message().header(&n).cloned().unwrap_or_default())
    })
}

/// The expression of the message body, `body`.
pub fn body_expression() -> SharedExpression {
    named_expression("body", |e| Ok(e.message().body().clone()))
}

/// The expression of the exchange property, `exchangeProperty(name)`.
pub fn exchange_property_expression(name: &str) -> SharedExpression {
    let n = name.to_string();
    named_expression(format!("exchangeProperty({name})"), move |e| {
        Ok(e.property(&n).cloned().unwrap_or_default())
    })
}

/// The expression of the variable, `variable(name)`.
pub fn variable_expression(name: &str) -> SharedExpression {
    let n = name.to_string();
    named_expression(format!("variable({name})"), move |e| {
        Ok(e.variable(&n).cloned().unwrap_or_default())
    })
}

/// The constant expression (its text form is the value).
pub fn constant_expression(value: impl Into<Value>) -> SharedExpression {
    Arc::new(camel_api::processor::ConstantExpression(value.into()))
}

/// The expression of the exchange id, `exchangeId`.
pub fn exchange_id_expression() -> SharedExpression {
    named_expression("exchangeId", |e| Ok(Value::from(e.exchange_id())))
}

/// The expression of the exchange pattern, `exchangePattern`.
pub fn exchange_pattern_expression() -> SharedExpression {
    named_expression("exchangePattern", |e| {
        Ok(Value::from(e.pattern().to_string()))
    })
}

/// Values usable as the right side of a predicate or as an expression: a [`ValueBuilder`], a
/// [`SharedExpression`], or a constant value.
pub trait IntoExpression {
    /// Converts to an expression.
    fn into_expression(self) -> SharedExpression;
}

impl IntoExpression for ValueBuilder {
    fn into_expression(self) -> SharedExpression {
        self.expression
    }
}

impl IntoExpression for SharedExpression {
    fn into_expression(self) -> SharedExpression {
        self
    }
}

macro_rules! constant_into_expression {
    ($($t:ty),*) => {
        $(impl IntoExpression for $t {
            fn into_expression(self) -> SharedExpression {
                constant_expression(self)
            }
        })*
    };
}

constant_into_expression!(&str, String, i32, i64, u32, u64, f64, bool, Value);

/// The comparison of a binary predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOp {
    Equal,
    EqualIgnoreCase,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Contains,
    ContainsIgnoreCase,
    StartsWith,
    EndsWith,
    IsNull,
    IsNotNull,
}

impl BinaryOp {
    fn text(self) -> &'static str {
        match self {
            BinaryOp::Equal => "==",
            BinaryOp::EqualIgnoreCase => "=~",
            BinaryOp::NotEqual => "!=",
            BinaryOp::Less => "<",
            BinaryOp::LessOrEqual => "<=",
            BinaryOp::Greater => ">",
            BinaryOp::GreaterOrEqual => ">=",
            BinaryOp::Contains => "contains",
            BinaryOp::ContainsIgnoreCase => "~~",
            BinaryOp::StartsWith => "startsWith",
            BinaryOp::EndsWith => "endsWith",
            BinaryOp::IsNull => "is",
            BinaryOp::IsNotNull => "is not",
        }
    }

    fn matches(self, l: &Value, r: &Value) -> bool {
        let both_null_or = |f: &dyn Fn() -> bool| {
            if l.is_null() && r.is_null() {
                true
            } else if l.is_null() || r.is_null() {
                false
            } else {
                f()
            }
        };
        match self {
            BinaryOp::Equal => both_null_or(&|| type_coerce_equals(l, r, false)),
            BinaryOp::EqualIgnoreCase => both_null_or(&|| type_coerce_equals(l, r, true)),
            BinaryOp::NotEqual => {
                if l.is_null() && r.is_null() {
                    false
                } else if l.is_null() || r.is_null() {
                    true
                } else {
                    !type_coerce_equals(l, r, false)
                }
            }
            BinaryOp::Less => both_null_or(&|| type_coerce_compare(l, r) == Ordering::Less),
            BinaryOp::LessOrEqual => {
                both_null_or(&|| type_coerce_compare(l, r) != Ordering::Greater)
            }
            BinaryOp::Greater => both_null_or(&|| type_coerce_compare(l, r) == Ordering::Greater),
            BinaryOp::GreaterOrEqual => {
                both_null_or(&|| type_coerce_compare(l, r) != Ordering::Less)
            }
            BinaryOp::Contains => both_null_or(&|| type_coerce_contains(l, r, false)),
            BinaryOp::ContainsIgnoreCase => both_null_or(&|| type_coerce_contains(l, r, true)),
            BinaryOp::StartsWith => both_null_or(&|| l.to_string().starts_with(&r.to_string())),
            BinaryOp::EndsWith => both_null_or(&|| l.to_string().ends_with(&r.to_string())),
            BinaryOp::IsNull => l.is_null(),
            BinaryOp::IsNotNull => !l.is_null(),
        }
    }
}

/// A predicate comparing two expressions (Java `BinaryPredicateSupport`); its text form is
/// `left op right` and its failure text the evaluated values, such as `234 == 444`.
struct BinaryPredicate {
    left: SharedExpression,
    right: SharedExpression,
    op: BinaryOp,
}

impl Predicate for BinaryPredicate {
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        let l = self.left.evaluate(exchange)?;
        let r = self.right.evaluate(exchange)?;
        Ok(self.op.matches(&l, &r))
    }

    fn init(&self, context: &Arc<dyn CamelContext>) -> CamelResult<()> {
        self.left.init(context)?;
        self.right.init(context)
    }

    fn failure_text(&self, exchange: &Exchange) -> CamelResult<Option<String>> {
        let l = self.left.evaluate(exchange)?;
        let r = self.right.evaluate(exchange)?;
        Ok(if self.op.matches(&l, &r) {
            None
        } else {
            Some(format!("{l} {} {r}", self.op.text()))
        })
    }

    fn describe(&self) -> String {
        format!(
            "{} {} {}",
            self.left.describe(),
            self.op.text(),
            self.right.describe()
        )
    }
}

fn binary(left: SharedExpression, right: SharedExpression, op: BinaryOp) -> SharedPredicate {
    Arc::new(BinaryPredicate { left, right, op })
}

/// A predicate from a closure with its text form.
struct NamedPredicate<F> {
    name: String,
    f: F,
}

impl<F> Predicate for NamedPredicate<F>
where
    F: Fn(&Exchange) -> CamelResult<bool> + Send + Sync,
{
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        (self.f)(exchange)
    }

    fn describe(&self) -> String {
        self.name.clone()
    }
}

/// Creates a predicate from a closure with its text form.
pub fn named_predicate<F>(name: impl Into<String>, f: F) -> SharedPredicate
where
    F: Fn(&Exchange) -> CamelResult<bool> + Send + Sync + 'static,
{
    Arc::new(NamedPredicate {
        name: name.into(),
        f,
    })
}

/// The negation of the predicate, `not (p)`.
pub fn not(predicate: SharedPredicate) -> SharedPredicate {
    let name = format!("not ({})", predicate.describe());
    named_predicate(name, move |e| Ok(!predicate.matches(e)?))
}

/// Both predicates, `(a) and (b)`.
pub fn and(left: SharedPredicate, right: SharedPredicate) -> SharedPredicate {
    let name = format!("({}) and ({})", left.describe(), right.describe());
    named_predicate(name, move |e| Ok(left.matches(e)? && right.matches(e)?))
}

/// Either predicate, `(a) or (b)`.
pub fn or(left: SharedPredicate, right: SharedPredicate) -> SharedPredicate {
    let name = format!("({}) or ({})", left.describe(), right.describe());
    named_predicate(name, move |e| Ok(left.matches(e)? || right.matches(e)?))
}

/// Any of the predicates, `in ([a, b])`.
pub fn in_predicates(predicates: Vec<SharedPredicate>) -> SharedPredicate {
    let names: Vec<String> = predicates.iter().map(|p| p.describe()).collect();
    named_predicate(format!("in ([{}])", names.join(", ")), move |e| {
        for p in &predicates {
            if p.matches(e)? {
                return Ok(true);
            }
        }
        Ok(false)
    })
}

/// The predicate of the expression value (`true`, a non-empty text other than `false`, a
/// non-empty collection, ...), as Java `PredicateBuilder.toPredicate`.
pub fn to_predicate(expression: SharedExpression) -> SharedPredicate {
    Arc::new(camel_api::processor::ExpressionPredicate(expression))
}

/// A value from an expression, with methods building predicates and derived expressions
/// (Java `ValueBuilder`).
#[derive(Clone)]
pub struct ValueBuilder {
    expression: SharedExpression,
}

/// The header of the message.
pub fn header(name: &str) -> ValueBuilder {
    ValueBuilder::new(header_expression(name))
}

/// The body of the message.
pub fn body() -> ValueBuilder {
    ValueBuilder::new(body_expression())
}

/// The exchange property.
pub fn exchange_property(name: &str) -> ValueBuilder {
    ValueBuilder::new(exchange_property_expression(name))
}

/// The variable.
pub fn variable(name: &str) -> ValueBuilder {
    ValueBuilder::new(variable_expression(name))
}

/// A constant value.
pub fn constant(value: impl Into<Value>) -> ValueBuilder {
    ValueBuilder::new(constant_expression(value))
}

/// The exchange pattern (`InOnly` / `InOut`).
pub fn exchange_pattern() -> ValueBuilder {
    ValueBuilder::new(exchange_pattern_expression())
}

impl ValueBuilder {
    /// A value builder on the expression.
    pub fn new(expression: SharedExpression) -> Self {
        ValueBuilder { expression }
    }

    /// The expression.
    pub fn expression(&self) -> SharedExpression {
        self.expression.clone()
    }

    /// The text form of the expression.
    pub fn describe(&self) -> String {
        self.expression.describe()
    }

    fn op(&self, right: impl IntoExpression, op: BinaryOp) -> SharedPredicate {
        binary(self.expression.clone(), right.into_expression(), op)
    }

    /// The value equals the other (with type coercion), `left == right`.
    pub fn is_equal_to(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::Equal)
    }

    /// The value equals the other ignoring case, `left =~ right`.
    pub fn is_equal_to_ignore_case(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::EqualIgnoreCase)
    }

    /// The value does not equal the other, `left != right`.
    pub fn is_not_equal_to(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::NotEqual)
    }

    /// The value is less than the other.
    pub fn is_less_than(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::Less)
    }

    /// The value is less than or equal to the other.
    pub fn is_less_than_or_equal_to(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::LessOrEqual)
    }

    /// The value is greater than the other.
    pub fn is_greater_than(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::Greater)
    }

    /// The value is greater than or equal to the other.
    pub fn is_greater_than_or_equal_to(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::GreaterOrEqual)
    }

    /// The value (text or collection) contains the other.
    pub fn contains(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::Contains)
    }

    /// The value contains the other ignoring case, `left ~~ right`.
    pub fn contains_ignore_case(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::ContainsIgnoreCase)
    }

    /// The value (as text) starts with the other.
    pub fn starts_with(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::StartsWith)
    }

    /// The value (as text) ends with the other.
    pub fn ends_with(&self, value: impl IntoExpression) -> SharedPredicate {
        self.op(value, BinaryOp::EndsWith)
    }

    /// The value is `Null` (absent), `left is null`.
    pub fn is_null(&self) -> SharedPredicate {
        self.op(Value::Null, BinaryOp::IsNull)
    }

    /// The value is not `Null`, `left is not null`.
    pub fn is_not_null(&self) -> SharedPredicate {
        self.op(Value::Null, BinaryOp::IsNotNull)
    }

    /// The value (as text) matches the regular expression completely, `left.matches('re')`.
    /// An invalid expression never matches.
    pub fn regex(&self, pattern: &str) -> SharedPredicate {
        let expression = self.expression.clone();
        let re = regex::Regex::new(&format!("^(?:{pattern})$")).ok();
        named_predicate(
            format!("{}.matches('{pattern}')", expression.describe()),
            move |e| {
                let v = expression.evaluate(e)?;
                Ok(!v.is_null() && re.as_ref().is_some_and(|r| r.is_match(&v.to_string())))
            },
        )
    }

    /// The value is of the type, by its Java type name (`String`, `Long`, `Double`, `Boolean`,
    /// `byte[]`, `List`, `Map`, or the Rust type name of an object): `left instanceof
    /// java.lang.String`.
    pub fn is_instance_of(&self, type_name: &str) -> SharedPredicate {
        let expression = self.expression.clone();
        let t = type_name.to_string();
        let canonical = match type_name {
            "String" | "Long" | "Double" | "Boolean" | "Integer" => {
                format!("java.lang.{type_name}")
            }
            "List" | "Map" => format!("java.util.{type_name}"),
            other => other.to_string(),
        };
        named_predicate(
            format!("{} instanceof {canonical}", expression.describe()),
            move |e| {
                let v = expression.evaluate(e)?;
                Ok(!v.is_null()
                    && (v.type_name() == t || (t == "Integer" && v.type_name() == "Long")))
            },
        )
    }

    /// The value is one of the values (with type coercion). When the value is a number, a
    /// value given as text is compared as the number it holds, keeping its decimals (Java
    /// `ExpressionBuilder.inValueExpression`): `2` is in `["2.0"]`, not in `["2.9"]`.
    pub fn is_in(&self, values: Vec<Value>) -> SharedPredicate {
        let preds = values
            .into_iter()
            .map(|v| self.is_equal_to(in_value_expression(v, self.expression.clone())))
            .collect();
        in_predicates(preds)
    }

    /// The value as a predicate (see [`to_predicate`]).
    pub fn as_predicate(&self) -> SharedPredicate {
        to_predicate(self.expression.clone())
    }

    /// The value with the text appended, `append(left, right)`.
    pub fn append(&self, value: impl IntoExpression) -> ValueBuilder {
        let (l, r) = (self.expression.clone(), value.into_expression());
        let name = format!("append({}, {})", l.describe(), r.describe());
        ValueBuilder::new(named_expression(name, move |e| {
            // Java string concatenation: a missing value is "null"
            let (a, b) = (l.evaluate(e)?, r.evaluate(e)?);
            Ok(Value::from(format!("{a}{b}")))
        }))
    }

    /// The value with the text prepended, `prepend(left, right)`.
    pub fn prepend(&self, value: impl IntoExpression) -> ValueBuilder {
        let (l, r) = (self.expression.clone(), value.into_expression());
        let name = format!("prepend({}, {})", l.describe(), r.describe());
        ValueBuilder::new(named_expression(name, move |e| {
            let (a, b) = (l.evaluate(e)?, r.evaluate(e)?);
            Ok(Value::from(format!("{b}{a}")))
        }))
    }

    /// The value converted to text, `convertToString(left)`.
    pub fn convert_to_string(&self) -> ValueBuilder {
        let l = self.expression.clone();
        let name = format!("convertToString({})", l.describe());
        ValueBuilder::new(named_expression(name, move |e| {
            let v = l.evaluate(e)?;
            Ok(if v.is_null() {
                v
            } else {
                Value::from(v.to_string())
            })
        }))
    }

    /// The value converted to an integer (`Null` if not convertible), `convertTo(left, Long)`.
    pub fn convert_to_long(&self) -> ValueBuilder {
        let l = self.expression.clone();
        let name = format!("convertTo({}, Long)", l.describe());
        ValueBuilder::new(named_expression(name, move |e| {
            let v = l.evaluate(e)?;
            Ok(<i64 as camel_api::convert::FromValue>::from_value(&v)
                .ok()
                .flatten()
                .map(Value::Int)
                .unwrap_or_default())
        }))
    }

    /// The value with the regular expression replaced, `regexReplaceAll(left, re, replacement)`.
    /// An invalid expression leaves the value unchanged.
    pub fn regex_replace_all(&self, pattern: &str, replacement: &str) -> ValueBuilder {
        let l = self.expression.clone();
        let re = regex::Regex::new(pattern).ok();
        let replacement = crate::builder::java_replacement(replacement);
        let name = format!(
            "regexReplaceAll({}, {pattern}, {})",
            l.describe(),
            replacement
        );
        ValueBuilder::new(named_expression(name, move |e| {
            let v = l.evaluate(e)?;
            Ok(match (&re, v.is_null()) {
                (Some(r), false) => Value::from(
                    r.replace_all(&v.to_string(), replacement.as_str())
                        .into_owned(),
                ),
                _ => v,
            })
        }))
    }
}

/// Converts a Java regex replacement (`$1`, `\$`) to the `regex` crate syntax (`${1}`, `$$`).
pub(crate) fn java_replacement(replacement: &str) -> String {
    let mut out = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('$') => out.push_str("$$"),
                Some(other) => out.push(other),
                None => {}
            },
            '$' => {
                let mut group = String::new();
                while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    group.push(*d);
                    chars.next();
                }
                if group.is_empty() {
                    out.push_str("$$");
                } else {
                    out.push_str(&format!("${{{group}}}"));
                }
            }
            other => out.push(other),
        }
    }
    out
}

impl Expression for ValueBuilder {
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        self.expression.evaluate(exchange)
    }

    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.expression.evaluate_mut(exchange)
    }

    fn init(&self, context: &Arc<dyn CamelContext>) -> CamelResult<()> {
        self.expression.init(context)
    }

    fn describe(&self) -> String {
        self.expression.describe()
    }
}

/// A value of the `in` predicate: as a number when the left value is a number and the value is
/// text holding a decimal number (Java `ExpressionBuilder.inValueExpression`).
fn in_value_expression(value: Value, left: SharedExpression) -> SharedExpression {
    let constant = constant_expression(value.clone());
    named_expression(constant.describe(), move |e| {
        if let Value::Text(text) = &value {
            if matches!(left.evaluate(e)?, Value::Int(_) | Value::Float(_)) {
                if let Some(number) = decimal_or_none(text) {
                    return Ok(number);
                }
            }
        }
        Ok(value.clone())
    })
}

/// The number of a decimal text (Java `new BigDecimal(text.trim())`): an integer when it fits,
/// else a floating point number; `None` when the text is not a decimal number.
fn decimal_or_none(text: &str) -> Option<Value> {
    let number = text.trim();
    let digits = number.strip_prefix(['+', '-']).unwrap_or(number);
    let (mantissa, exponent) = match digits.split_once(['e', 'E']) {
        Some((m, x)) => (m, Some(x)),
        None => (digits, None),
    };
    let (int_part, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let valid_mantissa = !(int_part.is_empty() && fraction.is_empty())
        && int_part.chars().all(|c| c.is_ascii_digit())
        && fraction.chars().all(|c| c.is_ascii_digit());
    let valid_exponent = match exponent {
        None => true,
        Some(x) => {
            let x = x.strip_prefix(['+', '-']).unwrap_or(x);
            !x.is_empty() && x.chars().all(|c| c.is_ascii_digit())
        }
    };
    if !valid_mantissa || !valid_exponent {
        return None;
    }
    if !mantissa.contains('.') && exponent.is_none() {
        if let Ok(i) = number.parse::<i64>() {
            return Some(Value::Int(i));
        }
    }
    number.parse::<f64>().ok().map(Value::Float)
}

#[cfg(test)]
mod tests {
    #[test]
    fn decimal_or_none_parses_decimals() {
        use super::decimal_or_none;
        assert_eq!(Some(Value::Int(2)), decimal_or_none(" 2 "));
        assert_eq!(Some(Value::Float(2.0)), decimal_or_none("2.0"));
        assert_eq!(Some(Value::Float(-0.5)), decimal_or_none("-.5"));
        assert_eq!(Some(Value::Float(1000.0)), decimal_or_none("1e3"));
        assert_eq!(None, decimal_or_none("NaN"));
        assert_eq!(None, decimal_or_none("inf"));
        assert_eq!(None, decimal_or_none("1,5"));
        assert_eq!(None, decimal_or_none("."));
        assert_eq!(None, decimal_or_none(""));
    }

    use super::*;
    use camel_api::context::StandaloneContext;

    fn ex() -> Exchange {
        let mut e = Exchange::with_body(StandaloneContext::shared(), "Hello World");
        e.message_mut().set_header("foo", 123);
        e.message_mut().set_header("name", "Camel");
        e.set_property("p", "x");
        e
    }

    #[test]
    fn binary_predicates() {
        let e = ex();
        assert!(header("foo").is_equal_to("123").matches(&e).unwrap());
        assert!(header("foo").is_not_equal_to(5).matches(&e).unwrap());
        assert!(header("foo").is_greater_than(100).matches(&e).unwrap());
        assert!(header("foo")
            .is_greater_than_or_equal_to(123)
            .matches(&e)
            .unwrap());
        assert!(header("foo").is_less_than("200").matches(&e).unwrap());
        assert!(header("foo")
            .is_less_than_or_equal_to(123)
            .matches(&e)
            .unwrap());
        assert!(header("name")
            .is_equal_to_ignore_case("camel")
            .matches(&e)
            .unwrap());
        assert!(body().contains_ignore_case("world").matches(&e).unwrap());
        assert!(
            body().starts_with("Hello").matches(&e).unwrap()
                && body().ends_with("World").matches(&e).unwrap()
        );
        assert!(header("missing").is_null().matches(&e).unwrap());
        assert!(header("foo").is_not_null().matches(&e).unwrap());
        assert!(!header("missing").is_equal_to("x").matches(&e).unwrap());
        assert!(header("missing").is_not_equal_to("x").matches(&e).unwrap());
        assert!(exchange_property("p").is_equal_to("x").matches(&e).unwrap());
        assert!(variable("v").is_null().matches(&e).unwrap());
        assert!(header("foo")
            .is_in(vec![Value::from(1), Value::from("123")])
            .matches(&e)
            .unwrap());
        assert_eq!("header(bar) is null", header("bar").is_null().describe());
        assert_eq!(
            Some("123 is null".into()),
            header("foo").is_null().failure_text(&e).unwrap()
        );
        assert_eq!(None, header("foo").is_not_null().failure_text(&e).unwrap());
    }

    #[test]
    fn combined() {
        let e = ex();
        let p = and(header("foo").is_equal_to(123), not(body().contains("Bye")));
        assert!(p.matches(&e).unwrap());
        assert_eq!(
            "(header(foo) == 123) and (not (body contains Bye))",
            p.describe()
        );
        assert!(or(
            header("foo").is_equal_to(1),
            header("name").is_equal_to("Camel")
        )
        .matches(&e)
        .unwrap());
        assert!(header("name").regex("C.*l").matches(&e).unwrap());
        assert!(!header("name").regex("C").matches(&e).unwrap());
        assert_eq!(
            "header(name).matches('C.*l')",
            header("name").regex("C.*l").describe()
        );
        assert!(header("name").as_predicate().matches(&e).unwrap());
        assert!(header("name").is_instance_of("String").matches(&e).unwrap());
        assert!(!header("foo").is_instance_of("String").matches(&e).unwrap());
        assert_eq!(
            "header(foo) instanceof java.lang.String",
            header("foo").is_instance_of("String").describe()
        );
        assert!(!constant(false).as_predicate().matches(&e).unwrap());
    }

    #[test]
    fn value_expressions() {
        let e = ex();
        assert_eq!(
            Value::from("Hello World!"),
            body().append("!").evaluate(&e).unwrap()
        );
        assert_eq!(
            Value::from(">Hello World"),
            body().prepend(">").evaluate(&e).unwrap()
        );
        assert_eq!(
            Value::from("123"),
            header("foo").convert_to_string().evaluate(&e).unwrap()
        );
        assert_eq!(
            Value::Int(123),
            constant("123").convert_to_long().evaluate(&e).unwrap()
        );
        assert_eq!(
            Value::from("Hello Camel"),
            body()
                .regex_replace_all("W(or)ld", "Camel")
                .evaluate(&e)
                .unwrap()
        );
        assert_eq!(
            Value::from("orx"),
            constant("World")
                .regex_replace_all("W(or)ld", "$1x")
                .evaluate(&e)
                .unwrap()
        );
        assert_eq!("append(body,  World)", body().append(" World").describe());
        assert_eq!(
            Value::from("InOnly"),
            exchange_pattern().evaluate(&e).unwrap()
        );
        assert!(!exchange_id_expression().evaluate(&e).unwrap().is_null());
        assert_eq!("$$x${1}", java_replacement("\\$x$1"));
    }
}
