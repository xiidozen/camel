//! # camel-core-reifier
//!
//! Turns the route model ([`RouteDefinition`]) into the processors that run it (Java
//! `org.apache.camel.reifier`: `RouteReifier`, `ProcessorReifier` and one reifier per EIP).
//! Endpoints are resolved and language expressions are created with the CamelContext.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use camel_api::bean_binding::BeanProcessorFactory;
use camel_api::context::{context_plugin, CamelContext};
use camel_api::error::{CamelError, CamelResult};
use camel_api::processor::{
    ExpressionPredicate, Processor, SharedAggregationStrategy, SharedExpression, SharedPredicate,
    SharedProcessor,
};
use camel_api::spi::registry::lookup_bean;
use camel_core_model::errorhandler::{
    ErrorHandlerDefinition, ErrorHandlerKind, RedeliveryPolicyDefinition,
};
use camel_core_model::expression::ExpressionDefinition;
use camel_core_model::processor::{
    AggregationRef, Eip, IdempotentRepositoryRef, LoadBalancerDefinition, MulticastOptions,
    OnCompletionDefinition, ProcessRef, ProcessorDefinition, ResequenceMode, Steps,
};
use camel_core_model::route::RouteDefinition;
use camel_core_processor::aggregate::{AggregateOptions, AggregateProcessor};
use camel_core_processor::choice::ChoiceProcessor;
use camel_core_processor::claim_check::{ClaimCheckOperation, ClaimCheckProcessor};
use camel_core_processor::dynamic::{
    ConvertBodyProcessor, ConvertTarget, ConvertValueProcessor, RecipientListProcessor,
    SendDynamicProcessor, SetExchangePatternProcessor, WireTapProcessor,
};
use camel_core_processor::enricher::{EnrichProcessor, PollEnrichProcessor};
use camel_core_processor::errorhandler::exception_policy::ExceptionPolicy;
use camel_core_processor::errorhandler::redelivery_policy::RedeliveryPolicyOverrides;
use camel_core_processor::errorhandler::{ErrorHandlerConfig, RedeliveryErrorHandler};
use camel_core_processor::filter::FilterProcessor;
use camel_core_processor::idempotent::{IdempotentConsumerProcessor, IdempotentOptions};
use camel_core_processor::intercept::{
    InterceptFromProcessor, InterceptProcessor, InterceptSendToEndpointProcessor, Interceptor,
    SendInterceptor,
};
use camel_core_processor::loadbalancer::{FailoverOptions, LoadBalance, LoadBalancerProcessor};
use camel_core_processor::log_processor::LogProcessor;
use camel_core_processor::multicast::{
    DelayProcessor, MulticastConfig, MulticastProcessor, SplitProcessor,
};
use camel_core_processor::on_completion::{OnCompletionOptions, OnCompletionProcessor};
use camel_core_processor::pipeline::Pipeline;
use camel_core_processor::resequence::{ResequenceConfig, ResequenceProcessor};
use camel_core_processor::routing::{
    LoopProcessor, RoutingSlipProcessor, SamplingProcessor, SortProcessor, StepProcessor,
};
use camel_core_processor::send::SendProcessor;
use camel_core_processor::threads::{ThreadsConfig, ThreadsProcessor};
use camel_core_processor::throttle::{ThrottleProcessor, ThrottlingMode};
use camel_core_processor::transform::{
    RemoveHeaderProcessor, RemoveHeadersProcessor, RemovePropertiesProcessor,
    RemovePropertyProcessor, RemoveVariableProcessor, RollbackProcessor, ScriptProcessor,
    SetBodyProcessor, SetHeaderProcessor, SetHeadersProcessor, SetPropertyProcessor,
    SetVariableProcessor, SetVariablesProcessor, StopProcessor, ThrowExceptionProcessor,
    ValidateProcessor,
};
use camel_core_processor::try_catch::{CatchProcessor, FinallyProcessor, TryProcessor};
use camel_support::endpoint_helper::match_endpoint;
use camel_util::value::Value;
use std::sync::Arc;
use std::time::Duration;

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
            let expression = resolve_placeholders(context, expression)?;
            let e = context
                .resolve_language(language)?
                .create_expression(&expression)?;
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
        ExpressionDefinition::Configured {
            definition,
            options,
        } => configured_expression(context, definition, options),
    }
}

