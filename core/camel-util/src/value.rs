//! A dynamically typed value, the Rust counterpart of `java.lang.Object` in Camel APIs.
//!
//! Camel passes untyped data around everywhere: message bodies, header and property maps
//! (`Map<String, Object>`), endpoint options and expression results. Java can store any object
//! there and inspect it at runtime with `instanceof`. Rust has no universal object type, so the
//! port uses [`Value`]: an enum covering the common data shapes (text, numbers, bytes, lists,
//! maps) plus [`Value::Object`], which can hold any `Send + Sync` Rust value and be downcast
//! back to its concrete type.
//!
//! # Examples
//!
//! ```
//! use camel_util::value::Value;
//!
//! let v = Value::from("Hello");
//! assert_eq!(Some("Hello"), v.as_str());
//! assert_eq!("[1, true, null]", Value::from(vec![Value::from(1), Value::from(true), Value::Null]).to_string());
//!
//! #[derive(Debug, PartialEq)]
//! struct Order { id: u32 }
//! let v = Value::object(Order { id: 7 });
//! assert_eq!(Some(&Order { id: 7 }), v.downcast_ref::<Order>());
//! ```

use indexmap::IndexMap;
use std::any::{type_name, Any};
use std::fmt;
use std::sync::Arc;

/// A map of named values in insertion order (the Java `LinkedHashMap<String, Object>`).
pub type ValueMap = IndexMap<String, Value>;

type DisplayFn =
    Arc<dyn Fn(&(dyn Any + Send + Sync), &mut fmt::Formatter<'_>) -> fmt::Result + Send + Sync>;

/// The properties and methods of a Rust value, for the OGNL navigation of the simple language
/// (`${body.name}`, `${header.order.getId()}`): Java finds them by reflection, Rust values
/// declare them by implementing this trait and being wrapped with [`Value::bean`].
///
/// # Examples
///
/// ```
/// use camel_util::value::{Bean, Value};
///
/// struct OrderLine { id: i64, name: String }
///
/// impl Bean for OrderLine {
///     fn property(&self, name: &str) -> Option<Value> {
///         match name {
///             "id" => Some(Value::Int(self.id)),
///             "name" => Some(Value::from(self.name.as_str())),
///             _ => None,
///         }
///     }
/// }
///
/// let v = Value::bean(OrderLine { id: 123, name: "Camel in Action".into() });
/// let Value::Object(o) = &v else { unreachable!() };
/// assert_eq!(Some(Value::Int(123)), o.bean_property("id"));
/// ```
pub trait Bean: Any + Send + Sync {
    /// The value of the property (Java getter `getName()` or `isName()` for `name`), if the
    /// bean has it.
    fn property(&self, name: &str) -> Option<Value> {
        let _ = name;
        None
    }

    /// Invokes the method with the arguments (already evaluated), if the bean has it.
    ///
    /// # Returns
    ///
    /// `None` if the bean has no such method, else the result or the error message.
    fn invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        let _ = (method, args);
        None
    }
}

type PropertyFn = fn(&(dyn Any + Send + Sync), &str) -> Option<Value>;
type InvokeFn = fn(&(dyn Any + Send + Sync), &str, &[Value]) -> Option<Result<Value, String>>;

#[derive(Clone, Copy)]
struct BeanFns {
    property: PropertyFn,
    invoke: InvokeFn,
}

fn bean_fns<T: Bean>() -> BeanFns {
    fn property<T: Bean>(v: &(dyn Any + Send + Sync), name: &str) -> Option<Value> {
        v.downcast_ref::<T>()?.property(name)
    }
    fn invoke<T: Bean>(
        v: &(dyn Any + Send + Sync),
        method: &str,
        args: &[Value],
    ) -> Option<Result<Value, String>> {
        v.downcast_ref::<T>()?.invoke(method, args)
    }
    BeanFns {
        property: property::<T>,
        invoke: invoke::<T>,
    }
}

/// A reference counted, type erased Rust value stored in [`Value::Object`].
///
/// Cloning is cheap (the value is shared). Two `ObjectRef`s are equal when they point to the
/// same instance (Java reference equality, the default `Object.equals`).
#[derive(Clone)]
pub struct ObjectRef {
    inner: Arc<dyn Any + Send + Sync>,
    type_name: &'static str,
    display: Option<DisplayFn>,
    bean: Option<BeanFns>,
    equals: Option<EqualsFn>,
}

type EqualsFn = fn(&(dyn Any + Send + Sync), &(dyn Any + Send + Sync)) -> bool;

