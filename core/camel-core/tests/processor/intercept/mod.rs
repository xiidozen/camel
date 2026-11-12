//! Tests of `org.apache.camel.processor.intercept` (the interceptors of route builders).
//!
//! The Rust `interceptSendToEndpoint` intercepts the `to` steps of the routes of its route
//! builder; the Java cases intercepting the sending of a `ProducerTemplate` (Java wraps the
//! endpoint itself) are not ported yet.

mod intercept_from_test;
mod intercept_send_to_endpoint_test;
mod intercept_simple_route_test;
