//! The bean language and bean binding (port of `components/camel-bean`): calling the methods
//! of beans from the registry, as `${bean:greeter.hello}` in the simple language or the `bean`
//! language itself (`greeter.hello`, `greeter?method=hello`, `greeter::hello('World')`).
//!
//! Java finds the methods of a bean by reflection; a Rust bean implements
//! [`ExchangeBean`](camel_api::bean_binding::ExchangeBean), or is a [`MethodBean`] built from
//! closures, one per method (see [`camel_api::bean_binding`] for the parameter binding).
//!
//! # Examples
//!
//! ```
//! use camel_bean::MethodBean;
//! use camel_core::prelude::*;
//!
//! let context = camel_core::default_context();
//! let greeter = MethodBean::new("Greeter")
//!     .method("hello", |inv| {
//!         let name = inv.parameter_as::<String>(0)?.unwrap_or_default();
//!         Ok(Value::from(format!("Hello {name}")))
//!     });
//! context.registry().unwrap().bind("greeter", greeter.into_value()).unwrap();
//!
//! let mut exchange = Exchange::new(context.as_shared());
//! exchange.message_mut().set_body("World");
//! let simple = context.resolve_language("simple").unwrap();
//! let hello = simple.create_expression("${bean:greeter.hello}").unwrap();
//! assert_eq!(Value::from("Hello World"), hello.evaluate(&exchange).unwrap());
//! let hi = simple.create_expression("${bean:greeter.hello('Camel')}").unwrap();
//! assert_eq!(Value::from("Hello Camel"), hi.evaluate(&exchange).unwrap());
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod language;
pub mod method_bean;

pub use language::{BeanExpression, BeanLanguage};
pub use method_bean::MethodBean;
