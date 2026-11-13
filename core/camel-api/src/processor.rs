//! The processing contracts: [`Processor`], [`Expression`], [`Predicate`] and
//! [`AggregationStrategy`] (Java `org.apache.camel.Processor`, `AsyncProcessor`, `Expression`,
//! `Predicate` and `AggregationStrategy`).
//!
//! Processing is asynchronous: [`Processor::process`] returns a future, which replaces the Java
//! `AsyncProcessor` + `AsyncCallback` pair. A processor reports failure by returning an error
//! (the routing engine stores it on the exchange, as Java does with a thrown exception).
//! Expressions and predicates are synchronous, as in Java.
//!
//! Closures can be used everywhere: see [`processor_fn`], [`async_processor_fn`],
//! [`expression_fn`], [`predicate_fn`] and [`aggregation_fn`].

use crate::context::CamelContext;
use crate::error::CamelResult;
use crate::exchange::Exchange;
use camel_util::value::Value;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed, `Send` future (the return type of [`Processor::process`]).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A processor of message exchanges: the building block of routes (EIPs, producers, beans and
/// user code are all processors).
pub trait Processor: Send + Sync {
    /// Processes the exchange.
    ///
    /// # Errors
    ///
    /// Returns the error if processing failed; the routing engine sets it on the exchange and
    /// lets the error handler deal with it.
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>>;

    /// Starts the processor, and the processors it contains, before its route consumes (Java:
    /// starting the processor as a service). Must be idempotent; the default does nothing.
    ///
    /// The name differs from [`Service::start`](crate::service::Service::start) because
    /// producers are both processors and services.
    ///
    /// # Errors
    ///
    /// Returns the error if the processor cannot start.
    fn start_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// Stops the processor, and the processors it contains, after its route stopped consuming
    /// (Java: stopping the processor as a service). Must be idempotent; the default does
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns the error if the processor cannot stop.
    fn stop_processor(&self) -> BoxFuture<'_, CamelResult<()>> {
        Box::pin(std::future::ready(Ok(())))
    }
}

/// A shared processor.
pub type SharedProcessor = Arc<dyn Processor>;

/// Starts the processors in order (Java `ServiceHelper.startService(collection)`).
///
/// # Errors
///
/// Returns the first error; the processors after it are not started.
pub async fn start_processors<'a>(
    processors: impl IntoIterator<Item = &'a SharedProcessor>,
) -> CamelResult<()> {
    for p in processors {
        p.start_processor().await?;
    }
    Ok(())
}

/// Stops all the processors in order, even if some fail (Java
/// `ServiceHelper.stopService(collection)`).
///
/// # Errors
///
/// Returns the first error, after stopping all of them.
pub async fn stop_processors<'a>(
    processors: impl IntoIterator<Item = &'a SharedProcessor>,
) -> CamelResult<()> {
    let mut first = None;
    for p in processors {
        if let Err(e) = p.stop_processor().await {
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}

/// A processor from a synchronous closure.
pub struct FnProcessor<F>(F);

impl<F> Processor for FnProcessor<F>
where
    F: Fn(&mut Exchange) -> CamelResult<()> + Send + Sync,
{
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        let r = (self.0)(exchange);
        Box::pin(std::future::ready(r))
    }
}

/// Creates a processor from a synchronous closure (the common case for user code).
///
/// # Examples
///
/// ```
/// use camel_api::processor::{processor_fn, Processor};
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
///
/// let upper = processor_fn(|ex: &mut Exchange| {
///     let body = ex.message().body_as::<String>()?.unwrap_or_default();
///     ex.message_mut().set_body(body.to_uppercase());
///     Ok(())
/// });
/// let mut ex = Exchange::with_body(StandaloneContext::shared(), "hello");
/// camel_api::processor::block_on(upper.process(&mut ex)).unwrap();
/// assert_eq!("HELLO", ex.message().body().to_string());
/// ```
pub fn processor_fn<F>(f: F) -> FnProcessor<F>
where
    F: Fn(&mut Exchange) -> CamelResult<()> + Send + Sync,
{
    FnProcessor(f)
}

/// A processor from an asynchronous closure.
pub struct AsyncFnProcessor<F>(F);

impl<F> Processor for AsyncFnProcessor<F>
where
    F: for<'a> Fn(&'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> + Send + Sync,
{
    fn process<'a>(&'a self, exchange: &'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> {
        (self.0)(exchange)
    }
}

