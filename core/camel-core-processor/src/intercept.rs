//! The interceptors (Java `InterceptReifier`'s intercept strategy, `InterceptFromReifier`'s
//! filter and `InterceptSendToEndpointProcessor`): steps run before each step of a route,
//! when a route consumes, or before sending to an endpoint.

use crate::{continue_processing, prepare_out_to_in, process_and_capture};
use camel_api::error::CamelResult;
use camel_api::exchange::Exchange;
use camel_api::exchange_constants::{
    INTERCEPTED_ENDPOINT, INTERCEPTED_NODE_ID, INTERCEPTED_ROUTE_ENDPOINT_URI,
    INTERCEPTED_ROUTE_ID, INTERCEPT_SEND_TO_ENDPOINT_WHEN_MATCHED,
};
use camel_api::processor::{
    start_processors, stop_processors, BoxFuture, Processor, SharedPredicate, SharedProcessor,
};
use std::sync::Arc;

/// An interceptor: the steps, run when the predicate (if any) matches.
#[derive(Clone)]
pub struct Interceptor {
    /// The predicate of the exchanges to intercept.
    pub on_when: Option<SharedPredicate>,
    /// The steps of the interception.
    pub processor: SharedProcessor,
}

impl Interceptor {
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        match &self.on_when {
            Some(p) => p.matches(exchange),
            None => Ok(true),
        }
    }
}

/// Runs the `intercept()`s before the step (Java: the intercept strategy wrapping each
/// processor of a route in a pipeline of the interception and the processor); a `stop()` or
/// an error in an interception skips the step.
pub struct InterceptProcessor {
    interceptors: Arc<Vec<Interceptor>>,
    route_id: String,
    route_uri: String,
    node_id: Option<String>,
    target: SharedProcessor,
}

impl InterceptProcessor {
    /// Wraps the processor of the step with the node id of the route consuming from the uri.
    pub fn new(
        interceptors: Arc<Vec<Interceptor>>,
        route_id: &str,
        route_uri: &str,
        node_id: Option<&str>,
        target: SharedProcessor,
    ) -> Self {
        InterceptProcessor {
            interceptors,
            route_id: route_id.to_string(),
            route_uri: route_uri.to_string(),
            node_id: node_id.map(str::to_string),
            target,
        }
    }
}

impl Processor for InterceptProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let mut all: Vec<SharedProcessor> = self
                .interceptors
                .iter()
                .map(|i| i.processor.clone())
                .collect();
            all.push(self.target.clone());
            start_processors(&all).await
        })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(std::slice::from_ref(&self.target)).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            for i in self.interceptors.iter() {
                exchange.set_property(INTERCEPTED_ROUTE_ID, self.route_id.as_str());
                if let Some(id) = &self.node_id {
                    exchange.set_property(INTERCEPTED_NODE_ID, id.as_str());
                }
                exchange.set_property(INTERCEPTED_ROUTE_ENDPOINT_URI, self.route_uri.as_str());
                if i.matches(exchange)? {
                    process_and_capture(i.processor.as_ref(), exchange).await;
                }
                if exchange.is_route_stop()
                    || !continue_processing(exchange, "so breaking out of intercept")
                {
                    return Ok(());
                }
                prepare_out_to_in(exchange);
            }
            self.target.process(exchange).await
        })
    }
}

/// Runs an `interceptFrom` when the route receives an exchange (Java: a filter of the
/// predicate at the start of the route, setting the `CamelInterceptedEndpoint` property).
pub struct InterceptFromProcessor {
    interceptor: Interceptor,
    route_id: String,
    route_uri: String,
}

impl InterceptFromProcessor {
    /// Creates the processor of the route consuming from the uri.
    pub fn new(interceptor: Interceptor, route_id: &str, route_uri: &str) -> Self {
        InterceptFromProcessor {
            interceptor,
            route_id: route_id.to_string(),
            route_uri: route_uri.to_string(),
        }
    }
}

