//! # camel-ref
//!
//! The ref component (Java `RefComponent`): `ref:name` is the endpoint bound in the registry
//! with the name (see [`endpoint_value`](camel_api::endpoint::endpoint_value)), so routes can
//! refer to an endpoint configured in code. The producers and consumers are the ones of the
//! referenced endpoint. This crate is the Rust port of the Java `components/camel-ref` module.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::CamelContext;
use camel_api::endpoint::{endpoint_of, Component, SharedEndpoint};
use camel_api::error::{CamelError, CamelResult};
use camel_api::service::Service;
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::Arc;

/// The name of the referenced endpoint: the uri after the scheme, without a leading `//`
/// (Java `RefComponent.createEndpoint`).
///
/// # Examples
///
/// ```
/// assert_eq!("foo", camel_ref::ref_name("ref:foo"));
/// assert_eq!("foo", camel_ref::ref_name("ref://foo"));
/// ```
pub fn ref_name(uri: &str) -> &str {
    let name = uri.split_once(':').map_or(uri, |(_, n)| n);
    name.strip_prefix("//").unwrap_or(name)
}

/// The ref component: resolves `ref:name` to the endpoint bound in the registry.
#[derive(Default)]
pub struct RefComponent;

impl RefComponent {
    /// Creates the component.
    pub fn new() -> Self {
        RefComponent
    }
}

impl Service for RefComponent {}

impl Component for RefComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        _remaining: &str,
        _parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        let name = ref_name(uri);
        let not_found =
            || CamelError::no_such_bean_of_type(Some(name), "org.apache.camel.Endpoint");
        let registry = context.registry().ok_or_else(not_found)?;
        registry
            .lookup_by_name(name)
            .as_ref()
            .and_then(endpoint_of)
            .ok_or_else(not_found)
    }

    fn default_name(&self) -> Option<&str> {
        Some("ref")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
