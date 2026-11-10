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
use camel_core_model::errorhandler::{
    ErrorHandlerDefinition, ErrorHandlerKind, RedeliveryPolicyDefinition,
};
use camel_core_model::expression::ExpressionDefinition;
use camel_core_model::processor::{Eip, ProcessRef, ProcessorDefinition};
use camel_core_model::route::RouteDefinition;
use camel_core_processor::choice::ChoiceProcessor;
use camel_core_processor::errorhandler::exception_policy::ExceptionPolicy;
use camel_core_processor::errorhandler::redelivery_policy::RedeliveryPolicyOverrides;
use camel_core_processor::errorhandler::{ErrorHandlerConfig, RedeliveryErrorHandler};
use camel_core_processor::filter::FilterProcessor;
use camel_core_processor::log_processor::LogProcessor;
use camel_core_processor::pipeline::Pipeline;
use camel_core_processor::send::SendProcessor;
use camel_core_processor::transform::{
    RemoveHeaderProcessor, RemoveHeadersProcessor, SetBodyProcessor, SetHeaderProcessor,
    SetPropertyProcessor, SetVariableProcessor, StopProcessor, ThrowExceptionProcessor,
};
use camel_core_processor::try_catch::{CatchProcessor, FinallyProcessor, TryProcessor};
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
    error_handler: Option<Arc<ErrorHandlerConfig>>,
}

impl<'a> RouteReifier<'a> {
    /// A reifier for the route with the id (without error handler).
    pub fn new(context: &'a Arc<dyn CamelContext>, route_id: &'a str) -> Self {
        RouteReifier {
            context,
            route_id,
            error_handler: None,
        }
    }

    /// A reifier wrapping each step in the error handler (Java channels with the error
    /// handler of the route).
    pub fn with_error_handler(
        context: &'a Arc<dyn CamelContext>,
        route_id: &'a str,
        error_handler: Option<Arc<ErrorHandlerConfig>>,
    ) -> Self {
        RouteReifier {
            context,
            route_id,
            error_handler,
        }
    }