impl ObjectRef {
    /// Wraps the value.
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        ObjectRef {
            inner: Arc::new(value),
            type_name: type_name::<T>(),
            display: None,
            bean: None,
            equals: None,
        }
    }

    /// Wraps the value, using its [`fmt::Display`] implementation for `to_string()` (the
    /// equivalent of a Java class overriding `toString()`).
    pub fn new_display<T: Any + Send + Sync + fmt::Display>(value: T) -> Self {
        fn show<T: Any + fmt::Display>(
            v: &(dyn Any + Send + Sync),
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            match v.downcast_ref::<T>() {
                Some(t) => fmt::Display::fmt(t, f),
                None => f.write_str("?"),
            }
        }
        ObjectRef {
            inner: Arc::new(value),
            type_name: type_name::<T>(),
            display: Some(Arc::new(show::<T>)),
            bean: None,
            equals: None,
        }
    }

    /// Wraps an already shared value.
    pub fn from_arc<T: Any + Send + Sync>(value: Arc<T>) -> Self {
        ObjectRef {
            inner: value,
            type_name: type_name::<T>(),
            display: None,
            bean: None,
            equals: None,
        }
    }

    /// The Rust type name of the wrapped value (the Java `getClass().getName()`).
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// Uses the function as the `Display` (the Java `toString()`) of the wrapped value of type
    /// `T` (no change if the value is not a `T`).
    pub fn with_display<T: Any>(
        mut self,
        show: impl Fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result + Send + Sync + 'static,
    ) -> Self {
        if self.inner.is::<T>() {
            self.display = Some(Arc::new(move |v, f| match v.downcast_ref::<T>() {
                Some(t) => show(t, f),
                None => f.write_str("?"),
            }));
        }
        self
    }

    /// Makes the wrapped value a [`Bean`] for the OGNL navigation of the simple language.
    pub fn with_bean<T: Bean>(mut self) -> Self {
        if self.inner.is::<T>() {
            self.bean = Some(bean_fns::<T>());
        }
        self
    }

    /// Compares the wrapped value of type `T` with `PartialEq` (the equivalent of a Java class
    /// overriding `equals`), instead of by instance.
    pub fn with_equals<T: Any + PartialEq>(mut self) -> Self {
        fn eq<T: Any + PartialEq>(
            a: &(dyn Any + Send + Sync),
            b: &(dyn Any + Send + Sync),
        ) -> bool {
            match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            }
        }
        if self.inner.is::<T>() {
            self.equals = Some(eq::<T>);
        }
        self
    }

    /// Whether the wrapped value is a [`Bean`].
    pub fn is_bean(&self) -> bool {
        self.bean.is_some()
    }

    /// The property of the [`Bean`], if it is one and has the property.
    pub fn bean_property(&self, name: &str) -> Option<Value> {
        self.bean
            .and_then(|b| (b.property)(self.inner.as_ref(), name))
    }

    /// Invokes the method of the [`Bean`], if it is one and has the method.
    pub fn bean_invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        self.bean
            .and_then(|b| (b.invoke)(self.inner.as_ref(), method, args))
    }

    /// Returns a reference to the wrapped value if it is of type `T`.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    /// Whether the wrapped value is of type `T` (the Java `instanceof`).
    pub fn is<T: Any>(&self) -> bool {
        self.inner.is::<T>()
    }

    /// Returns the shared value as `Arc<T>` if it is of type `T`.
    pub fn downcast_arc<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.inner.clone().downcast::<T>().ok()
    }

    /// Whether both refer to the same instance.
    pub fn ptr_eq(&self, other: &ObjectRef) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// An identity hash code style text such as `0x7f3a2c10` (Java `ObjectHelper.getIdentityHashCode`).
    pub fn identity_hash_code(&self) -> String {
        format!(
            "0x{:x}",
            Arc::as_ptr(&self.inner) as *const () as usize as u32
        )
    }
}

impl fmt::Debug for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectRef({})", self)
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.display {
            Some(show) => show(self.inner.as_ref(), f),
            None => write!(f, "{}@{}", self.type_name, self.identity_hash_code()),
        }
    }
}

impl PartialEq for ObjectRef {
    /// The same instance, or equal values of a type registered with
    /// [`ObjectRef::with_equals`].
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
            || self
                .equals
                .is_some_and(|eq| eq(self.inner.as_ref(), other.inner.as_ref()))
    }
}

/// A dynamically typed value (see the [module documentation](self)).
#[derive(Debug, Clone, Default)]
pub enum Value {
    /// No value (Java `null`).
    #[default]
    Null,
    /// A boolean.
    Bool(bool),
    /// An integer number (Java `byte`, `short`, `int`, `long` and their wrappers).
    Int(i64),
    /// A floating point number (Java `float`, `double` and their wrappers).
    Float(f64),
    /// A text (Java `String`, `char`, `StringBuilder`, ...).
    Text(String),
    /// Binary data (Java `byte[]`).
    Bytes(Vec<u8>),
    /// A list of values (Java `List`, `Collection`, `Object[]`).
    List(Vec<Value>),
    /// A map of values in insertion order (Java `Map<String, Object>`).
    Map(ValueMap),
    /// Any other Rust value.
    Object(ObjectRef),
}

