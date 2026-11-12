//! Tests of `org.apache.camel.component.properties` (property placeholders).

mod properties_component_circular_reference_test;
mod properties_component_default_functions_test;
mod properties_component_escaped_test;
mod properties_component_function_test;
mod properties_component_ignore_missing_property_test;
mod properties_component_initial_properties_test;
mod properties_component_nested_test;
mod properties_component_only_use_default_values_test;
mod properties_component_override_properties_test;
mod properties_component_simple_test;
mod properties_parser_edge_cases_test;
mod properties_route_id_test;
mod property_placeholder_default_value_test;

use crate::support::ContextTestSupport;
use camel_core::model::expression::ExpressionDefinition;
use camel_core::prelude::*;

/// The `myproperties.properties` of the Java tests.
pub const MY_PROPERTIES: &str =
    "classpath:org/apache/camel/component/properties/myproperties.properties";

/// A context with the properties of `myproperties.properties` (Java `createCamelContext`).
pub fn with_location() -> ContextTestSupport {
    let t = ContextTestSupport::new();
    t.context
        .properties()
        .set_locations(&[MY_PROPERTIES])
        .unwrap();
    t
}

/// Adds the routes and starts the context.
pub async fn start(t: &ContextTestSupport, routes: impl RouteBuilder) {
    t.context
        .add_routes(routes)
        .await
        .expect("the routes of the test");
    t.context.start().await.expect("the context starts");
}

/// Java `context.resolvePropertyPlaceholders(text)`.
pub fn resolve(t: &ContextTestSupport, text: &str) -> Option<String> {
    t.context.resolve_property_placeholders(text).unwrap()
}

/// Java `constant("..")` of the DSL: the constant language, so the property placeholders of
/// the text are resolved (the Rust `constant(..)` is the value itself).
pub fn constant_text(text: &str) -> ExpressionDefinition {
    language("constant", text)
}