    /// The same reifier without error handler (for the steps of `doTry` and `onException`,
    /// as Java `shouldWrapInErrorHandler`).
    fn without_error_handler(&self) -> RouteReifier<'a> {
        RouteReifier {
            context: self.context,
            route_id: self.route_id,
            error_handler: None,
        }
    }

    /// Creates the processor of the step, wrapped in the error handler if any.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint, expression or bean of the step cannot be resolved.
    pub fn step(&self, def: &ProcessorDefinition) -> CamelResult<SharedProcessor> {
        let p = self.create(def)?;
        Ok(match &self.error_handler {
            Some(config) => Arc::new(RedeliveryErrorHandler::new(p, config.clone())),
            None => p,
        })
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

    fn step_list(&self, steps: &[ProcessorDefinition]) -> CamelResult<Vec<SharedProcessor>> {
        let mut processors = Vec::new();
        for s in steps.iter().filter(|s| !s.disabled) {
            processors.push(self.step(s)?);
        }
        Ok(processors)
    }

    fn expr(&self, def: &ExpressionDefinition) -> CamelResult<SharedExpression> {
        reify_expression(self.context, def)
    }

    /// Creates the processor of the step (without error handler).
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint, expression or bean of the step cannot be resolved.
    pub fn create(&self, def: &ProcessorDefinition) -> CamelResult<SharedProcessor> {
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
            Eip::Process(r) => resolve_process_ref(self.context, r)?,
            Eip::ThrowException(e) => Arc::new(ThrowExceptionProcessor::new(e.clone())),
            Eip::DoTry {
                steps,
                catches,
                finally,
            } => {
                let inner = self.without_error_handler();
                let try_processor: SharedProcessor =
                    Arc::new(Pipeline::new(inner.step_list(steps)?));
                let mut clauses: Vec<SharedProcessor> = Vec::new();
                for c in catches {
                    let on_when = match &c.on_when {
                        Some(p) => Some(reify_predicate(self.context, p)?),
                        None => None,
                    };
                    let body: SharedProcessor = Arc::new(Pipeline::new(inner.step_list(&c.steps)?));
                    clauses.push(Arc::new(CatchProcessor::new(
                        c.exceptions.clone(),
                        body,
                        on_when,
                    )));
                }
                let finally = match finally {
                    Some(f) => Some(Arc::new(FinallyProcessor::new(Arc::new(Pipeline::new(
                        inner.step_list(f)?,
                    )))) as SharedProcessor),
                    None => None,
                };
                Arc::new(TryProcessor::new(try_processor, clauses, finally))
            }
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
    let error_handler = reify_error_handler(context, route_id, def).map_err(|e| {
        let mut route = def.clone();
        route.id = Some(route_id.to_string());
        CamelError::failed_to_create_route(
            route_id,
            def.source_location.as_deref(),
            &route.to_string(),
            e,
        )
    })?;
    let reifier = RouteReifier::with_error_handler(context, route_id, error_handler);
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

/// The processor of a `process` reference: the processor itself, or a bean from the registry.
///
/// # Errors
///
/// Returns a `NoSuchBeanException` if there is no such processor bean.
pub fn resolve_process_ref(
    context: &Arc<dyn CamelContext>,
    r: &ProcessRef,
) -> CamelResult<SharedProcessor> {
    match r {
        ProcessRef::Processor(p) => Ok(p.clone()),
        ProcessRef::Ref(name) => {
            let not_found =
                || CamelError::no_such_bean_of_type(Some(name), "org.apache.camel.Processor");
            let registry = context.registry().ok_or_else(not_found)?;
            lookup_bean::<Arc<dyn Processor>>(registry.as_ref(), name)
                .map(|p| (*p).clone())
                .ok_or_else(not_found)
        }
    }
}

fn overrides(d: &RedeliveryPolicyDefinition) -> RedeliveryPolicyOverrides {
    RedeliveryPolicyOverrides {
        maximum_redeliveries: d.maximum_redeliveries,
        redelivery_delay: d.redelivery_delay,
        use_exponential_back_off: d.use_exponential_back_off,
        back_off_multiplier: d.back_off_multiplier,
        maximum_redelivery_delay: d.maximum_redelivery_delay,
        retries_exhausted_log_level: d.retries_exhausted_log_level,
        retry_attempted_log_level: d.retry_attempted_log_level,
        log_stack_trace: d.log_stack_trace,
        log_handled: d.log_handled,
        log_exhausted: d.log_exhausted,
        delay_pattern: d.delay_pattern.clone(),
    }
}

fn optional_ref(
    context: &Arc<dyn CamelContext>,
    r: &Option<ProcessRef>,
) -> CamelResult<Option<SharedProcessor>> {
    r.as_ref()
        .map(|r| resolve_process_ref(context, r))
        .transpose()
}

fn optional_predicate(
    context: &Arc<dyn CamelContext>,
    p: &Option<ExpressionDefinition>,
) -> CamelResult<Option<SharedPredicate>> {
    p.as_ref().map(|p| reify_predicate(context, p)).transpose()
}

/// Creates the error handler of the route (Java `ErrorHandlerReifier`): the error handler of
/// the route or its builder (the default error handler if none), with the `onException`
/// policies of the route and its builder; `None` for no error handler.
///
/// # Errors
///
/// Returns an error if the dead letter endpoint, a processor or a predicate cannot be resolved.
pub fn reify_error_handler(
    context: &Arc<dyn CamelContext>,
    route_id: &str,
    def: &RouteDefinition,
) -> CamelResult<Option<Arc<ErrorHandlerConfig>>> {
    let eh = def
        .error_handler
        .clone()
        .unwrap_or_else(|| ErrorHandlerDefinition::new(ErrorHandlerKind::Default));
    let mut config = match &eh.kind {
        ErrorHandlerKind::NoErrorHandler => return Ok(None),
        ErrorHandlerKind::Default => ErrorHandlerConfig::default(),
        ErrorHandlerKind::DeadLetterChannel {
            uri,
            handle_new_exception,
        } => {
            let send: SharedProcessor = Arc::new(SendProcessor::new(context.endpoint(uri)?, None));
            let mut c = ErrorHandlerConfig::dead_letter_channel(uri, send);
            if let Some(d) = c.dead_letter.as_mut() {
                d.handle_new_exception = *handle_new_exception;
            }
            c
        }
    };
    config.redelivery_policy = overrides(&eh.redelivery).apply(&config.redelivery_policy);
    config.on_redelivery = optional_ref(context, &eh.on_redelivery)?;
    config.on_exception_occurred = optional_ref(context, &eh.on_exception_occurred)?;
    config.on_prepare_failure = optional_ref(context, &eh.on_prepare_failure)?;
    config.retry_while = optional_predicate(context, &eh.retry_while)?;
    if let Some(n) = &eh.log_name {
        config.logger_name = n.clone();
    }
    let inner = RouteReifier::new(context, route_id);
    let scoped = def
        .on_exceptions
        .iter()
        .map(|o| (o, Some(route_id.to_string())));
    let builder = def.builder_on_exceptions.iter().map(|o| (o, None));
    for (i, (o, scope)) in scoped.chain(builder).enumerate() {
        let processor = if o.steps.is_empty() {
            None
        } else {
            Some(Arc::new(Pipeline::new(inner.step_list(&o.steps)?)) as SharedProcessor)
        };
        config.exception_policies.push(Arc::new(ExceptionPolicy {
            id: format!("onException{}", i + 1),
            exceptions: o.exceptions.clone(),
            route_id: scope,
            on_when: optional_predicate(context, &o.on_when)?,
            retry_while: optional_predicate(context, &o.retry_while)?,
            handled: optional_predicate(context, &o.handled)?,
            continued: optional_predicate(context, &o.continued)?,
            redelivery: overrides(&o.redelivery),
            processor,
            on_redelivery: optional_ref(context, &o.on_redelivery)?,
            on_exception_occurred: optional_ref(context, &o.on_exception_occurred)?,
        }));
    }
    Ok(Some(Arc::new(config)))
}