/// Creates a processor from a closure returning a boxed future (for processors that wait on
/// I/O).
///
/// # Examples
///
/// ```
/// use camel_api::processor::{async_processor_fn, block_on, Processor};
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
///
/// let p = async_processor_fn(|ex: &mut Exchange| Box::pin(async move {
///     ex.message_mut().set_body("done");
///     Ok(())
/// }));
/// let mut ex = Exchange::new(StandaloneContext::shared());
/// block_on(p.process(&mut ex)).unwrap();
/// assert_eq!("done", ex.message().body().to_string());
/// ```
pub fn async_processor_fn<F>(f: F) -> AsyncFnProcessor<F>
where
    F: for<'a> Fn(&'a mut Exchange) -> BoxFuture<'a, CamelResult<()>> + Send + Sync,
{
    AsyncFnProcessor(f)
}

/// An expression evaluated on an exchange (such as a simple, header or constant expression).
pub trait Expression: Send + Sync {
    /// Evaluates the expression.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression cannot be evaluated.
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value>;

    /// Evaluates the expression on an exchange it may change: a few functions of the simple
    /// language change the exchange while evaluating (such as `${setHeader(..)}`,
    /// `${setVariable(..)}` and `${listAdd(..)}`, which change a list of the exchange in place
    /// as Java does). Processors owning the exchange (setBody, setHeader, transform, log, ...)
    /// call this method; the default is [`evaluate`](Expression::evaluate), and expressions
    /// composed of other expressions forward it to their parts.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression cannot be evaluated.
    fn evaluate_mut(&self, exchange: &mut Exchange) -> CamelResult<Value> {
        self.evaluate(exchange)
    }

    /// Initializes the expression with the context (for example to resolve languages or
    /// endpoints) before it is used.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    fn init(&self, context: &Arc<dyn CamelContext>) -> CamelResult<()> {
        let _ = context;
        Ok(())
    }

    /// The text form of the expression (Java `toString()`), used in log and error messages: the
    /// expression text for a language expression (such as `${header.foo}`). The default is the
    /// short type name.
    fn describe(&self) -> String {
        short_type_name(std::any::type_name::<Self>())
    }
}

/// A shared expression.
pub type SharedExpression = Arc<dyn Expression>;

/// An expression from a closure.
pub struct FnExpression<F>(F);

impl<F> Expression for FnExpression<F>
where
    F: Fn(&Exchange) -> CamelResult<Value> + Send + Sync,
{
    fn evaluate(&self, exchange: &Exchange) -> CamelResult<Value> {
        (self.0)(exchange)
    }
}

/// Creates an expression from a closure.
pub fn expression_fn<F>(f: F) -> FnExpression<F>
where
    F: Fn(&Exchange) -> CamelResult<Value> + Send + Sync,
{
    FnExpression(f)
}

/// An expression with a constant value (Java `StaticExpression` / the constant language).
#[derive(Debug, Clone)]
pub struct ConstantExpression(pub Value);

impl Expression for ConstantExpression {
    fn evaluate(&self, _exchange: &Exchange) -> CamelResult<Value> {
        Ok(self.0.clone())
    }

    fn describe(&self) -> String {
        self.0.to_string()
    }
}

impl fmt::Display for ConstantExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A predicate (a boolean condition) evaluated on an exchange.
pub trait Predicate: Send + Sync {
    /// Evaluates the predicate.
    ///
    /// # Errors
    ///
    /// Returns an error if the predicate cannot be evaluated.
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool>;

