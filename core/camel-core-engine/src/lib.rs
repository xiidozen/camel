//! # camel-core-engine
//!
//! Adds the route model to the CamelContext (Java `DefaultCamelContext` of
//! `camel-core-engine`, with `addRoutes` and `addRouteDefinitions`): routes written with the Rust
//! DSL (or read from YAML / XML) are reified into processors and added to the engine.
//!
//! # Examples
//!
//! ```
//! use camel_api::context::CamelContext;
//! use camel_base_engine::context::DefaultCamelContext;
//! use camel_core_engine::ModelCamelContext;
//! use camel_core_model::builder::*;
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let context = DefaultCamelContext::new();
//! // routes need the components of their endpoints; here none is registered
//! let err = context
//!     .add_routes(|r: &mut Routes| {
//!         r.add(from("direct:start").to("mock:result"));
//!     })
//!     .await
//!     .unwrap_err();
//! assert_eq!(
//!     "Failed to create route: route1 at: >>> to[mock:result] <<< in route: \
//!      Route(route1)[From[direct:start] -> [to[mock:result]]] because: \
//!      No endpoint could be found for: mock:result, please check the needed Camel component is added to the CamelContext \
//!      (the mock component is in camel-mock; add camel-mock to the dependencies).",
//!     err.to_string()
//! );
//! # });
//! ```
//!
//! See the tests of this crate for complete routes with the `direct` and `mock` components.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::processor::BoxFuture;
use camel_api::spi::registry::Registry;
use camel_base_engine::context::DefaultCamelContext;
use camel_base_engine::route::RouteSpec;
use camel_core_model::builder::RouteBuilder;
use camel_core_model::route::RouteDefinition;
use camel_core_reifier::reify_route;

/// The route model operations of the CamelContext.
pub trait ModelCamelContext {
    /// Adds the routes of the builder (Java `addRoutes(RouteBuilder)`); they start right away if
    /// the context is started.
    ///
    /// # Returns
    ///
    /// The ids of the added routes.
    ///
    /// # Errors
    ///
    /// Returns a `FailedToCreateRouteException` if a route cannot be created; the routes before
    /// it are added.
    fn add_routes(&self, builder: impl RouteBuilder) -> BoxFuture<'_, CamelResult<Vec<String>>>;

    /// Adds the route definitions (Java `addRouteDefinitions`).
    ///
    /// # Errors
    ///
    /// Returns a `FailedToCreateRouteException` if a route cannot be created.
    fn add_route_definitions(
        &self,
        definitions: Vec<RouteDefinition>,
    ) -> BoxFuture<'_, CamelResult<Vec<String>>>;
}

impl ModelCamelContext for DefaultCamelContext {
    fn add_routes(&self, builder: impl RouteBuilder) -> BoxFuture<'_, CamelResult<Vec<String>>> {
        let definitions = builder.build();
        self.add_route_definitions(definitions)
    }

    fn add_route_definitions(
        &self,
        definitions: Vec<RouteDefinition>,
    ) -> BoxFuture<'_, CamelResult<Vec<String>>> {
        Box::pin(async move {
            let context: std::sync::Arc<dyn CamelContext> = self.as_shared();
            // the data formats of the route builders, bound by id (Java <dataFormats>)
            for def in &definitions {
                for (id, df) in &def.builder_data_formats {
                    let registry = self.simple_registry();
                    if registry.lookup_by_name(id).is_none() {
                        let created = camel_core_reifier::resolve_data_format(&context, df)?;
                        registry
                            .bind(id, camel_api::spi::data_format::data_format_value(created))?;
                    }
                }
            }
            let mut ids = Vec::new();
            for def in definitions {
                let (id, generated) = match &def.id {
                    // the property placeholders of the route id (Java RouteReifier)
                    Some(id) => (
                        camel_core_reifier::resolve_placeholders(&context, id)?,
                        false,
                    ),
                    None => (self.generate_route_id(), true),
                };
                let processor = reify_route(&context, &id, &def)?;
                let mut spec = RouteSpec::new(def.from.clone(), processor);
                spec = if generated {
                    spec.with_generated_id(&id)
                } else {
                    spec.with_id(&id)
                };
                spec.auto_startup = def.auto_startup;
                spec.startup_order = def.startup_order;
                spec.group = def.group.clone();
                spec.description = def.description.clone();
                spec.source_location = def.source_location.clone();
                spec.properties = def.properties.clone();
                // the policies of the route: the policies, or the policy beans of the registry
                for policy in &def.route_policies {
                    spec.route_policies.push(match policy {
                        camel_core_model::route::RoutePolicyRef::Policy(p) => p.clone(),
                        camel_core_model::route::RoutePolicyRef::Ref(name) => context
                            .registry()
                            .and_then(|r| r.lookup_by_name(name))
                            .and_then(|v| camel_api::spi::route_policy::route_policy_of(&v))
                            .ok_or_else(|| CamelError::no_such_bean(name))?,
                    });
                }
                ids.push(self.add_route(spec).await.map_err(|e| {
                    if e.is(camel_api::error::ErrorKind::FailedToCreateRoute) {
                        e
                    } else {
                        let mut d = def.clone();
                        d.id = Some(id.clone());
                        CamelError::failed_to_create_route(
                            &id,
                            def.source_location.as_deref(),
                            &d.to_string(),
                            e,
                        )
                    }
                })?);
            }
            Ok(ids)
        })
    }
}
