//! # camel-stub
//!
//! The stub component (Java `StubComponent`): `stub:anyUri` stubs out any endpoint with an
//! in-memory seda queue, so routes can be tested without the real components. Any uri and any
//! parameters are accepted (the endpoints are lenient); only the queue options of the seda
//! component (`size`, `concurrentConsumers`, `blockWhenFull`, `discardWhenFull`,
//! `offerTimeout`) are used. This crate is the Rust port of the Java `components/camel-stub`
//! module.
//!
//! Not ported yet: the `shadow` and `shadowPattern` options, and the stub dev console.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::CamelContext;
use camel_api::endpoint::{Component, SharedEndpoint};
use camel_api::error::CamelResult;
use camel_api::service::Service;
use camel_seda::SedaComponent;
use camel_util::uri_support::Parameters;
use std::any::Any;
use std::sync::Arc;

/// The stub component: a seda component accepting any uri.
pub struct StubComponent {
    seda: SedaComponent,
}

impl Default for StubComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl StubComponent {
    /// Creates the component.
    pub fn new() -> Self {
        StubComponent {
            seda: SedaComponent::stub(),
        }
    }

    /// The seda component holding the queues.
    pub fn seda(&self) -> &SedaComponent {
        &self.seda
    }
}

impl Service for StubComponent {}

impl Component for StubComponent {
    fn create_endpoint(
        &self,
        context: &Arc<dyn CamelContext>,
        uri: &str,
        remaining: &str,
        parameters: &mut Parameters,
    ) -> CamelResult<SharedEndpoint> {
        self.seda
            .create_endpoint(context, uri, remaining, parameters)
    }

    fn default_name(&self) -> Option<&str> {
        Some("stub")
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