    /// Initializes the predicate with the context before it is used.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    fn init(&self, context: &Arc<dyn CamelContext>) -> CamelResult<()> {
        let _ = context;
        Ok(())
    }

    /// For a binary predicate (such as `header(foo) == 123`), the text of the evaluated
    /// operands (such as `456 == 123`), shown when an assertion fails (Java
    /// `BinaryPredicate.matchesReturningFailureMessage`). Other predicates return `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if an operand cannot be evaluated.
    fn failure_text(&self, exchange: &Exchange) -> CamelResult<Option<String>> {
        let _ = exchange;
        Ok(None)
    }

    /// The text form of the predicate (Java `toString()`), used in log and error messages: the
    /// expression text for a language predicate (such as `${header.foo}`). The default is the
    /// short type name.
    fn describe(&self) -> String {
        short_type_name(std::any::type_name::<Self>())
    }
}

/// A shared predicate.
pub type SharedPredicate = Arc<dyn Predicate>;

/// The expression as a value, to bind it in the registry (for the `ref` language).
pub fn expression_value(expression: SharedExpression) -> Value {
    Value::Object(camel_util::value::ObjectRef::new(expression))
}

/// The expression held by a value of the registry (see [`expression_value`]).
pub fn expression_of(value: &Value) -> Option<SharedExpression> {
    value.downcast_ref::<SharedExpression>().cloned()
}

/// The predicate as a value, to bind it in the registry (for the `ref` language).
pub fn predicate_value(predicate: SharedPredicate) -> Value {
    Value::Object(camel_util::value::ObjectRef::new(predicate))
}

/// The predicate held by a value of the registry (see [`predicate_value`]).
pub fn predicate_of(value: &Value) -> Option<SharedPredicate> {
    value.downcast_ref::<SharedPredicate>().cloned()
}

/// A predicate from a closure.
pub struct FnPredicate<F>(F);

impl<F> Predicate for FnPredicate<F>
where
    F: Fn(&Exchange) -> CamelResult<bool> + Send + Sync,
{
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        (self.0)(exchange)
    }
}

/// Creates a predicate from a closure.
///
/// # Examples
///
/// ```
/// use camel_api::processor::{predicate_fn, Predicate};
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
/// let is_gold = predicate_fn(|ex: &Exchange| Ok(ex.message().header_as::<String>("level")?.as_deref() == Some("gold")));
/// let mut ex = Exchange::new(StandaloneContext::shared());
/// ex.message_mut().set_header("level", "gold");
/// assert!(is_gold.matches(&ex).unwrap());
/// ```
pub fn predicate_fn<F>(f: F) -> FnPredicate<F>
where
    F: Fn(&Exchange) -> CamelResult<bool> + Send + Sync,
{
    FnPredicate(f)
}

/// Every expression can be used as a predicate (Java `PredicateBuilder.toPredicate`): the
/// value is evaluated with [`camel_util::object_helper::evaluate_value_predicate`].
pub struct ExpressionPredicate(pub SharedExpression);

impl Predicate for ExpressionPredicate {
    fn matches(&self, exchange: &Exchange) -> CamelResult<bool> {
        let value = self.0.evaluate(exchange)?;
        Ok(camel_util::object_helper::evaluate_value_predicate(&value))
    }

    fn describe(&self) -> String {
        self.0.describe()
    }
}

/// The type name without the module paths (`a::b::Foo<c::Bar>` becomes `Foo<Bar>`).
///
/// # Examples
///
/// ```
/// use camel_api::processor::short_type_name;
/// assert_eq!("FnExpression<{{closure}}>", short_type_name("camel_api::processor::FnExpression<my::test::{{closure}}>"));
/// ```
pub fn short_type_name(name: &str) -> String {
    let mut out = String::new();
    let mut segment = String::new();
    for c in name.chars() {
        match c {
            '<' | '>' | ',' | ' ' | '(' | ')' | '[' | ']' | '&' => {
                out.push_str(segment.rsplit("::").next().unwrap_or(""));
                segment.clear();
                out.push(c);
            }
            _ => segment.push(c),
        }
    }
    out.push_str(segment.rsplit("::").next().unwrap_or(""));
    out
}

