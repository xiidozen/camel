//! # camel-core-reifier
//!
//! Turns the route model ([`RouteDefinition`]) into the processors that run it (Java
//! `org.apache.camel.reifier`: `RouteReifier`, `ProcessorReifier` and one reifier per EIP).
//! Endpoints are resolved and language expressions are created with the CamelContext.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::processor::{
    ExpressionPredicate, Processor, SharedExpression, SharedPredicate, SharedProcessor,
};
use camel_api::spi::registry::lookup_bean;
use camel_core_model::expression::ExpressionDefinition;
use camel_core_model::processor::{Eip, ProcessRef, ProcessorDefinition};
use camel_core_model::route::RouteDefinition;
use camel_core_processor::choice::ChoiceProcessor;
use camel_core_processor::filter::FilterProcessor;
use camel_core_processor::log_processor::LogProcessor;
use camel_core_processor::pipeline::Pipeline;
use camel_core_processor::send::SendProcessor;
use camel_core_processor::transform::{
    RemoveHeaderProcessor, RemoveHeadersProcessor, SetBodyProcessor, SetHeaderProcessor,
    SetPropertyProcessor, SetVariableProcessor, StopProcessor, ThrowExceptionProcessor,
};
use camel_util::value::Value;
use std::sync::Arc;

/// Creates the expression of the definition: a language expression is created by its language
/// from the context.
///
/// # Errors
///
/// Returns an error if the language is unknown or the expression is invalid.
pub fn reify_expression(
    context: &Arc<dyn CamelContext>,
    def: &ExpressionDefinition,
) -> CamelResult<SharedExpression> {
    match def {
        ExpressionDefinition::Language {
            language,
            expression,
        } => {
            let e = context
                .resolve_language(language)?
                .create_expression(expression)?;
            e.init(context)?;
            Ok(e)
        }
        ExpressionDefinition::Expression(e) => {
            e.init(context)?;
            Ok(e.clone())
        }
        ExpressionDefinition::Predicate(p) => {
            p.init(context)?;
            let p = p.clone();
            let name = p.describe();
            Ok(camel_support::builder::named_expression(name, move |e| {
                Ok(Value::Bool(p.matches(e)?))
            }))
        }
    }
}

/// Creates the predicate of the definition (an expression is a predicate by its value).
///
/// # Errors
///
/// Returns an error if the language is unknown or the predicate is invalid.
pub fn reify_predicate(
    context: &Arc<dyn CamelContext>,
    def: &ExpressionDefinition,
) -> CamelResult<SharedPredicate> {
    match def {
        ExpressionDefinition::Language {
            language,
            expression,
        } => {
            let p = context
                .resolve_language(language)?
                .create_predicate(expression)?;
            p.init(context)?;
            Ok(p)
        }
        ExpressionDefinition::Expression(e) => {
            e.init(context)?;
            Ok(Arc::new(ExpressionPredicate(e.clone())))
        }
        ExpressionDefinition::Predicate(p) => {
            p.init(context)?;
            Ok(p.clone())
        }
    }
}

/// The context of reifying one route.
pub struct RouteReifier<'a> {
    context: &'a Arc<dyn CamelContext>,
    route_id: &'a str,
}

impl<'a> RouteReifier<'a> {
    /// A reifier for the route with the id.
    pub fn new(context: &'a Arc<dyn CamelContext>, route_id: &'a str) -> Self {
        RouteReifier { context, route_id }
    }

    /// Creates the processor of the steps: a pipeline (disabled steps are left out).
    ///
    /// # Errors
    ///
    /// Returns the error of the first step that cannot be created.
    pub fn steps(&self, steps: &[ProcessorDefinition]) -> CamelResult<SharedProcessor> {
        let mut processors = Vec::new();
        for s in steps.iter().filter(|s| !s.disabled) {
            processors.push(self.step(s)?);
        }
        Ok(if processors.len() == 1 {
            processors.pop().unwrap()
        } else {
            Arc::new(Pipeline::new(processors))
        })
    }

    fn expr(&self, def: &ExpressionDefinition) -> CamelResult<SharedExpression> {
        reify_expression(self.context, def)
    }