impl Value {
    /// Wraps any Rust value as a [`Value::Object`].
    pub fn object<T: Any + Send + Sync>(value: T) -> Value {
        Value::Object(ObjectRef::new(value))
    }

    /// Wraps a [`Bean`] as a [`Value::Object`] whose properties and methods the simple
    /// language can navigate.
    pub fn bean<T: Bean>(value: T) -> Value {
        Value::Object(ObjectRef::new(value).with_bean::<T>())
    }

    /// Wraps a displayable [`Bean`] (its `Display` is the Java `toString()`).
    pub fn bean_display<T: Bean + fmt::Display>(value: T) -> Value {
        Value::Object(ObjectRef::new_display(value).with_bean::<T>())
    }

    /// Wraps any displayable Rust value as a [`Value::Object`] that uses its `Display` for
    /// `to_string()`.
    pub fn object_display<T: Any + Send + Sync + fmt::Display>(value: T) -> Value {
        Value::Object(ObjectRef::new_display(value))
    }

    /// Whether this is [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns the text, if this is a [`Value::Text`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the boolean, if this is a [`Value::Bool`].
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the integer, if this is a [`Value::Int`].
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the number as `f64`, if this is a [`Value::Int`] or [`Value::Float`].
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Returns the bytes, if this is a [`Value::Bytes`].
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the list, if this is a [`Value::List`].
    pub fn as_list(&self) -> Option<&Vec<Value>> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// Returns the mutable list, if this is a [`Value::List`].
    pub fn as_list_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// Returns the map, if this is a [`Value::Map`].
    pub fn as_map(&self) -> Option<&ValueMap> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the mutable map, if this is a [`Value::Map`].
    pub fn as_map_mut(&mut self) -> Option<&mut ValueMap> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the wrapped object, if this is a [`Value::Object`].
    pub fn as_object(&self) -> Option<&ObjectRef> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// Returns a reference to the wrapped Rust value if this is a [`Value::Object`] of type `T`.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_object().and_then(|o| o.downcast_ref::<T>())
    }

    /// The name of the kind of value, such as `String` or `Integer` (used in error messages,
    /// like the Java class name).
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "Boolean",
            Value::Int(_) => "Long",
            Value::Float(_) => "Double",
            Value::Text(_) => "String",
            Value::Bytes(_) => "byte[]",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Object(o) => o.type_name(),
        }
    }

    /// Returns the size if the value is a collection, map, bytes or text (used by
    /// `CollectionHelper.size`, which returned `null` for other values).
    pub fn size(&self) -> Option<usize> {
        match self {
            Value::List(l) => Some(l.len()),
            Value::Map(m) => Some(m.len()),
            Value::Bytes(b) => Some(b.len()),
            _ => None,
        }
    }
}

/// Formats a `f64` like Java `Double.toString`: `1.0`, `0.5`, `1.0E7`, `NaN`, `Infinity`.
pub fn java_double_to_string(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let abs = f.abs();
    if (1e-3..1e7).contains(&abs) {
        let s = format!("{f}");
        if s.contains('.') {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        // scientific: mantissa always has a fraction digit, exponent with E
        let s = format!("{f:e}");
        let (mantissa, exp) = s.split_once('e').unwrap();
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}

impl fmt::Display for Value {
    /// Formats like Java `toString()`: `null`, `true`, `123`, `1.5`, text as-is, bytes as
    /// (lossy) UTF-8 text, lists as `[a, b]` and maps as `{k=v, k2=v2}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => f.write_str(&java_double_to_string(*x)),
            Value::Text(s) => f.write_str(s),
            Value::Bytes(b) => f.write_str(&String::from_utf8_lossy(b)),
            Value::List(l) => {
                f.write_str("[")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Value::Map(m) => {
                f.write_str("{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}={v}")?;
                }
                f.write_str("}")
            }
            Value::Object(o) => write!(f, "{o}"),
        }
    }
}

impl PartialEq for Value {
    /// Structural equality for data values (like Java `equals` on strings, numbers, lists and
    /// maps); [`Value::Object`]s are equal when they are the same instance. An `Int` and a
    /// `Float` are never equal (as `Long.equals(Double)` in Java).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a == b,
            _ => false,
        }
    }
}

macro_rules! from_int {
    ($($t:ty),*) => {
        $(impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Value::Int(v as i64)
            }
        })*
    };
}
from_int!(i8, i16, i32, i64, u8, u16, u32, isize);

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::Int(v as i64)
    }
}