/// A strategy for aggregating two exchanges together into a single exchange.
///
/// On the first invocation the `old_exchange` is `None`: return the `new_exchange`. On
/// subsequent invocations merge the new exchange into the old one and return the old exchange
/// (favor returning one of the input exchanges rather than a new one).
pub trait AggregationStrategy: Send + Sync {
    /// Aggregates an old and new exchange together.
    ///
    /// # Arguments
    ///
    /// * `old_exchange` - the oldest exchange (`None` on the first aggregation)
    /// * `new_exchange` - the newest exchange (`None` if there was no data possible to acquire)
    ///
    /// # Returns
    ///
    /// The combined exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if the exchanges cannot be aggregated.
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>>;

    /// Aggregates with access to the input exchange of the EIP (only used by multicast,
    /// recipient list and splitter; by default the input is ignored).
    ///
    /// # Errors
    ///
    /// Returns an error if the exchanges cannot be aggregated.
    fn aggregate_with_input(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
        input_exchange: &Exchange,
    ) -> CamelResult<Option<Exchange>> {
        let _ = input_exchange;
        self.aggregate(old_exchange, new_exchange)
    }

    /// Whether the strategy supports pre-completion (see [`AggregationStrategy::pre_complete`]).
    fn can_pre_complete(&self) -> bool {
        false
    }

    /// Whether the aggregation should complete before the new exchange is aggregated.
    fn pre_complete(
        &self,
        old_exchange: Option<&Exchange>,
        new_exchange: Option<&Exchange>,
    ) -> bool {
        let _ = (old_exchange, new_exchange);
        false
    }

    /// Called when the aggregation completes.
    fn on_completion(&self, exchange: &mut Exchange, input_exchange: Option<&Exchange>) {
        let _ = (exchange, input_exchange);
    }

    /// Called when an aggregation (of a multicast, splitter, ...) times out.
    ///
    /// # Arguments
    ///
    /// * `exchange` - the current aggregated exchange
    /// * `index` - the index of the exchange that timed out (`-1` from the aggregator)
    /// * `total` - the total number of exchanges (`-1` if not known)
    /// * `timeout_millis` - the timeout (`-1` if not known)
    fn timeout(&self, exchange: &mut Exchange, index: i32, total: i32, timeout_millis: i64) {
        let _ = (exchange, index, total, timeout_millis);
    }

    /// Called when an optimistic lock failure happens in the aggregator.
    fn on_optimistic_lock_failure(&self, old_exchange: &Exchange, new_exchange: &Exchange) {
        let _ = (old_exchange, new_exchange);
    }
}

/// A shared aggregation strategy.
pub type SharedAggregationStrategy = Arc<dyn AggregationStrategy>;

/// An aggregation strategy from a closure (the [`AggregationStrategy::aggregate`] method).
pub struct FnAggregationStrategy<F>(F);

impl<F> AggregationStrategy for FnAggregationStrategy<F>
where
    F: Fn(Option<Exchange>, Option<Exchange>) -> CamelResult<Option<Exchange>> + Send + Sync,
{
    fn aggregate(
        &self,
        old_exchange: Option<Exchange>,
        new_exchange: Option<Exchange>,
    ) -> CamelResult<Option<Exchange>> {
        (self.0)(old_exchange, new_exchange)
    }
}

/// Creates an aggregation strategy from a closure.
///
/// # Examples
///
/// ```
/// use camel_api::processor::{aggregation_fn, AggregationStrategy};
/// use camel_api::context::StandaloneContext;
/// use camel_api::exchange::Exchange;
///
/// let concat = aggregation_fn(|old: Option<Exchange>, new: Option<Exchange>| {
///     Ok(match (old, new) {
///         (None, new) => new,
///         (Some(mut old), Some(new)) => {
///             let body = format!("{}+{}", old.message().body(), new.message().body());
///             old.message_mut().set_body(body);
///             Some(old)
///         }
///         (old, None) => old,
///     })
/// });
/// let ctx = StandaloneContext::shared();
/// let a = concat.aggregate(None, Some(Exchange::with_body(ctx.clone(), "A"))).unwrap();
/// let ab = concat.aggregate(a, Some(Exchange::with_body(ctx, "B"))).unwrap().unwrap();
/// assert_eq!("A+B", ab.message().body().to_string());
/// ```
pub fn aggregation_fn<F>(f: F) -> FnAggregationStrategy<F>
where
    F: Fn(Option<Exchange>, Option<Exchange>) -> CamelResult<Option<Exchange>> + Send + Sync,
{
    FnAggregationStrategy(f)
}