    /// Creates the processor of the step.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint, expression or bean of the step cannot be resolved.
    pub fn step(&self, def: &ProcessorDefinition) -> CamelResult<SharedProcessor> {
        let p: SharedProcessor = match &def.eip {
            Eip::To { uri, pattern } => {
                Arc::new(SendProcessor::new(self.context.endpoint(uri)?, *pattern))
            }
            Eip::Filter {
                predicate,
                steps,
                status_property_name,
            } => {
                let mut f = FilterProcessor::new(
                    reify_predicate(self.context, predicate)?,
                    self.steps(steps)?,
                );
                if let Some(n) = status_property_name {
                    f = f.with_status_property_name(n);
                }
                Arc::new(f)
            }
            Eip::Choice { whens, otherwise } => {
                let mut filters = Vec::new();
                for w in whens {
                    filters.push(FilterProcessor::new(
                        reify_predicate(self.context, &w.predicate)?,
                        self.steps(&w.steps)?,
                    ));
                }
                let otherwise = match otherwise {
                    Some(o) => Some(self.steps(o)?),
                    None => None,
                };
                Arc::new(ChoiceProcessor::new(filters, otherwise))
            }
            Eip::Pipeline { steps } => {
                let mut processors = Vec::new();
                for s in steps.iter().filter(|s| !s.disabled) {
                    processors.push(self.step(s)?);
                }
                Arc::new(Pipeline::new(processors))
            }
            Eip::SetBody(e) | Eip::Transform(e) => Arc::new(SetBodyProcessor::new(self.expr(e)?)),
            Eip::SetHeader { name, expression } => {
                Arc::new(SetHeaderProcessor::new(name, self.expr(expression)?))
            }
            Eip::SetProperty { name, expression } => {
                Arc::new(SetPropertyProcessor::new(name, self.expr(expression)?))
            }
            Eip::SetVariable { name, expression } => {
                Arc::new(SetVariableProcessor::new(name, self.expr(expression)?))
            }
            Eip::RemoveHeader(name) => Arc::new(RemoveHeaderProcessor::new(name)),
            Eip::RemoveHeaders {
                pattern,
                exclude_patterns,
            } => {
                let excludes: Vec<&str> = exclude_patterns.iter().map(String::as_str).collect();
                Arc::new(RemoveHeadersProcessor::new(pattern, &excludes))
            }
            Eip::Log {
                message,
                level,
                log_name,
            } => {
                let e = self.expr(&ExpressionDefinition::language("simple", message))?;
                Arc::new(LogProcessor::new(
                    e,
                    log_name.as_deref().unwrap_or(self.route_id),
                    *level,
                ))
            }
            Eip::Stop => Arc::new(StopProcessor),
            Eip::Process(ProcessRef::Processor(p)) => p.clone(),
            Eip::Process(ProcessRef::Ref(name)) => {
                let registry = self.context.registry().ok_or_else(|| {
                    CamelError::no_such_bean_of_type(Some(name), "org.apache.camel.Processor")
                })?;
                lookup_bean::<Arc<dyn Processor>>(registry.as_ref(), name)
                    .map(|p| (*p).clone())
                    .ok_or_else(|| {
                        CamelError::no_such_bean_of_type(Some(name), "org.apache.camel.Processor")
                    })?
            }
            Eip::ThrowException(e) => Arc::new(ThrowExceptionProcessor::new(e.clone())),
        };
        Ok(p)
    }
}

/// Creates the processor of the route: resolves the endpoints and expressions of all steps.
///
/// # Errors
///
/// Returns a `FailedToCreateRouteException` naming the failing step (`at: >>> to[x] <<<`).
pub fn reify_route(
    context: &Arc<dyn CamelContext>,
    route_id: &str,
    def: &RouteDefinition,
) -> CamelResult<SharedProcessor> {
    let reifier = RouteReifier::new(context, route_id);
    let mut processors = Vec::new();
    for step in def.steps.iter().filter(|s| !s.disabled) {
        let p = reifier.step(step).map_err(|e| {
            let mut route = def.clone();
            route.id = Some(route_id.to_string());
            CamelError::failed_to_create_route_at(
                route_id,
                def.source_location.as_deref(),
                &route.to_string(),
                &step.to_string(),
                e,
            )
        })?;
        processors.push(p);
    }
    Ok(Arc::new(Pipeline::new(processors)))
}