impl From<usize> for Value {
    fn from(v: usize) -> Self {
        Value::Int(v as i64)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v as f64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<char> for Value {
    fn from(v: char) -> Self {
        Value::Text(v.to_string())
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_string())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::Text(v.clone())
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}

impl From<&[u8]> for Value {
    fn from(v: &[u8]) -> Self {
        Value::Bytes(v.to_vec())
    }
}

impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::List(v)
    }
}

impl From<Vec<String>> for Value {
    fn from(v: Vec<String>) -> Self {
        Value::List(v.into_iter().map(Value::Text).collect())
    }
}

impl From<Vec<&str>> for Value {
    fn from(v: Vec<&str>) -> Self {
        Value::List(v.into_iter().map(Value::from).collect())
    }
}

impl From<ValueMap> for Value {
    fn from(v: ValueMap) -> Self {
        Value::Map(v)
    }
}

impl From<ObjectRef> for Value {
    fn from(v: ObjectRef) -> Self {
        Value::Object(v)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map(Into::into).unwrap_or(Value::Null)
    }
}

impl FromIterator<Value> for Value {
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        Value::List(iter.into_iter().collect())
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<i64> for Value {
    fn eq(&self, other: &i64) -> bool {
        self.as_i64() == Some(*other)
    }
}

impl PartialEq<i32> for Value {
    fn eq(&self, other: &i32) -> bool {
        self.as_i64() == Some(*other as i64)
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        self.as_bool() == Some(*other)
    }
}

macro_rules! reverse_eq {
    ($($t:ty),*) => {
        $(impl PartialEq<Value> for $t {
            fn eq(&self, other: &Value) -> bool {
                other == self
            }
        })*
    };
}
reverse_eq!(&str, i64, i32, bool);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Order {
        id: u32,
    }

    impl fmt::Display for Order {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "Order[{}]", self.id)
        }
    }

    #[test]
    fn display_like_java() {
        assert_eq!("null", Value::Null.to_string());
        assert_eq!("true", Value::from(true).to_string());
        assert_eq!("123", Value::from(123).to_string());
        assert_eq!("1.0", Value::from(1.0).to_string());
        assert_eq!("1.5", Value::from(1.5).to_string());
        assert_eq!("1.0E7", Value::from(1e7).to_string());
        assert_eq!("1.5E-5", Value::from(1.5e-5).to_string());
        assert_eq!("NaN", Value::from(f64::NAN).to_string());
        assert_eq!("abc", Value::from("abc").to_string());
        assert_eq!("abc", Value::from(b"abc".to_vec()).to_string());
        let mut m = ValueMap::new();
        m.insert("a".into(), 1.into());
        m.insert("b".into(), vec!["x", "y"].into());
        assert_eq!("{a=1, b=[x, y]}", Value::from(m).to_string());
        assert_eq!(
            "Order[7]",
            Value::object_display(Order { id: 7 }).to_string()
        );
        assert!(Value::object(Order { id: 7 })
            .to_string()
            .contains("Order@0x"));
    }

    #[test]
    fn equality() {
        assert_eq!(Value::from("a"), Value::from("a"));
        assert_ne!(Value::from(1), Value::from(1.0));
        assert_eq!(Value::from(1), 1);
        assert_eq!(Value::from("a"), "a");
        let o = Value::object(Order { id: 1 });
        assert_eq!(o, o.clone());
        assert_ne!(o, Value::object(Order { id: 1 }));
        assert_eq!(Value::Null, Value::from(None::<i32>));
    }

    #[test]
    fn downcast() {
        let v = Value::object(Order { id: 3 });
        assert_eq!(Some(&Order { id: 3 }), v.downcast_ref::<Order>());
        assert_eq!(None, v.downcast_ref::<String>());
        assert!(v.as_object().unwrap().is::<Order>());
        assert_eq!(
            3,
            v.as_object().unwrap().downcast_arc::<Order>().unwrap().id
        );
        assert!(v.type_name().ends_with("Order"));
        let shared = ObjectRef::from_arc(Arc::new(Order { id: 4 }));
        assert_eq!(4, shared.downcast_ref::<Order>().unwrap().id);
    }

    #[test]
    fn accessors() {
        assert_eq!(Some(2.0), Value::from(2).as_f64());
        assert_eq!(Some(2), Value::from(vec![Value::Null, Value::Null]).size());
        assert_eq!(None, Value::from("ab").size());
        let mut l = Value::from(vec![Value::from(1)]);
        l.as_list_mut().unwrap().push(2.into());
        assert_eq!("[1, 2]", l.to_string());
        let collected: Value = (1..=3).map(Value::from).collect();
        assert_eq!("[1, 2, 3]", collected.to_string());
        assert_eq!("String", Value::from("x").type_name());
    }
}