/// Runs a future to completion on the current thread (a minimal executor for tests and
/// synchronous callers; the routing engine uses an async runtime).
pub fn block_on<F: Future>(future: F) -> F::Output {
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Wake, Waker};

    struct Signal {
        woken: Mutex<bool>,
        cond: Condvar,
    }

    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            *self.woken.lock().unwrap() = true;
            self.cond.notify_one();
        }
    }

    let signal = Arc::new(Signal {
        woken: Mutex::new(false),
        cond: Condvar::new(),
    });
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            return v;
        }
        let mut woken = signal.woken.lock().unwrap();
        while !*woken {
            woken = signal.cond.wait(woken).unwrap();
        }
        *woken = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StandaloneContext;
    use crate::error::CamelError;

    #[test]
    fn processors() {
        let p: SharedProcessor = Arc::new(processor_fn(|ex: &mut Exchange| {
            ex.message_mut().set_header("seen", true);
            Ok(())
        }));
        let fail: SharedProcessor = Arc::new(processor_fn(|_ex: &mut Exchange| {
            Err(CamelError::runtime("boom"))
        }));
        let mut ex = Exchange::new(StandaloneContext::shared());
        block_on(p.process(&mut ex)).unwrap();
        assert_eq!(Some(true), ex.message().header_as::<bool>("seen").unwrap());
        assert_eq!(
            "boom",
            block_on(fail.process(&mut ex)).unwrap_err().to_string()
        );
    }

    #[test]
    fn expressions_and_predicates() {
        let ctx = StandaloneContext::shared();
        let ex = Exchange::with_body(ctx.clone(), "hello");
        let body = expression_fn(|ex: &Exchange| Ok(ex.message().body().clone()));
        assert_eq!(Value::from("hello"), body.evaluate(&ex).unwrap());
        assert!(body.init(&ctx).is_ok());
        let c = ConstantExpression(Value::from(5));
        assert_eq!(Value::from(5), c.evaluate(&ex).unwrap());
        assert_eq!("5", c.to_string());
        assert!(ExpressionPredicate(Arc::new(body)).matches(&ex).unwrap());
        assert!(
            !ExpressionPredicate(Arc::new(ConstantExpression(Value::from("false"))))
                .matches(&ex)
                .unwrap()
        );
        let p = predicate_fn(|_ex: &Exchange| Ok(true));
        assert!(p.matches(&ex).unwrap());
        assert!(p.init(&ctx).is_ok());
    }

    #[test]
    fn aggregation_defaults() {
        let s = aggregation_fn(|_old: Option<Exchange>, new: Option<Exchange>| Ok(new));
        let ctx = StandaloneContext::shared();
        let mut a = Exchange::new(ctx.clone());
        let b = Exchange::new(ctx);
        assert!(!s.can_pre_complete());
        assert!(!s.pre_complete(Some(&a), Some(&b)));
        s.on_completion(&mut a, None);
        s.timeout(&mut a, 0, 1, 10);
        s.on_optimistic_lock_failure(&a, &b);
        let id = b.exchange_id().to_string();
        let r = s.aggregate_with_input(None, Some(b), &a).unwrap().unwrap();
        assert_eq!(id, r.exchange_id());
    }

    #[test]
    fn block_on_waits_for_wakeups() {
        struct Later(bool);
        impl Future for Later {
            type Output = i32;
            fn poll(
                mut self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<i32> {
                if self.0 {
                    std::task::Poll::Ready(7)
                } else {
                    self.0 = true;
                    let waker = cx.waker().clone();
                    std::thread::spawn(move || waker.wake());
                    std::task::Poll::Pending
                }
            }
        }
        assert_eq!(7, block_on(Later(false)));
    }
}
