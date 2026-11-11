//! The properties of the API types for the OGNL navigation of the simple language (the Java
//! getters found by reflection, such as `${exchange.exchangeId}`, `${exception.message}` or
//! `${exchange.message.body}`): [`Bean`] implementations of [`CamelError`], [`Exchange`] and
//! [`Message`], and the values that wrap them.

use crate::error::{error_type_name, CamelError};
use crate::exchange::Exchange;
use crate::message::Message;
use camel_util::value::{Bean, ObjectRef, Value, ValueMap};
use std::error::Error;

/// The Java class of an error, as a map with `name`, `simpleName` and `canonicalName`.
fn class_of(error: &(dyn Error + 'static)) -> Value {
    let name = error_type_name(error);
    let simple = name.rsplit('.').next().unwrap_or(&name).to_string();
    let mut m = ValueMap::new();
    m.insert("name".to_string(), Value::from(name.as_str()));
    m.insert("simpleName".to_string(), Value::from(simple));
    m.insert("canonicalName".to_string(), Value::from(name.as_str()));
    Value::Map(m)
}

impl Bean for CamelError {
    /// `message`, `localizedMessage`, `cause` and `class` (Java `Throwable` getters).
    fn property(&self, name: &str) -> Option<Value> {
        match name {
            "message" | "localizedMessage" => Some(
                self.message()
                    .map(Value::from)
                    .unwrap_or_else(|| Value::from(self.to_string())),
            ),
            "cause" => Some(match self.cause() {
                Some(c) => match c.downcast_ref::<CamelError>() {
                    Some(ce) => exception_value(ce),
                    None => Value::from(c.to_string()),
                },
                None => Value::Null,
            }),
            "class" => Some(class_of(self)),
            _ => None,
        }
    }
}

/// The error as a value (Java the `Exception` object): a [`Bean`] whose text is the Java
/// `toString()`, `type: message`.
pub fn exception_value(error: &CamelError) -> Value {
    Value::Object(
        ObjectRef::new(error.clone())
            .with_bean::<CamelError>()
            .with_display(|e: &CamelError, f| write!(f, "{}: {e}", error_type_name(e))),
    )
}

impl Bean for Message {
    /// `body`, `headers`, `messageId` and `messageTimestamp`; method `getHeader(name)`.
    fn property(&self, name: &str) -> Option<Value> {
        match name {
            "body" => Some(self.body().clone()),
            "headers" => Some(Value::Map(
                self.headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            )),
            "messageId" => Some(self.message_id().map(Value::from).unwrap_or_default()),
            "messageTimestamp" => Some(Value::Int(self.message_timestamp())),
            _ => None,
        }
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        match (method, args) {
            ("getHeader", [name]) => Some(Ok(self
                .header(&name.to_string())
                .cloned()
                .unwrap_or_default())),
            _ => None,
        }
    }
}

/// The message as a value: a [`Bean`] (see [`Bean for Message`](Message#impl-Bean-for-Message)).
pub fn message_value(message: &Message) -> Value {
    Value::bean(message.clone())
}

impl Bean for Exchange {
    /// `exchangeId`, `pattern`, `in`, `message`, `out`, `properties`, `variables`,
    /// `exception`, `fromEndpoint`, `fromRouteId`, `created`, `failed`, `rollbackOnly` and
    /// `transacted`; methods `getProperty(name)` and `getVariable(name)`.
    fn property(&self, name: &str) -> Option<Value> {
        let v = match name {
            "exchangeId" => Value::from(self.exchange_id()),
            "pattern" => Value::from(self.pattern().to_string()),
            "in" => message_value(self.in_message()),
            "message" => message_value(self.message()),
            "out" => self.out_message().map(message_value).unwrap_or_default(),
            "properties" => Value::Map(self.properties().clone()),
            "variables" => Value::Map(self.variables().clone()),
            "exception" => self.exception().map(exception_value).unwrap_or_default(),
            "fromEndpoint" => self.from_endpoint().map(Value::from).unwrap_or_default(),
            "fromRouteId" => self.from_route_id().map(Value::from).unwrap_or_default(),
            "created" => Value::Int(self.created()),
            "failed" => Value::Bool(self.is_failed()),
            "rollbackOnly" => Value::Bool(self.is_rollback_only()),
            "transacted" => Value::Bool(self.is_transacted()),
            // the Rust exchange is the Java DefaultExchange
            "class" => {
                let mut m = ValueMap::new();
                let name = "org.apache.camel.support.DefaultExchange";
                m.insert("name".to_string(), Value::from(name));
                m.insert("simpleName".to_string(), Value::from("DefaultExchange"));
                m.insert("canonicalName".to_string(), Value::from(name));
                Value::Map(m)
            }
            _ => return None,
        };
        Some(v)
    }

    fn invoke(&self, method: &str, args: &[Value]) -> Option<Result<Value, String>> {
        match (method, args) {
            ("getProperty", [name]) => Some(Ok(self
                .property(name.to_string().as_str())
                .cloned()
                .unwrap_or_default())),
            ("getVariable", [name]) => Some(Ok(self
                .variable(&name.to_string())
                .cloned()
                .unwrap_or_default())),
            _ => None,
        }
    }
}

/// The exchange as a value (Java `${exchange}`): a copy of the exchange (same exchange id) as
/// a [`Bean`] displayed as `Exchange[id]`.
pub fn exchange_value(exchange: &Exchange) -> Value {
    Value::bean_display(exchange.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StandaloneContext;
    use crate::error::ErrorKind;

    #[test]
    fn exception_value_is_a_bean() {
        let e = CamelError::illegal_argument("Just testing");
        let v = exception_value(&e);
        assert_eq!(
            "java.lang.IllegalArgumentException: Just testing",
            v.to_string()
        );
        let Value::Object(o) = &v else { panic!() };
        assert_eq!(
            Some(Value::from("Just testing")),
            o.bean_property("message")
        );
        assert!(o
            .downcast_ref::<CamelError>()
            .unwrap()
            .is(ErrorKind::IllegalArgument));
    }

    #[test]
    fn exchange_value_is_a_bean() {
        let mut e = Exchange::new(StandaloneContext::shared());
        e.message_mut().set_body("Hello");
        let v = exchange_value(&e);
        let Value::Object(o) = &v else { panic!() };
        assert_eq!(
            Some(Value::from(e.exchange_id())),
            o.bean_property("exchangeId")
        );
        let Some(Value::Object(m)) = o.bean_property("message") else {
            panic!()
        };
        assert_eq!(Some(Value::from("Hello")), m.bean_property("body"));
    }
}