/// The expression with its options (Java `SimpleLanguage.createExpression(expression,
/// resultType, pretty, trimResult, nested)`).
fn configured_expression(
    context: &Arc<dyn CamelContext>,
    definition: &ExpressionDefinition,
    options: &camel_core_model::expression::ExpressionOptions,
) -> CamelResult<SharedExpression> {
    use camel_support::builder::named_expression;
    let result_type = options.result_type.clone();
    if matches!(
        result_type.as_deref(),
        Some("boolean" | "Boolean" | "java.lang.Boolean")
    ) {
        // a boolean result: the expression is a predicate
        let p = reify_predicate(context, definition)?;
        return Ok(named_expression(definition.to_string(), move |e| {
            Ok(Value::Bool(p.matches(e)?))
        }));
    }
    let mut exp = reify_expression(context, definition)?;
    let name = definition.to_string();
    if options.nested {
        let language = context.resolve_language(definition.language_name().unwrap_or("simple"))?;
        let inner = exp;
        exp = named_expression(name.clone(), move |e| {
            let v = inner.evaluate(e)?;
            if v.is_null() {
                return Ok(v);
            }
            language.create_expression(&v.to_string())?.evaluate(e)
        });
    }
    if let Some(t) = result_type {
        let inner = exp;
        exp = named_expression(name.clone(), move |e| {
            camel_support::type_conversion::convert_to_type(&inner.evaluate(e)?, &t)
        });
    }
    if options.trim {
        let inner = exp;
        exp = named_expression(name.clone(), move |e| {
            let v = inner.evaluate(e)?;
            Ok(match v {
                Value::Text(t) => Value::Text(t.trim().to_string()),
                other => other,
            })
        });
    }
    if options.pretty {
        let inner = exp;
        exp = named_expression(format!("pretty({name})"), move |e| {
            let v = inner.evaluate(e)?;
            if v.is_null() {
                return Ok(v);
            }
            let body = v.to_string();
            let json = (body.starts_with('{') && body.ends_with('}'))
                || (body.starts_with('[') && body.ends_with(']'));
            Ok(Value::Text(if json {
                camel_util::pretty::pretty_json(&body).unwrap_or(body)
            } else if body.starts_with('<') && body.ends_with('>') {
                camel_util::pretty::pretty_xml(&body, 2, body.starts_with("<?xml")).unwrap_or(body)
            } else {
                body
            }))
        });
    }
    Ok(exp)
}

/// The text with its property placeholders resolved (Java `AbstractReifier.parseString`); a
/// single optional placeholder of a missing property is an empty text.
///
/// # Errors
///
/// A missing property.
pub fn resolve_placeholders(context: &Arc<dyn CamelContext>, text: &str) -> CamelResult<String> {
    Ok(context
        .resolve_property_placeholders(text)?
        .unwrap_or_default())
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
            let expression = resolve_placeholders(context, expression)?;
            let p = context
                .resolve_language(language)?
                .create_predicate(&expression)?;
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
        ExpressionDefinition::Configured { .. } => {
            let e = reify_expression(context, def)?;
            Ok(Arc::new(ExpressionPredicate(e)))
        }
    }
}

/// The context of reifying one route.
pub struct RouteReifier<'a> {
    context: &'a Arc<dyn CamelContext>,
    route_id: &'a str,
    error_handler: Option<Arc<ErrorHandlerConfig>>,
    intercepts: Interception,
}

/// The interceptors applied to the steps of a route (`intercept` and
/// `interceptSendToEndpoint`).
#[derive(Clone, Default)]
struct Interception {
    route_uri: String,
    intercepts: Arc<Vec<Interceptor>>,
    send: Arc<Vec<(String, SendInterceptor)>>,
}

impl<'a> RouteReifier<'a> {
    /// A reifier for the route with the id (without error handler).
    pub fn new(context: &'a Arc<dyn CamelContext>, route_id: &'a str) -> Self {
        RouteReifier {
            context,
            route_id,
            error_handler: None,
            intercepts: Interception::default(),
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
            intercepts: Interception::default(),
        }
    }