impl Processor for InterceptFromProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.interceptor.processor.start_processor()
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        self.interceptor.processor.stop_processor()
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            if self.interceptor.matches(exchange)? {
                exchange.set_property(INTERCEPTED_ROUTE_ID, self.route_id.as_str());
                exchange.set_property(INTERCEPTED_ROUTE_ENDPOINT_URI, self.route_uri.as_str());
                if let Some(from) = exchange.from_endpoint().map(str::to_string) {
                    exchange.set_property(INTERCEPTED_ENDPOINT, from);
                }
                process_and_capture(self.interceptor.processor.as_ref(), exchange).await;
            }
            Ok(())
        })
    }
}

/// An `interceptSendToEndpoint`: the steps run before sending, whether the sending is
/// skipped, and the processor sending to the `afterUri`.
#[derive(Clone)]
pub struct SendInterceptor {
    /// The steps and their predicate.
    pub interceptor: Interceptor,
    /// Whether the sending to the original endpoint is skipped.
    pub skip: bool,
    /// The processor sending to the `afterUri`.
    pub after: Option<SharedProcessor>,
}

/// Runs the `interceptSendToEndpoint`s of the endpoint around the sending (Java
/// `InterceptSendToEndpointProcessor`): the steps first (when their predicate matches), then
/// the sending unless skipped, then the `afterUri`.
pub struct InterceptSendToEndpointProcessor {
    uri: String,
    interceptors: Vec<SendInterceptor>,
    target: SharedProcessor,
}

impl InterceptSendToEndpointProcessor {
    /// Wraps the processor sending to the uri.
    pub fn new(uri: &str, interceptors: Vec<SendInterceptor>, target: SharedProcessor) -> Self {
        InterceptSendToEndpointProcessor {
            uri: uri.to_string(),
            interceptors,
            target,
        }
    }
}

impl Processor for InterceptSendToEndpointProcessor {
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move {
            let mut all: Vec<SharedProcessor> = Vec::new();
            for i in &self.interceptors {
                all.push(i.interceptor.processor.clone());
                if let Some(a) = &i.after {
                    all.push(a.clone());
                }
            }
            all.push(self.target.clone());
            start_processors(&all).await
        })
    }

    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(async move { stop_processors(std::slice::from_ref(&self.target)).await })
    }

    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        Box::pin(async move {
            exchange.set_property(INTERCEPTED_ENDPOINT, self.uri.as_str());
            let mut afters: Vec<SharedProcessor> = Vec::new();
            for i in &self.interceptors {
                let matched = i.interceptor.matches(exchange)?;
                exchange.set_property(INTERCEPT_SEND_TO_ENDPOINT_WHEN_MATCHED, matched);
                if matched {
                    process_and_capture(i.interceptor.processor.as_ref(), exchange).await;
                }
                if exchange.is_route_stop()
                    || !continue_processing(
                        exchange,
                        &format!(
                            "skip sending to original intended destination: {}",
                            self.uri
                        ),
                    )
                {
                    exchange.remove_property(INTERCEPT_SEND_TO_ENDPOINT_WHEN_MATCHED);
                    return Ok(());
                }
                if i.skip && matched {
                    exchange.remove_property(INTERCEPT_SEND_TO_ENDPOINT_WHEN_MATCHED);
                    return Ok(());
                }
                prepare_out_to_in(exchange);
                if matched {
                    if let Some(a) = &i.after {
                        afters.push(a.clone());
                    }
                }
            }
            exchange.remove_property(INTERCEPT_SEND_TO_ENDPOINT_WHEN_MATCHED);
            self.target.process(exchange).await?;
            for a in afters.iter().rev() {
                if !continue_processing(exchange, "so breaking out of afterUri") {
                    break;
                }
                prepare_out_to_in(exchange);
                a.process(exchange).await?;
            }
            Ok(())
        })
    }
}