    /// The same reifier without error handler (for the steps of `doTry` and `onException`,
    /// as Java `shouldWrapInErrorHandler`).
    fn without_error_handler(&self) -> RouteReifier<'a> {
        RouteReifier {
            context: self.context,
            route_id: self.route_id,
            error_handler: None,
            intercepts: self.intercepts.clone(),
        }
    }

    /// Creates the processor of the step, wrapped in the error handler if any.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint, expression or bean of the step cannot be resolved.
    pub fn step(&self, def: &ProcessorDefinition) -> CamelResult<SharedProcessor> {
        let mut p = self.create(def)?;
        if self.context.is_message_history() {
            p = self.with_message_history(def, p);
        }
        if !self.intercepts.intercepts.is_empty() {
            p = Arc::new(InterceptProcessor::new(
                self.intercepts.intercepts.clone(),
                self.route_id,
                &self.intercepts.route_uri,
                def.id.as_deref(),
                p,
            ));
        }
        Ok(match &self.error_handler {
            Some(config) => Arc::new(RedeliveryErrorHandler::new(p, config.clone())),
            None => p,
        })
    }

    /// Records the step in the message history when its node matches the node pattern of the
    /// `MessageHistoryFactory` (all nodes by default).
    fn with_message_history(
        &self,
        def: &ProcessorDefinition,
        p: SharedProcessor,
    ) -> SharedProcessor {
        use camel_api::message_history::MessageHistoryFactory;
        use camel_core_processor::message_history::{HistoryNode, MessageHistoryProcessor};
        let factory =
            camel_api::context::context_plugin::<MessageHistoryFactory>(self.context.as_ref());
        let short_name = def.eip.short_name().to_string();
        let node_id = match (&def.id, &def.eip) {
            (Some(id), _) => id.clone(),
            (None, Eip::Step { id, .. }) => id.clone(),
            (None, _) => match &factory {
                Some(f) => f.next_node_id(&short_name),
                None => short_name.clone(),
            },
        };
        let pattern = factory.as_ref().and_then(|f| f.node_pattern());
        if let Some(pattern) = pattern {
            let matches = pattern.split(',').any(|pat| {
                camel_support::pattern_helper::match_pattern(&short_name, pat.trim())
                    || camel_support::pattern_helper::match_pattern(&node_id, pat.trim())
            });
            if !matches {
                return p;
            }
        }
        // the Java label: the short name with the text between the brackets of the node
        let text = def.to_string();
        let label = match (text.find('['), text.rfind(']')) {
            (Some(start), Some(end)) if end > start => {
                format!("{short_name}[{}]", &text[start + 1..end])
            }
            _ => short_name.clone(),
        };
        Arc::new(MessageHistoryProcessor::new(
            HistoryNode {
                route_id: self.route_id.to_string(),
                node_id,
                short_name,
                label,
                size_enabled: factory.as_ref().is_some_and(|f| f.is_size_enabled()),
                copy_message: factory.is_some_and(|f| f.is_copy_message()),
            },
            p,
        ))
    }

    /// Creates the processor of the steps: a pipeline (disabled steps are left out).
    ///
    /// # Errors
    ///
    /// Returns the error of the first step that cannot be created.
    pub fn steps(&self, steps: &[ProcessorDefinition]) -> CamelResult<SharedProcessor> {
        let mut processors = self.step_list(steps)?;
        Ok(if processors.len() == 1 {
            processors.pop().unwrap()
        } else {
            Arc::new(Pipeline::new(processors))
        })
    }

    /// The processor of a `threads` step running the following steps on its pool.
    fn threads(
        &self,
        t: &camel_core_model::processor::ThreadsDefinition,
        rest: &[ProcessorDefinition],
    ) -> CamelResult<SharedProcessor> {
        let rest = self.steps(rest)?;
        let mut p: SharedProcessor = Arc::new(ThreadsProcessor::new(threads_config(t), rest)?);
        if let Some(config) = &self.error_handler {
            p = Arc::new(RedeliveryErrorHandler::new(p, config.clone()));
        }
        Ok(p)
    }

    /// The processors of the steps; a `threads` step takes the following steps (they run on
    /// its pool).
    fn step_list(&self, steps: &[ProcessorDefinition]) -> CamelResult<Vec<SharedProcessor>> {
        let mut processors = Vec::new();
        for (i, s) in steps.iter().enumerate().filter(|(_, s)| !s.disabled) {
            if let Eip::Threads(t) = &s.eip {
                processors.push(self.threads(t, &steps[i + 1..])?);
                break;
            }
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
                let endpoint = self.context.endpoint(uri)?;
                let send: SharedProcessor =
                    Arc::new(SendProcessor::new(endpoint.clone(), *pattern));
                let mut matching = Vec::new();
                for (p, i) in self.intercepts.send.iter() {
                    if match_endpoint(self.context.as_ref(), endpoint.endpoint_uri(), p)? {
                        matching.push(i.clone());
                    }
                }
                if matching.is_empty() {
                    send
                } else {
                    Arc::new(InterceptSendToEndpointProcessor::new(
                        endpoint.endpoint_uri(),
                        matching,
                        send,
                    ))
                }
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
            Eip::Split {
                expression,
                steps,
                strategy,
                options,
                delimiter,
            } => {
                let strategy = resolve_aggregation(self.context, strategy)?;
                let mut p = SplitProcessor::new(
                    self.expr(expression)?,
                    Arc::new(Pipeline::new(self.step_list(steps)?)),
                    strategy,
                    multicast_config(options),
                );
                if let Some(d) = delimiter {
                    p = p.with_delimiter(d);
                }
                Arc::new(p)
            }
            Eip::Multicast {
                steps,
                strategy,
                options,
            } => {
                let strategy = resolve_aggregation(self.context, strategy)?;
                Arc::new(MulticastProcessor::new(
                    self.step_list(steps)?,
                    strategy,
                    multicast_config(options),
                ))
            }
            Eip::Delay(e) => Arc::new(DelayProcessor::new(self.expr(e)?)),
            Eip::OnCompletion(_) => {
                return Err(CamelError::illegal_argument(
                    "onCompletion must be defined directly in a route or in the route builder",
                ))
            }
            Eip::Aggregate(a) => {
                let strategy =
                    resolve_aggregation(self.context, &a.strategy)?.ok_or_else(|| {
                        CamelError::illegal_argument(format!(
                            "AggregationStrategy must be set on {}",
                            def
                        ))
                    })?;
                let opt_expr =
                    |e: &Option<ExpressionDefinition>| e.as_ref().map(|e| self.expr(e)).transpose();
                let defaults = AggregateOptions::default();
                let options = AggregateOptions {
                    completion_size: a.completion_size.unwrap_or(defaults.completion_size),
                    completion_size_expression: opt_expr(&a.completion_size_expression)?,
                    completion_timeout: a.completion_timeout.unwrap_or(defaults.completion_timeout),
                    completion_timeout_expression: opt_expr(&a.completion_timeout_expression)?,
                    completion_timeout_checker_interval: a
                        .completion_timeout_checker_interval
                        .unwrap_or(defaults.completion_timeout_checker_interval),
                    completion_interval: a
                        .completion_interval
                        .unwrap_or(defaults.completion_interval),
                    completion_predicate: a
                        .completion_predicate
                        .as_ref()
                        .map(|p| reify_predicate(self.context, p))
                        .transpose()?,
                    eager_check_completion: a.eager_check_completion,
                    ignore_invalid_correlation_keys: a.ignore_invalid_correlation_keys,
                    close_correlation_key_on_completion: a.close_correlation_key_on_completion,
                    discard_on_completion_timeout: a.discard_on_completion_timeout,
                    discard_on_aggregation_failure: a.discard_on_aggregation_failure,
                    completion_on_new_correlation_group: a.completion_on_new_correlation_group,
                    force_completion_on_stop: a.force_completion_on_stop,
                    complete_all_on_stop: a.complete_all_on_stop,
                    repository: a.repository.as_ref().map(|r| r.0.clone()),
                };
                let outputs: SharedProcessor = Arc::new(Pipeline::new(self.step_list(&a.steps)?));
                Arc::new(AggregateProcessor::new(
                    self.expr(&a.correlation)?,
                    strategy,
                    outputs,
                    options,
                )?)
            }
            Eip::ToD {
                uri,
                pattern,
                ignore_invalid_endpoint,
            } => {
                let e = self.expr(&ExpressionDefinition::language("simple", uri))?;
                Arc::new(SendDynamicProcessor::new(
                    e,
                    *pattern,
                    *ignore_invalid_endpoint,
                ))
            }
            Eip::RecipientList {
                expression,
                delimiter,
                strategy,
                options,
                ignore_invalid_endpoints,
            } => {
                let strategy = resolve_aggregation(self.context, strategy)?;
                let mut p = RecipientListProcessor::new(
                    self.expr(expression)?,
                    strategy,
                    multicast_config(options),
                    *ignore_invalid_endpoints,
                );
                if let Some(d) = delimiter {
                    p = p.with_delimiter(d);
                }
                Arc::new(p)
            }
            Eip::WireTap {
                uri,
                ignore_invalid_endpoint,
            } => Arc::new(WireTapProcessor::new(
                self.expr(&ExpressionDefinition::language("simple", uri))?,
                *ignore_invalid_endpoint,
            )),
            Eip::SetExchangePattern(p) => Arc::new(SetExchangePatternProcessor(*p)),
            Eip::ConvertBodyTo {
                type_name,
                mandatory,
            } => Arc::new(ConvertBodyProcessor::new(type_name, *mandatory)?),
            Eip::SetHeaders(headers) => {
                let mut list = Vec::new();
                for (n, e) in headers {
                    list.push((n.clone(), self.expr(e)?));
                }
                Arc::new(SetHeadersProcessor::new(list))
            }
            Eip::SetVariables(variables) => {
                let mut list = Vec::new();
                for (n, e) in variables {
                    list.push((n.clone(), self.expr(e)?));
                }
                Arc::new(SetVariablesProcessor::new(list))
            }
            Eip::RemoveProperty(name) => Arc::new(RemovePropertyProcessor::new(name)),
            Eip::RemoveProperties {
                pattern,
                exclude_patterns,
            } => {
                let excludes: Vec<&str> = exclude_patterns.iter().map(String::as_str).collect();
                Arc::new(RemovePropertiesProcessor::new(pattern, &excludes))
            }
            Eip::RemoveVariable(name) => Arc::new(RemoveVariableProcessor::new(name)),
            Eip::Script(e) => Arc::new(ScriptProcessor::new(self.expr(e)?)),
            Eip::Loop {
                count,
                while_predicate,
                copy,
                steps,
            } => {
                let body = self.steps(steps)?;
                let p = match (count, while_predicate) {
                    (_, Some(w)) => {
                        LoopProcessor::do_while(reify_predicate(self.context, w)?, body)
                    }
                    (Some(c), None) => LoopProcessor::count(self.expr(c)?, body),
                    (None, None) => {
                        return Err(CamelError::illegal_argument(
                            "A loop needs a count or a while predicate",
                        ))
                    }
                };
                Arc::new(p.with_copy(*copy))
            }
            Eip::RoutingSlip {
                expression,
                delimiter,
                ignore_invalid_endpoints,
            } => Arc::new(
                RoutingSlipProcessor::new(self.expr(expression)?, delimiter.as_deref())
                    .with_ignore_invalid_endpoints(*ignore_invalid_endpoints),
            ),
            Eip::DynamicRouter {
                expression,
                delimiter,
                ignore_invalid_endpoints,
            } => Arc::new(
                RoutingSlipProcessor::dynamic_router(self.expr(expression)?, delimiter.as_deref())
                    .with_ignore_invalid_endpoints(*ignore_invalid_endpoints),
            ),
            Eip::Sort {
                expression,
                comparator,
            } => {
                let e = match expression {
                    Some(e) => self.expr(e)?,
                    None => camel_support::builder::body_expression(),
                };
                Arc::new(SortProcessor::new(
                    e,
                    comparator.as_ref().map(|c| c.0.clone()),
                ))
            }
            Eip::Step { id, steps } => Arc::new(StepProcessor::new(id, self.step_list(steps)?)),
            Eip::Enrich {
                uri,
                strategy,
                aggregate_on_exception,
                ignore_invalid_endpoint,
            } => {
                let strategy = resolve_aggregation(self.context, strategy)?;
                Arc::new(EnrichProcessor::new(
                    self.expr(uri)?,
                    strategy,
                    *aggregate_on_exception,
                    *ignore_invalid_endpoint,
                ))
            }
            Eip::PollEnrich {
                uri,
                timeout,
                strategy,
                aggregate_on_exception,
                ignore_invalid_endpoint,
            } => {
                let strategy = resolve_aggregation(self.context, strategy)?;
                Arc::new(PollEnrichProcessor::new(
                    self.expr(uri)?,
                    *timeout,
                    strategy,
                    *aggregate_on_exception,
                    *ignore_invalid_endpoint,
                ))
            }
            Eip::Sample {
                message_frequency,
                period_millis,
            } => Arc::new(match message_frequency {
                Some(n) => SamplingProcessor::per_messages(*n)?,
                None => {
                    SamplingProcessor::per_period(std::time::Duration::from_millis(*period_millis))?
                }
            }),
            Eip::Throttle {
                max_requests,
                time_period_millis,
                concurrent,
                correlation,
                reject_execution,
            } => {
                if !*concurrent && *time_period_millis == 0 {
                    return Err(CamelError::illegal_argument(
                        "TimePeriodMillis should be a positive number, was: 0",
                    ));
                }
                let mode = if *concurrent {
                    ThrottlingMode::ConcurrentRequests
                } else {
                    ThrottlingMode::TotalRequests(*time_period_millis)
                };
                let correlation = match correlation {
                    Some(c) => Some(self.expr(c)?),
                    None => None,
                };
                Arc::new(ThrottleProcessor::new(
                    self.expr(max_requests)?,
                    mode,
                    correlation,
                    *reject_execution,
                ))
            }
            Eip::ClaimCheck {
                operation,
                key,
                filter,
                strategy,
            } => {
                let key = match key {
                    Some(k) => Some(self.expr(&ExpressionDefinition::language("simple", k))?),
                    None => None,
                };
                Arc::new(ClaimCheckProcessor::new(
                    ClaimCheckOperation::of(operation)?,
                    key,
                    filter.clone(),
                    resolve_aggregation(self.context, strategy)?,
                )?)
            }
            Eip::Rollback {
                message,
                mark_rollback_only,
                mark_rollback_only_last,
            } => Arc::new(RollbackProcessor::new(
                message.as_deref(),
                *mark_rollback_only,
                *mark_rollback_only_last,
            )),
            Eip::ConvertValueTo {
                variable,
                name,
                to_name,
                type_name,
                mandatory,
            } => Arc::new(ConvertValueProcessor::new(
                if *variable {
                    ConvertTarget::Variable
                } else {
                    ConvertTarget::Header
                },
                name,
                to_name.as_deref(),
                type_name,
                *mandatory,
            )?),
            Eip::Resequence(r) => {
                let timeout = std::time::Duration::from_millis(r.timeout);
                let config = match &r.mode {
                    ResequenceMode::Batch {
                        size,
                        allow_duplicates,
                        reverse,
                    } => ResequenceConfig::Batch {
                        size: *size,
                        timeout,
                        allow_duplicates: *allow_duplicates,
                        reverse: *reverse,
                        ignore_invalid: r.ignore_invalid_exchanges,
                    },
                    ResequenceMode::Stream {
                        capacity,
                        reject_old,
                    } => ResequenceConfig::Stream {
                        capacity: *capacity,
                        timeout,
                        reject_old: *reject_old,
                        ignore_invalid: r.ignore_invalid_exchanges,
                    },
                };
                Arc::new(ResequenceProcessor::new(
                    self.expr(&r.expression)?,
                    config,
                    self.steps(&r.steps)?,
                ))
            }
            Eip::Validate(p) => Arc::new(ValidateProcessor::new(reify_predicate(self.context, p)?)),
            Eip::IdempotentConsumer(i) => {
                let repository = match &i.repository {
                    IdempotentRepositoryRef::Repository(r) => r.clone(),
                    IdempotentRepositoryRef::Ref(name) => self
                        .context
                        .registry()
                        .and_then(|r| r.lookup_by_name(name))
                        .and_then(|v| camel_support::idempotent::repository_of(&v))
                        .ok_or_else(|| CamelError::no_such_bean(name))?,
                };
                let options = IdempotentOptions {
                    eager: i.eager,
                    completion_eager: i.completion_eager,
                    skip_duplicate: i.skip_duplicate,
                    remove_on_failure: i.remove_on_failure,
                };
                Arc::new(IdempotentConsumerProcessor::new(
                    self.expr(&i.message_id)?,
                    repository,
                    options,
                    self.steps(&i.steps)?,
                ))
            }
            Eip::Threads(t) => Arc::new(ThreadsProcessor::new(
                threads_config(t),
                Arc::new(Pipeline::new(Vec::new())),
            )?),
            Eip::LoadBalance { balancer, steps } => {
                let balance = match balancer {
                    LoadBalancerDefinition::RoundRobin => LoadBalance::RoundRobin,
                    LoadBalancerDefinition::Random => LoadBalance::Random,
                    LoadBalancerDefinition::Sticky(e) => LoadBalance::Sticky(self.expr(e)?),
                    LoadBalancerDefinition::Topic => LoadBalance::Topic,
                    LoadBalancerDefinition::Failover {
                        exceptions,
                        round_robin,
                        sticky,
                        maximum_failover_attempts,
                    } => LoadBalance::Failover(FailoverOptions {
                        exceptions: exceptions.clone(),
                        round_robin: *round_robin,
                        sticky: *sticky,
                        maximum_failover_attempts: *maximum_failover_attempts,
                    }),
                    LoadBalancerDefinition::Weighted {
                        round_robin,
                        distribution_ratio,
                        delimiter,
                    } => {
                        let mut ratios = Vec::new();
                        for r in distribution_ratio.split(delimiter.as_str()) {
                            let r = r.trim();
                            let v: i64 = r.parse().map_err(|_| {
                                CamelError::illegal_argument(format!(
                                    "Distribution ratio must be a number, was: {r}"
                                ))
                            })?;
                            if v < 0 {
                                return Err(CamelError::illegal_argument(format!(
                                    "Distribution ratio must be zero or a positive number, was: {v}"
                                )));
                            }
                            ratios.push(v as u32);
                        }
                        LoadBalance::Weighted {
                            round_robin: *round_robin,
                            ratios,
                        }
                    }
                };
                // each step is a processor of the balancer
                let mut processors = Vec::new();
                for s in steps.iter().filter(|s| !s.disabled) {
                    processors.push(self.step(s)?);
                }
                Arc::new(LoadBalancerProcessor::new(balance, processors)?)
            }
            Eip::Bean { bean, method } => {
                let factory = context_plugin::<Arc<dyn BeanProcessorFactory>>(
                    self.context.as_ref(),
                )
                .ok_or_else(|| {
                    CamelError::illegal_state(
                        "Cannot create the bean processor: the context has no BeanProcessorFactory plugin (add the camel-bean crate, as camel_core::default_context() does)",
                    )
                })?;
                factory.create_bean_processor(self.context, bean, method.as_deref())?
            }
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

/// The pool configuration of a `threads` definition (the defaults of the default thread pool
/// profile for the unset options).
fn threads_config(t: &camel_core_model::processor::ThreadsDefinition) -> ThreadsConfig {
    let d = ThreadsConfig::default();
    let pool_size = t.pool_size.unwrap_or(d.pool_size);
    ThreadsConfig {
        pool_size,
        // a pool size above the default max pool size raises it (Java ThreadsReifier)
        max_pool_size: t.max_pool_size.unwrap_or(if pool_size > d.max_pool_size {
            pool_size
        } else {
            d.max_pool_size
        }),
        max_queue_size: t.max_queue_size.unwrap_or(d.max_queue_size),
        thread_name: t.thread_name.clone().unwrap_or(d.thread_name),
        rejected_policy: t.rejected_policy.unwrap_or(d.rejected_policy),
        caller_runs_when_rejected: t
            .caller_runs_when_rejected
            .unwrap_or(d.caller_runs_when_rejected),
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
    let mut reifier = RouteReifier::with_error_handler(context, route_id, error_handler);
    let failed = |e: CamelError| {
        let mut route = def.clone();
        route.id = Some(route_id.to_string());
        CamelError::failed_to_create_route(
            route_id,
            def.source_location.as_deref(),
            &route.to_string(),
            e,
        )
    };
    let mut processors = Vec::new();
    // the interceptors of the route builder: their steps are not intercepted
    let mut intercepts = Vec::new();
    for i in &def.builder_intercepts {
        intercepts.push(interceptor(&reifier, &i.on_when, &i.steps).map_err(failed)?);
    }
    let mut send = Vec::new();
    for i in &def.builder_intercept_send_to_endpoints {
        let after = match &i.after_uri {
            Some(u) => Some(
                reifier
                    .create(&ProcessorDefinition::new(Eip::To {
                        uri: u.clone(),
                        pattern: None,
                    }))
                    .map_err(failed)?,
            ),
            None => None,
        };
        send.push((
            i.uri.clone(),
            SendInterceptor {
                interceptor: interceptor(&reifier, &i.on_when, &i.steps).map_err(failed)?,
                skip: i.skip_send_to_original_endpoint,
                after,
            },
        ));
    }
    for i in &def.builder_intercept_froms {
        let applies = match &i.uri {
            Some(pattern) => {
                match_endpoint(context.as_ref(), &def.from, pattern).map_err(failed)?
            }
            None => true,
        };
        if applies {
            let it = interceptor(&reifier, &i.on_when, &i.steps).map_err(failed)?;
            processors.push(
                Arc::new(InterceptFromProcessor::new(it, route_id, &def.from)) as SharedProcessor,
            );
        }
    }
    reifier.intercepts = Interception {
        route_uri: def.from.clone(),
        intercepts: Arc::new(intercepts),
        send: Arc::new(send),
    };
    // the onCompletions first (Java RouteDefinitionHelper.initOnCompletions): the ones of the
    // route, else the ones of the route builder
    let scoped: Vec<&OnCompletionDefinition> = def
        .steps
        .iter()
        .filter(|s| !s.disabled)
        .filter_map(|s| match &s.eip {
            Eip::OnCompletion(c) => Some(c.as_ref()),
            _ => None,
        })
        .collect();
    let (completions, route_scoped) = if scoped.is_empty() {
        (def.builder_on_completions.iter().collect(), false)
    } else {
        (scoped, true)
    };
    for c in completions {
        let p = reify_on_completion(&reifier, route_id, c, route_scoped).map_err(|e| {
            let mut route = def.clone();
            route.id = Some(route_id.to_string());
            CamelError::failed_to_create_route(
                route_id,
                def.source_location.as_deref(),
                &route.to_string(),
                e,
            )
        })?;
        processors.push(p);
    }
    let steps: Vec<ProcessorDefinition> = def
        .steps
        .iter()
        .filter(|s| !s.disabled && !matches!(s.eip, Eip::OnCompletion(_)))
        .cloned()
        .collect();
    for (i, step) in steps.iter().enumerate() {
        let p = match &step.eip {
            // the following steps run on the pool of the threads step
            Eip::Threads(t) => reifier.threads(t, &steps[i + 1..]),
            _ => reifier.step(step),
        };
        let threads = matches!(step.eip, Eip::Threads(_));
        let p = p.map_err(|e| {
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
        if threads {
            break;
        }
    }
    Ok(Arc::new(Pipeline::new(processors)))
}

/// An interceptor of the steps (their steps reified without interception).
fn interceptor(
    reifier: &RouteReifier<'_>,
    on_when: &Option<ExpressionDefinition>,
    steps: &[ProcessorDefinition],
) -> CamelResult<Interceptor> {
    let on_when = match on_when {
        Some(p) => Some(reify_predicate(reifier.context, p)?),
        None => None,
    };
    Ok(Interceptor {
        on_when,
        processor: reifier.steps(steps)?,
    })
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
        log_exhausted_message_history: d.log_exhausted_message_history,
        log_exhausted_message_body: d.log_exhausted_message_body,
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
    config.use_original_message = eh.use_original_message;
    config.use_original_body = eh.use_original_body;
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
            use_original_message: o.use_original_message,
            use_original_body: o.use_original_body,
        }));
    }
    Ok(Some(Arc::new(config)))
}

/// The aggregation strategy of a reference: the strategy itself, or a bean from the registry.
///
/// # Errors
///
/// Returns a `NoSuchBeanException` if there is no such strategy bean.
pub fn resolve_aggregation(
    context: &Arc<dyn CamelContext>,
    r: &Option<AggregationRef>,
) -> CamelResult<Option<SharedAggregationStrategy>> {
    match r {
        None => Ok(None),
        Some(AggregationRef::Strategy(s)) => Ok(Some(s.clone())),
        Some(AggregationRef::Ref(name)) => {
            let not_found = || {
                CamelError::no_such_bean_of_type(Some(name), "org.apache.camel.AggregationStrategy")
            };
            let registry = context.registry().ok_or_else(not_found)?;
            lookup_bean::<SharedAggregationStrategy>(registry.as_ref(), name)
                .map(|s| Some((*s).clone()))
                .ok_or_else(not_found)
        }
    }
}

/// The processing options of a multicast, split or recipient list.
fn multicast_config(o: &MulticastOptions) -> MulticastConfig {
    MulticastConfig {
        stop_on_exception: o.stop_on_exception,
        parallel_processing: o.parallel_processing,
        streaming: o.streaming,
        timeout: o.timeout.filter(|t| *t > 0).map(Duration::from_millis),
    }
}

/// The processor of an onCompletion (Java `OnCompletionReifier`).
fn reify_on_completion(
    reifier: &RouteReifier<'_>,
    route_id: &str,
    c: &OnCompletionDefinition,
    route_scoped: bool,
) -> CamelResult<SharedProcessor> {
    if c.on_complete_only && c.on_failure_only {
        return Err(CamelError::illegal_argument(format!(
            "Both onCompleteOnly and onFailureOnly cannot be true. Only one of them can be true. On node: onCompletion[{}]",
            Steps(&c.steps)
        )));
    }
    let on_when = c
        .on_when
        .as_ref()
        .map(|p| reify_predicate(reifier.context, p))
        .transpose()?;
    let options = OnCompletionOptions {
        on_complete_only: c.on_complete_only,
        on_failure_only: c.on_failure_only,
        on_when,
        use_original_message: c.use_original_message,
        before_consumer: c.before_consumer,
        route_scoped,
        parallel_processing: c.parallel_processing,
    };
    Ok(Arc::new(OnCompletionProcessor::new(
        route_id,
        reifier.steps(&c.steps)?,
        options,
    )))
}
