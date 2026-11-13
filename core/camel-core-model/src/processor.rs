//! The processor definitions of the model (Java `ProcessorDefinition` and its subclasses such
//! as `ToDefinition`, `FilterDefinition` and `ChoiceDefinition`).

use crate::expression::ExpressionDefinition;
use camel_api::enums::{ExchangePattern, LoggingLevel};
use camel_api::error::{CamelError, ErrorType};
use camel_api::processor::{SharedAggregationStrategy, SharedProcessor};
use camel_api::spi::aggregation_repository::AggregationRepository;
use camel_util::uri_support::sanitize_uri;
use camel_util::value::Value;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

/// The idempotent repository of an idempotent consumer: a bean of the registry, or the
/// repository itself.
#[derive(Clone)]
pub enum IdempotentRepositoryRef {
    /// The name of the repository in the registry.
    Ref(String),
    /// The repository.
    Repository(camel_api::spi::idempotent_repository::SharedIdempotentRepository),
}

impl fmt::Debug for IdempotentRepositoryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdempotentRepositoryRef::Ref(r) => write!(f, "Ref({r})"),
            IdempotentRepositoryRef::Repository(_) => f.write_str("Repository"),
        }
    }
}

impl From<&str> for IdempotentRepositoryRef {
    fn from(name: &str) -> Self {
        IdempotentRepositoryRef::Ref(name.to_string())
    }
}

impl From<camel_api::spi::idempotent_repository::SharedIdempotentRepository>
    for IdempotentRepositoryRef
{
    fn from(repository: camel_api::spi::idempotent_repository::SharedIdempotentRepository) -> Self {
        IdempotentRepositoryRef::Repository(repository)
    }
}

/// The idempotent consumer EIP (Java `IdempotentConsumerDefinition`).
#[derive(Debug, Clone)]
pub struct IdempotentConsumerDefinition {
    /// The message id.
    pub message_id: ExpressionDefinition,
    /// The repository of the ids.
    pub repository: IdempotentRepositoryRef,
    /// Whether the id is added before processing (default `true`).
    pub eager: bool,
    /// Whether the completion work is done when the block ends (default `false`).
    pub completion_eager: bool,
    /// Whether duplicates are skipped (default `true`).
    pub skip_duplicate: bool,
    /// Whether the id is removed when the exchange fails (default `true`).
    pub remove_on_failure: bool,
    /// The steps of a new message.
    pub steps: Vec<ProcessorDefinition>,
}

/// How a load balancer chooses the step (Java `LoadBalancerDefinition`s).
#[derive(Debug, Clone)]
pub enum LoadBalancerDefinition {
    /// The steps in turn.
    RoundRobin,
    /// A step at random.
    Random,
    /// The same step for the same correlation key.
    Sticky(ExpressionDefinition),
    /// All the steps, one after the other, each with a copy.
    Topic,
    /// The first step, then the next one when it fails.
    Failover {
        /// The errors that fail over (empty: any error).
        exceptions: Vec<ErrorType>,
        /// Whether each message starts at the next step.
        round_robin: bool,
        /// Whether each message starts at the last step that succeeded.
        sticky: bool,
        /// The maximum number of failover attempts (`-1`: no limit).
        maximum_failover_attempts: i32,
    },
    /// The steps by weights, such as `4,2,1`.
    Weighted {
        /// Whether the steps are chosen in turn, else at random.
        round_robin: bool,
        /// The distribution ratios, separated by the delimiter.
        distribution_ratio: String,
        /// The delimiter of the ratios (default `,`).
        delimiter: String,
    },
}

impl LoadBalancerDefinition {
    /// A failover on any error, without limit.
    pub fn failover() -> Self {
        LoadBalancerDefinition::Failover {
            exceptions: Vec::new(),
            round_robin: false,
            sticky: false,
            maximum_failover_attempts: -1,
        }
    }

    /// A weighted balancer of the ratios (such as `4,2,1`).
    pub fn weighted(round_robin: bool, distribution_ratio: &str) -> Self {
        LoadBalancerDefinition::Weighted {
            round_robin,
            distribution_ratio: distribution_ratio.to_string(),
            delimiter: ",".to_string(),
        }
    }
}

impl fmt::Display for LoadBalancerDefinition {
    /// Java `toString()` of the load balancer definitions.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadBalancerDefinition::RoundRobin => f.write_str("RoundRobinLoadBalancer"),
            LoadBalancerDefinition::Random => f.write_str("RandomLoadBalancer"),
            LoadBalancerDefinition::Sticky(e) => write!(f, "StickyLoadBalancer[{e}]"),
            LoadBalancerDefinition::Topic => f.write_str("TopicLoadBalancer"),
            LoadBalancerDefinition::Failover { .. } => f.write_str("FailoverLoadBalancer"),
            LoadBalancerDefinition::Weighted {
                round_robin,
                distribution_ratio,
                ..
            } => {
                if *round_robin {
                    write!(f, "WeightedRoundRobinLoadBalancer[{distribution_ratio}]")
                } else {
                    write!(f, "WeightedRandomLoadBalancer[{distribution_ratio}]")
                }
            }
        }
    }
}

/// The mode of a resequencer and its options.
#[derive(Debug, Clone)]
pub enum ResequenceMode {
    /// The batch mode.
    Batch {
        /// The maximum number of exchanges of a batch (default 100).
        size: usize,
        /// Whether exchanges with the same expression value are all kept.
        allow_duplicates: bool,
        /// Whether the order is reversed.
        reverse: bool,
    },
    /// The stream mode: the expression is a sequence number.
    Stream {
        /// The maximum number of exchanges waiting (default 1000).
        capacity: usize,
        /// Whether exchanges older than the last sent one fail.
        reject_old: bool,
    },
}

/// The resequence EIP (Java `ResequenceDefinition`).
#[derive(Debug, Clone)]
pub struct ResequenceDefinition {
    /// The order (batch) or sequence number (stream) of the exchanges.
    pub expression: ExpressionDefinition,
    /// The mode.
    pub mode: ResequenceMode,
    /// The batch timeout or the time to wait for a missing exchange, in millis (default 1000).
    pub timeout: u64,
    /// Whether exchanges without an expression value are ignored instead of failing.
    pub ignore_invalid_exchanges: bool,
    /// The steps receiving the exchanges in order.
    pub steps: Vec<ProcessorDefinition>,
}

/// A comparator of the sort EIP.
pub type SharedComparator = Arc<dyn Fn(&Value, &Value) -> Ordering + Send + Sync>;

/// The comparator of a sort step.
#[derive(Clone)]
pub struct SortComparator(pub SharedComparator);

impl fmt::Debug for SortComparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SortComparator")
    }
}

/// A step of a route: what it does ([`Eip`]) and its common options.
#[derive(Clone, Debug)]
pub struct ProcessorDefinition {
    /// The id of the node (generated, such as `to1`, when not set).
    pub id: Option<String>,
    /// The description of the node.
    pub description: Option<String>,
    /// Whether the node is disabled (then it is left out of the route).
    pub disabled: bool,
    /// What the node does.
    pub eip: Eip,
}

impl ProcessorDefinition {
    /// A node doing the EIP.
    pub fn new(eip: Eip) -> Self {
        ProcessorDefinition {
            id: None,
            description: None,
            disabled: false,
            eip,
        }
    }

    /// The short name of the node type, used as the prefix of generated ids (Java
    /// `getShortName()`).
    pub fn short_name(&self) -> &'static str {
        self.eip.short_name()
    }

    /// The nested steps of the node (for filter, pipeline, ...), if any.
    pub fn outputs(&self) -> Vec<&ProcessorDefinition> {
        match &self.eip {
            Eip::Filter { steps, .. }
            | Eip::Pipeline { steps }
            | Eip::LoadBalance { steps, .. }
            | Eip::Loop { steps, .. }
            | Eip::Step { steps, .. }
            | Eip::Split { steps, .. }
            | Eip::Multicast { steps, .. } => steps.iter().collect(),
            Eip::Aggregate(a) => a.steps.iter().collect(),
            Eip::IdempotentConsumer(i) => i.steps.iter().collect(),
            Eip::Resequence(r) => r.steps.iter().collect(),
            Eip::OnCompletion(c) => c.steps.iter().collect(),
            Eip::DoTry {
                steps,
                catches,
                finally,
            } => {
                let mut v: Vec<&ProcessorDefinition> = steps.iter().collect();
                v.extend(catches.iter().flat_map(|c| c.steps.iter()));
                if let Some(f) = finally {
                    v.extend(f.iter());
                }
                v
            }
            Eip::Choice { whens, otherwise } => {
                let mut v: Vec<&ProcessorDefinition> =
                    whens.iter().flat_map(|w| w.steps.iter()).collect();
                if let Some(o) = otherwise {
                    v.extend(o.iter());
                }
                v
            }
            _ => vec![],
        }
    }
}

/// How a `process` step gets its processor.
#[derive(Clone)]
pub enum ProcessRef {
    /// A processor bean looked up in the registry by name.
    Ref(String),
    /// A processor given in Rust code.
    Processor(SharedProcessor),
}

impl fmt::Debug for ProcessRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessRef::Ref(r) => write!(f, "Ref({r})"),
            ProcessRef::Processor(_) => f.write_str("Processor"),
        }
    }
}

/// The data format of a `marshal` or `unmarshal` (Java `DataFormatDefinition`).
#[derive(Clone)]
pub enum DataFormatDefinition {
    /// A data format by name (such as `base64`), with its options, created by the factory of
    /// the context (or a bean of the registry without options).
    Named {
        /// The name of the data format.
        name: String,
        /// The options of the data format, such as `lineLength=76`.
        options: Vec<(String, String)>,
    },
    /// A data format bean of the registry (Java `custom("name")`).
    Custom(String),
    /// A data format given in Rust code.
    DataFormat(camel_api::spi::data_format::SharedDataFormat),
}

impl DataFormatDefinition {
    /// The data format by name, without options.
    pub fn named(name: &str) -> Self {
        DataFormatDefinition::Named {
            name: name.to_string(),
            options: Vec::new(),
        }
    }

    /// With the option (of a named data format).
    pub fn option(mut self, key: &str, value: impl ToString) -> Self {
        if let DataFormatDefinition::Named { options, .. } = &mut self {
            options.push((key.to_string(), value.to_string()));
        }
        self
    }
}

impl From<camel_api::spi::data_format::SharedDataFormat> for DataFormatDefinition {
    fn from(df: camel_api::spi::data_format::SharedDataFormat) -> Self {
        DataFormatDefinition::DataFormat(df)
    }
}

impl From<&str> for DataFormatDefinition {
    fn from(name: &str) -> Self {
        DataFormatDefinition::named(name)
    }
}

impl fmt::Debug for DataFormatDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for DataFormatDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataFormatDefinition::Named { name, .. } => f.write_str(name),
            DataFormatDefinition::Custom(r) => write!(f, "custom[{r}]"),
            DataFormatDefinition::DataFormat(d) => f.write_str(d.name()),
        }
    }
}

/// How an EIP gets its aggregation strategy.
#[derive(Clone)]
pub enum AggregationRef {
    /// A strategy bean looked up in the registry by name.
    Ref(String),
    /// A strategy given in Rust code.
    Strategy(SharedAggregationStrategy),
}

impl fmt::Debug for AggregationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AggregationRef::Ref(r) => write!(f, "Ref({r})"),
            AggregationRef::Strategy(_) => f.write_str("Strategy"),
        }
    }
}

/// An aggregation repository given in Rust code.
#[derive(Clone)]
pub struct RepositoryRef(pub Arc<dyn AggregationRepository>);

impl fmt::Debug for RepositoryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AggregationRepository")
    }
}

/// The aggregate EIP (Java `AggregateDefinition`); the options left `None` use the defaults
/// of the aggregator.
#[derive(Debug, Clone)]
pub struct AggregateDefinition {
    /// The correlation key of the exchanges.
    pub correlation: ExpressionDefinition,
    /// The aggregation strategy (required).
    pub strategy: Option<AggregationRef>,
    /// The steps of the completed exchanges.
    pub steps: Vec<ProcessorDefinition>,
    /// The number of exchanges of a complete group.
    pub completion_size: Option<i32>,
    /// The number of exchanges of a complete group, per group.
    pub completion_size_expression: Option<ExpressionDefinition>,
    /// The inactivity timeout of a group in millis.
    pub completion_timeout: Option<i64>,
    /// The inactivity timeout in millis, per group.
    pub completion_timeout_expression: Option<ExpressionDefinition>,
    /// How often the timeouts are checked in millis.
    pub completion_timeout_checker_interval: Option<u64>,
    /// Completes all groups every interval millis.
    pub completion_interval: Option<i64>,
    /// Completes a group when it matches (a predicate).
    pub completion_predicate: Option<ExpressionDefinition>,
    /// Checks the completion on the incoming exchange.
    pub eager_check_completion: bool,
    /// Ignores exchanges without correlation key.
    pub ignore_invalid_correlation_keys: bool,
    /// Closes the keys of completed groups (the number of keys remembered, 0 for no limit).
    pub close_correlation_key_on_completion: Option<usize>,
    /// Discards groups completed by timeout.
    pub discard_on_completion_timeout: bool,
    /// Discards the group when the strategy fails.
    pub discard_on_aggregation_failure: bool,
    /// Completes all groups when a new group starts.
    pub completion_on_new_correlation_group: bool,
    /// Completes all groups when the route stops.
    pub force_completion_on_stop: bool,
    /// Completes all groups when the route stops.
    pub complete_all_on_stop: bool,
    /// The repository of the groups (default in memory).
    pub repository: Option<RepositoryRef>,
}

impl AggregateDefinition {
    /// A definition with the correlation, strategy and steps, and the default options.
    pub fn new(
        correlation: ExpressionDefinition,
        strategy: Option<AggregationRef>,
        steps: Vec<ProcessorDefinition>,
    ) -> Self {
        AggregateDefinition {
            correlation,
            strategy,
            steps,
            completion_size: None,
            completion_size_expression: None,
            completion_timeout: None,
            completion_timeout_expression: None,
            completion_timeout_checker_interval: None,
            completion_interval: None,
            completion_predicate: None,
            eager_check_completion: false,
            ignore_invalid_correlation_keys: false,
            close_correlation_key_on_completion: None,
            discard_on_completion_timeout: false,
            discard_on_aggregation_failure: false,
            completion_on_new_correlation_group: false,
            force_completion_on_stop: false,
            complete_all_on_stop: false,
            repository: None,
        }
    }
}

/// The options of the processing of the copies of multicast, split and recipient list (Java
/// `MulticastDefinition`, `SplitDefinition` and `RecipientListDefinition` options).
#[derive(Debug, Clone, Default)]
pub struct MulticastOptions {
    /// Whether to stop on the first failed copy (`stopOnException`).
    pub stop_on_exception: bool,
    /// Whether the copies are processed concurrently (`parallelProcessing`).
    pub parallel_processing: bool,
    /// Whether the results are aggregated in the order they complete instead of the order
    /// of the copies (`streaming`).
    pub streaming: bool,
    /// Whether the aggregation strategy may be called concurrently (`parallelAggregate`; the
    /// Rust port always aggregates from one task, so this only documents the strategy).
    pub parallel_aggregate: bool,
    /// The total timeout in millis of the parallel processing (`timeout`).
    pub timeout: Option<u64>,
    /// The processor preparing each copy before it is sent (`onPrepare`).
    pub on_prepare: Option<ProcessRef>,
}

/// The onCompletion EIP (Java `OnCompletionDefinition`): steps routing a copy of the exchange
/// when it is done.
#[derive(Debug, Clone, Default)]
pub struct OnCompletionDefinition {
    /// The steps.
    pub steps: Vec<ProcessorDefinition>,
    /// Only when the exchange completed successfully.
    pub on_complete_only: bool,
    /// Only when the exchange failed.
    pub on_failure_only: bool,
    /// Only when the exchange matches (a predicate).
    pub on_when: Option<ExpressionDefinition>,
    /// Routes the original in message.
    pub use_original_message: bool,
    /// Runs when the route is done, before its consumer continues (Java mode
    /// `BeforeConsumer`; default `AfterConsumer`).
    pub before_consumer: bool,
    /// Routes the copy in the background.
    pub parallel_processing: bool,
}

/// A `when` clause of a choice.
#[derive(Clone, Debug)]
pub struct WhenDefinition {
    /// The predicate.
    pub predicate: ExpressionDefinition,
    /// The steps when the predicate matches.
    pub steps: Vec<ProcessorDefinition>,
}

/// A `doCatch` clause of a `doTry`.
#[derive(Clone, Debug)]
pub struct CatchDefinition {
    /// The types of the errors to catch.
    pub exceptions: Vec<ErrorType>,
    /// An extra predicate the exchange must match.
    pub on_when: Option<ExpressionDefinition>,
    /// The steps handling the error.
    pub steps: Vec<ProcessorDefinition>,
}

impl fmt::Display for CatchDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.exceptions.iter().map(|e| e.to_string()).collect();
        write!(f, "DoCatch[{} -> {}]", names.join(","), Steps(&self.steps))
    }
}

/// What a step does: the Enterprise Integration Patterns of the model.
#[derive(Clone, Debug)]
pub enum Eip {
    /// Sends to an endpoint (`to`).
    To {
        /// The endpoint uri.
        uri: String,
        /// The exchange pattern to send with.
        pattern: Option<ExchangePattern>,
        /// The variable whose value is the body sent (`variableSend`).
        variable_send: Option<String>,
        /// The variable receiving the reply instead of the message (`variableReceive`).
        variable_receive: Option<String>,
    },
    /// Continues only with the exchanges matching the predicate (`filter`).
    Filter {
        /// The predicate.
        predicate: ExpressionDefinition,
        /// The steps for the matching exchanges.
        steps: Vec<ProcessorDefinition>,
        /// The name of an exchange property storing whether the predicate matched.
        status_property_name: Option<String>,
    },
    /// The content based router (`choice`).
    Choice {
        /// The `when` clauses, tried in order.
        whens: Vec<WhenDefinition>,
        /// The steps when no `when` matches.
        otherwise: Option<Vec<ProcessorDefinition>>,
    },
    /// Steps in sequence (`pipeline`).
    Pipeline {
        /// The steps.
        steps: Vec<ProcessorDefinition>,
    },
    /// Sets the message body (`setBody`).
    SetBody(ExpressionDefinition),
    /// Transforms the message body (`transform`).
    Transform(ExpressionDefinition),
    /// Sets a header (`setHeader`).
    SetHeader {
        /// The header name.
        name: String,
        /// The value.
        expression: ExpressionDefinition,
    },
    /// Sets an exchange property (`setProperty`).
    SetProperty {
        /// The property name.
        name: String,
        /// The value.
        expression: ExpressionDefinition,
    },
    /// Sets a variable (`setVariable`).
    SetVariable {
        /// The variable name.
        name: String,
        /// The value.
        expression: ExpressionDefinition,
    },
    /// Removes a header (`removeHeader`).
    RemoveHeader(String),
    /// Removes the headers matching a pattern (`removeHeaders`).
    RemoveHeaders {
        /// The pattern (name, wildcard or regular expression).
        pattern: String,
        /// The patterns of headers to keep.
        exclude_patterns: Vec<String>,
    },
    /// Logs a message (`log`), in the simple language.
    Log {
        /// The message.
        message: String,
        /// The logging level (default `INFO`).
        level: LoggingLevel,
        /// The logger name (default the `CamelLogEipName` global option, else the route id).
        log_name: Option<String>,
        /// The language of the message (default the `CamelLogEipLanguage` global option, else
        /// simple).
        log_language: Option<String>,
    },
    /// Stops routing the exchange (`stop`).
    Stop,
    /// Runs a processor (`process`).
    Process(ProcessRef),
    /// Fails the exchange with an error (`throwException`).
    ThrowException(CamelError),
    /// Fails the exchange with an error of the kind and the message, a simple expression
    /// evaluated with the exchange (`throwException(type, message)`; `None`: no message).
    ThrowExceptionOf {
        /// The kind of the error.
        kind: camel_api::error::ErrorKind,
        /// The message (a simple expression).
        message: Option<String>,
    },
    /// Splits the message into parts processed by the steps (`split`).
    Split {
        /// The expression of the parts.
        expression: ExpressionDefinition,
        /// The steps of each part.
        steps: Vec<ProcessorDefinition>,
        /// The aggregation of the parts (default: the original exchange).
        strategy: Option<AggregationRef>,
        /// How the parts are processed.
        options: MulticastOptions,
        /// The delimiter of texts (default `,`).
        delimiter: Option<String>,
    },
    /// Sends a copy to each step (`multicast`).
    Multicast {
        /// The outputs (each step gets a copy).
        steps: Vec<ProcessorDefinition>,
        /// The aggregation of the replies (default: the last reply).
        strategy: Option<AggregationRef>,
        /// How the copies are processed.
        options: MulticastOptions,
    },
    /// Routes a copy of the exchange when it is done (`onCompletion`; only directly in a
    /// route, where it applies to that route wherever it is).
    OnCompletion(Box<OnCompletionDefinition>),
    /// Aggregates exchanges by correlation key (`aggregate`).
    Aggregate(Box<AggregateDefinition>),
    /// Delays the exchange by the millis of the expression (`delay`).
    Delay(ExpressionDefinition),
    /// Sends to the endpoint of a uri in the simple language (`toD`).
    ToD {
        /// The uri (a simple expression).
        uri: String,
        /// The exchange pattern to send with.
        pattern: Option<ExchangePattern>,
        /// Whether an invalid endpoint uri is ignored.
        ignore_invalid_endpoint: bool,
    },
    /// Sends a copy to each endpoint of the expression (`recipientList`).
    RecipientList {
        /// The endpoint uris.
        expression: ExpressionDefinition,
        /// The delimiter of the uris in a text (default `,`).
        delimiter: Option<String>,
        /// The aggregation of the replies (default: the last reply).
        strategy: Option<AggregationRef>,
        /// How the copies are processed.
        options: MulticastOptions,
        /// Whether invalid endpoint uris are ignored.
        ignore_invalid_endpoints: bool,
    },
    /// Sends a copy to the endpoint in the background (`wireTap`).
    WireTap {
        /// The uri (a simple expression).
        uri: String,
        /// Whether an invalid endpoint uri is ignored.
        ignore_invalid_endpoint: bool,
        /// The processor preparing the copy before it is sent (`onPrepare`).
        on_prepare: Option<ProcessRef>,
        /// The variable whose value is the body sent (`variableSend`).
        variable_send: Option<String>,
    },
    /// Sets the exchange pattern (`setExchangePattern`).
    SetExchangePattern(ExchangePattern),
    /// Converts the body to the Java type (`convertBodyTo`).
    ConvertBodyTo {
        /// The Java type name, such as `String` or `java.lang.Integer`.
        type_name: String,
        /// Whether a body that cannot be converted fails (default `true`).
        mandatory: bool,
        /// The charset of a conversion between text and bytes (Java `charset`).
        charset: Option<String>,
    },
    /// Calls a method of a bean (`bean`): by name in the registry or the bean itself; the
    /// result becomes the message body.
    Bean {
        /// The bean.
        bean: camel_api::bean_binding::BeanReference,
        /// The method (a method call such as `hello('World')`), `None` for the only method of
        /// the bean.
        method: Option<String>,
    },
    /// Sets several headers, in order (`setHeaders`).
    SetHeaders(Vec<(String, ExpressionDefinition)>),
    /// Sets several variables, in order (`setVariables`).
    SetVariables(Vec<(String, ExpressionDefinition)>),
    /// Removes an exchange property (`removeProperty`).
    RemoveProperty(String),
    /// Removes the exchange properties matching the pattern (`removeProperties`).
    RemoveProperties {
        /// The pattern of the names.
        pattern: String,
        /// The patterns of the names to keep.
        exclude_patterns: Vec<String>,
    },
    /// Removes a variable, all of them with `*` (`removeVariable`).
    RemoveVariable(String),
    /// Evaluates an expression for its side effects (`script`).
    Script(ExpressionDefinition),
    /// Processes the steps a number of times or while a predicate matches (`loop`).
    Loop {
        /// The number of iterations.
        count: Option<ExpressionDefinition>,
        /// The predicate of `loopDoWhile`.
        while_predicate: Option<ExpressionDefinition>,
        /// Whether each iteration starts from a copy of the original exchange.
        copy: bool,
        /// The steps of each iteration.
        steps: Vec<ProcessorDefinition>,
    },
    /// Sends to the endpoints of the slip one after the other (`routingSlip`).
    RoutingSlip {
        /// The uris of the slip.
        expression: ExpressionDefinition,
        /// The delimiter of the uris (default `,`).
        delimiter: Option<String>,
        /// Whether invalid endpoint uris are skipped.
        ignore_invalid_endpoints: bool,
    },
    /// Sends to the endpoints the expression answers, evaluated again after each one
    /// (`dynamicRouter`).
    DynamicRouter {
        /// The uris of the next endpoints, nothing to end.
        expression: ExpressionDefinition,
        /// The delimiter of the uris (default `,`).
        delimiter: Option<String>,
        /// Whether invalid endpoint uris are skipped.
        ignore_invalid_endpoints: bool,
    },
    /// Sorts the list of the expression, by default the body, as the body (`sort`).
    Sort {
        /// The list to sort (`None`: the body).
        expression: Option<ExpressionDefinition>,
        /// The comparator (`None`: the natural order).
        comparator: Option<SortComparator>,
    },
    /// The steps as a step with an id (`step`).
    Step {
        /// The id of the step.
        id: String,
        /// The steps.
        steps: Vec<ProcessorDefinition>,
    },
    /// Fails the exchange when the predicate does not match (`validate`).
    Validate(ExpressionDefinition),
    /// Processes a message once per message id (`idempotentConsumer`).
    IdempotentConsumer(Box<IdempotentConsumerDefinition>),
    /// Continues the routing on a pool of tasks (`threads`): the following steps of the route
    /// run on it.
    Threads(ThreadsDefinition),
    /// Sends each message to one of the steps (`loadBalance`).
    LoadBalance {
        /// How the step is chosen.
        balancer: LoadBalancerDefinition,
        /// The steps balanced over.
        steps: Vec<ProcessorDefinition>,
    },
    /// Sends a copy to the endpoint and merges the reply (`enrich`).
    Enrich {
        /// The uri (an expression).
        uri: ExpressionDefinition,
        /// The strategy merging the reply (`None`: the reply replaces the message).
        strategy: Option<AggregationRef>,
        /// Whether a failed reply is merged too.
        aggregate_on_exception: bool,
        /// Whether an invalid endpoint uri is ignored.
        ignore_invalid_endpoint: bool,
        /// The variable sent as the body instead of the message body (`variableSend`).
        variable_send: Option<String>,
        /// The variable receiving the reply instead of the message (`variableReceive`).
        variable_receive: Option<String>,
    },
    /// Polls a message from the endpoint and merges it (`pollEnrich`).
    PollEnrich {
        /// The uri (an expression).
        uri: ExpressionDefinition,
        /// The timeout in millis: negative waits without limit, `0` does not wait.
        timeout: i64,
        /// The strategy merging the polled message.
        strategy: Option<AggregationRef>,
        /// Whether a failed message is merged too.
        aggregate_on_exception: bool,
        /// Whether an invalid endpoint uri is ignored.
        ignore_invalid_endpoint: bool,
        /// The variable receiving the polled message instead of the message
        /// (`variableReceive`).
        variable_receive: Option<String>,
    },
    /// Marshals the body with the data format (`marshal`).
    Marshal {
        /// The data format.
        data_format: DataFormatDefinition,
        /// The variable marshalled instead of the body (`variableSend`).
        variable_send: Option<String>,
        /// The variable receiving the result instead of the body (`variableReceive`).
        variable_receive: Option<String>,
        /// Whether a null body is left as is (`allowNullBody`).
        allow_null_body: bool,
    },
    /// Unmarshals the body with the data format (`unmarshal`).
    Unmarshal {
        /// The data format.
        data_format: DataFormatDefinition,
        /// The variable unmarshalled instead of the body (`variableSend`).
        variable_send: Option<String>,
        /// The variable receiving the result instead of the body (`variableReceive`).
        variable_receive: Option<String>,
        /// Whether a null body is left as is (`allowNullBody`).
        allow_null_body: bool,
    },
    /// Polls a message from the endpoint as the new message (`poll`, Java `PollDefinition`).
    Poll {
        /// The uri (an expression).
        uri: ExpressionDefinition,
        /// The timeout in millis: negative waits without limit, `0` does not wait.
        timeout: i64,
    },
    /// Lets one exchange through per number of messages or per period (`sample`).
    Sample {
        /// The number of messages per sample.
        message_frequency: Option<u64>,
        /// The sample period in millis (default 1000).
        period_millis: u64,
    },
    /// Limits the number of exchanges per period or at the same time (`throttle`).
    Throttle {
        /// The maximum number of requests.
        max_requests: ExpressionDefinition,
        /// The time period in millis of the total requests mode (default 1000).
        time_period_millis: u64,
        /// Whether the concurrent requests mode is used, else the total requests mode.
        concurrent: bool,
        /// The correlation key of separate limits.
        correlation: Option<ExpressionDefinition>,
        /// Whether exchanges over the limit fail instead of waiting.
        reject_execution: bool,
    },
    /// Stores a copy of the message and merges it back later (`claimCheck`).
    ClaimCheck {
        /// The operation: `Set`, `Get`, `GetAndRemove`, `Push` or `Pop`.
        operation: String,
        /// The key (a simple expression), for `Set`, `Get` and `GetAndRemove`.
        key: Option<String>,
        /// The rules of what is merged back (`body`, `headers`, `header:pattern`, ...).
        filter: Option<String>,
        /// The strategy merging the message back, instead of the filter.
        strategy: Option<AggregationRef>,
    },
    /// Marks the exchange for rollback (`rollback`, `markRollbackOnly`).
    Rollback {
        /// The message of the `RollbackExchangeException`.
        message: Option<String>,
        /// Whether the exchange is only marked for rollback (no exception).
        mark_rollback_only: bool,
        /// Whether only the last transaction is marked for rollback.
        mark_rollback_only_last: bool,
    },
    /// Converts a header (`convertHeaderTo`) or variable (`convertVariableTo`) to the Java type.
    ConvertValueTo {
        /// Whether it is a variable, else a header.
        variable: bool,
        /// The name.
        name: String,
        /// The name of the converted value (default the name).
        to_name: Option<String>,
        /// The Java type name.
        type_name: String,
        /// Whether a value that cannot be converted fails.
        mandatory: bool,
    },
    /// Sends the exchanges to the steps in the order of the expression (`resequence`).
    Resequence(Box<ResequenceDefinition>),
    /// `doTry` with its `doCatch` and `doFinally` clauses.
    DoTry {
        /// The steps of the try block.
        steps: Vec<ProcessorDefinition>,
        /// The catch clauses.
        catches: Vec<CatchDefinition>,
        /// The steps of the finally clause.
        finally: Option<Vec<ProcessorDefinition>>,
    },
}

impl Eip {
    /// The short name (Java `getShortName()`).
    pub fn short_name(&self) -> &'static str {
        match self {
            Eip::To { .. } => "to",
            Eip::Filter { .. } => "filter",
            Eip::Choice { .. } => "choice",
            Eip::Pipeline { .. } => "pipeline",
            Eip::SetBody(_) => "setBody",
            Eip::Transform(_) => "transform",
            Eip::SetHeader { .. } => "setHeader",
            Eip::SetProperty { .. } => "setProperty",
            Eip::SetVariable { .. } => "setVariable",
            Eip::RemoveHeader(_) => "removeHeader",
            Eip::RemoveHeaders { .. } => "removeHeaders",
            Eip::Log { .. } => "log",
            Eip::Stop => "stop",
            Eip::Process(_) => "process",
            Eip::ThrowException(_) | Eip::ThrowExceptionOf { .. } => "throwException",
            Eip::DoTry { .. } => "doTry",
            Eip::Split { .. } => "split",
            Eip::Multicast { .. } => "multicast",
            Eip::Delay(_) => "delay",
            Eip::Aggregate(_) => "aggregate",
            Eip::OnCompletion(_) => "onCompletion",
            Eip::ToD { .. } => "toD",
            Eip::RecipientList { .. } => "recipientList",
            Eip::WireTap { .. } => "wireTap",
            Eip::SetExchangePattern(_) => "setExchangePattern",
            Eip::ConvertBodyTo { .. } => "convertBodyTo",
            Eip::Bean { .. } => "bean",
            Eip::SetHeaders(_) => "setHeaders",
            Eip::SetVariables(_) => "setVariables",
            Eip::RemoveProperty(_) => "removeProperty",
            Eip::RemoveProperties { .. } => "removeProperties",
            Eip::RemoveVariable(_) => "removeVariable",
            Eip::Script(_) => "script",
            Eip::Loop { .. } => "loop",
            Eip::RoutingSlip { .. } => "routingSlip",
            Eip::DynamicRouter { .. } => "dynamicRouter",
            Eip::Sort { .. } => "sort",
            Eip::Step { .. } => "step",
            Eip::Validate(_) => "validate",
            Eip::Resequence(_) => "resequence",
            Eip::Throttle { .. } => "throttle",
            Eip::ClaimCheck { .. } => "claimCheck",
            Eip::Rollback { .. } => "rollback",
            Eip::ConvertValueTo {
                variable: false, ..
            } => "convertHeaderTo",
            Eip::ConvertValueTo { variable: true, .. } => "convertVariableTo",
            Eip::Enrich { .. } => "enrich",
            Eip::PollEnrich { .. } => "pollEnrich",
            Eip::Poll { .. } => "poll",
            Eip::Marshal { .. } => "marshal",
            Eip::Unmarshal { .. } => "unmarshal",
            Eip::Sample { .. } => "sample",
            Eip::IdempotentConsumer(_) => "idempotentConsumer",
            Eip::LoadBalance { .. } => "loadBalance",
            Eip::Threads(_) => "threads",
        }
    }
}

/// The options of the `threads` EIP (Java `ThreadsDefinition`); unset options take the
/// defaults of the default thread pool profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadsDefinition {
    /// The number of tasks kept running (default 10).
    pub pool_size: Option<i32>,
    /// The maximum number of tasks running (default 20).
    pub max_pool_size: Option<i32>,
    /// The maximum number of exchanges waiting for a task (default 1000, `-1` unbounded, `0`
    /// none).
    pub max_queue_size: Option<i32>,
    /// The keep alive time of idle tasks in seconds (kept for the configuration).
    pub keep_alive_time: Option<u64>,
    /// The name of the pool (default `Threads`).
    pub thread_name: Option<String>,
    /// What happens to an exchange when the pool is full (default `CallerRuns`).
    pub rejected_policy: Option<camel_api::enums::ThreadPoolRejectedPolicy>,
    /// Whether the caller runs the exchange when the pool is full and the policy is
    /// `CallerRuns` (default `true`; `false` rejects it).
    pub caller_runs_when_rejected: Option<bool>,
}

/// Formats the steps like a Java list: `[a, b]`.
pub struct Steps<'a>(pub &'a [ProcessorDefinition]);

impl fmt::Display for Steps<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (i, s) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{s}")?;
        }
        f.write_str("]")
    }
}

impl fmt::Display for WhenDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "When[{} -> {}]", self.predicate, Steps(&self.steps))
    }
}

impl fmt::Display for ProcessorDefinition {
    /// Java `toString()` of the definitions, such as `to[mock:a]` and
    /// `Filter[header(foo) == bar -> [to[mock:a]]]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.eip {
            Eip::To { uri, .. } => write!(f, "to[{}]", sanitize_uri(uri)),
            Eip::Filter {
                predicate, steps, ..
            } => write!(f, "Filter[{predicate} -> {}]", Steps(steps)),
            Eip::Choice { whens, otherwise } => {
                f.write_str("Choice[[")?;
                for (i, w) in whens.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{w}")?;
                }
                f.write_str("]")?;
                if let Some(o) = otherwise {
                    write!(f, " Otherwise[{}]", Steps(o))?;
                }
                f.write_str("]")
            }
            Eip::Pipeline { steps } => write!(f, "Pipeline[{}]", Steps(steps)),
            Eip::SetBody(e) => write!(f, "SetBody[{e}]"),
            Eip::Transform(e) => write!(f, "Transform[{e}]"),
            Eip::SetHeader { name, expression } => write!(f, "SetHeader[{name}, {expression}]"),
            Eip::SetProperty { name, expression } => write!(f, "SetProperty[{name}, {expression}]"),
            Eip::SetVariable { name, expression } => write!(f, "SetVariable[{name}, {expression}]"),
            Eip::RemoveHeader(name) => write!(f, "RemoveHeader[{name}]"),
            Eip::RemoveHeaders { pattern, .. } => write!(f, "RemoveHeaders[{pattern}]"),
            Eip::Log { message, .. } => write!(f, "Log[{message}]"),
            Eip::Stop => f.write_str("Stop"),
            Eip::Process(ProcessRef::Ref(r)) => write!(f, "process[ref:{r}]"),
            Eip::Process(ProcessRef::Processor(p)) => {
                write!(f, "process[Processor@{:p}]", std::sync::Arc::as_ptr(p))
            }
            Eip::ThrowException(e) => write!(
                f,
                "ThrowException[{}]",
                camel_api::error::error_type_name(e)
            ),
            Eip::ThrowExceptionOf { kind, .. } => write!(
                f,
                "ThrowException[{}]",
                camel_api::error::error_type_name(&camel_api::error::CamelError::of_kind(*kind))
            ),
            Eip::Split {
                expression, steps, ..
            } => write!(f, "Split[{expression} -> {}]", Steps(steps)),
            Eip::Multicast { steps, .. } => write!(f, "Multicast[{}]", Steps(steps)),
            Eip::Delay(e) => write!(f, "Delay[{e} -> []]"),
            Eip::Aggregate(a) => write!(f, "Aggregate[{} -> {}]", a.correlation, Steps(&a.steps)),
            Eip::OnCompletion(c) => write!(f, "onCompletion[{}]", Steps(&c.steps)),
            Eip::ToD { uri, .. } => write!(f, "DynamicTo[toD[{uri}]]"),
            Eip::RecipientList { expression, .. } => write!(f, "RecipientList[{expression}]"),
            Eip::WireTap { uri, .. } => write!(f, "WireTap[{uri}]"),
            Eip::SetExchangePattern(p) => write!(f, "setExchangePattern[pattern: {p}]"),
            Eip::ConvertBodyTo { type_name, .. } => write!(f, "ConvertBodyTo[{type_name}]"),
            Eip::SetHeaders(h) | Eip::SetVariables(h) => {
                let kind = if matches!(self.eip, Eip::SetHeaders(_)) {
                    "SetHeaders"
                } else {
                    "SetVariables"
                };
                let items: Vec<String> = h.iter().map(|(n, e)| format!("{n}={e}")).collect();
                write!(f, "{kind}[{}]", items.join(", "))
            }
            Eip::RemoveProperty(name) => write!(f, "RemoveProperty[{name}]"),
            Eip::RemoveProperties { pattern, .. } => write!(f, "RemoveProperties[{pattern}]"),
            Eip::RemoveVariable(name) => write!(f, "RemoveVariable[{name}]"),
            Eip::Script(e) => write!(f, "Script[{e}]"),
            Eip::Loop {
                count,
                while_predicate,
                steps,
                ..
            } => {
                let e = count
                    .as_ref()
                    .or(while_predicate.as_ref())
                    .map(|e| e.to_string())
                    .unwrap_or_default();
                write!(f, "Loop[{e} -> {}]", Steps(steps))
            }
            Eip::RoutingSlip { expression, .. } => write!(f, "RoutingSlip[{expression}]"),
            Eip::DynamicRouter { expression, .. } => write!(f, "DynamicRouter[{expression}]"),
            Eip::Sort { expression, .. } => match expression {
                Some(e) => write!(f, "sort[{e}]"),
                None => f.write_str("sort[body]"),
            },
            Eip::Step { steps, .. } => write!(f, "Step[{}]", Steps(steps)),
            Eip::Validate(e) => write!(f, "Validate[{e} -> []]"),
            Eip::Resequence(r) => {
                write!(f, "Resequencer[{} -> {}]", r.expression, Steps(&r.steps))
            }
            Eip::Throttle {
                max_requests,
                time_period_millis,
                concurrent,
                ..
            } => {
                if *concurrent {
                    write!(f, "Throttle[{max_requests} maximum concurrent requests]")
                } else {
                    write!(
                        f,
                        "Throttle[{max_requests} request per {time_period_millis} millis]"
                    )
                }
            }
            Eip::ClaimCheck { operation, .. } => write!(f, "ClaimCheck[{operation}]"),
            Eip::Rollback { message, .. } => match message {
                Some(m) => write!(f, "Rollback[{m}]"),
                None => f.write_str("Rollback"),
            },
            Eip::ConvertValueTo {
                variable,
                name,
                type_name,
                ..
            } => {
                let kind = if *variable {
                    "ConvertVariableTo"
                } else {
                    "ConvertHeaderTo"
                };
                write!(f, "{kind}[{name}: {type_name}]")
            }
            Eip::Enrich { uri, .. } => write!(f, "Enrich[{uri}]"),
            Eip::PollEnrich { uri, .. } => write!(f, "PollEnrich[{uri}]"),
            Eip::Poll { uri, .. } => write!(f, "Poll[{uri}]"),
            Eip::Marshal { data_format, .. } => write!(f, "Marshal[{data_format}]"),
            Eip::Unmarshal { data_format, .. } => write!(f, "Unmarshal[{data_format}]"),
            Eip::Sample {
                message_frequency,
                period_millis,
            } => match message_frequency {
                Some(n) => write!(f, "Sample[1 Exchange per {n} messages received -> []]"),
                None => write!(f, "Sample[1 Exchange per {period_millis} millis -> []]"),
            },
            Eip::IdempotentConsumer(i) => {
                write!(
                    f,
                    "IdempotentConsumer[{} -> {}]",
                    i.message_id,
                    Steps(&i.steps)
                )
            }
            Eip::LoadBalance { balancer, steps } => {
                write!(f, "LoadBalanceType[{balancer}, {}]", Steps(steps))
            }
            Eip::Threads(_) => f.write_str("Threads[[]]"),
            Eip::Bean { bean, method } => match bean {
                camel_api::bean_binding::BeanReference::Name(name) => match method {
                    Some(m) => write!(f, "Bean[ref:{name} method:{m}]"),
                    None => write!(f, "Bean[ref:{name}]"),
                },
                camel_api::bean_binding::BeanReference::Bean(b) => write!(f, "Bean[{b}]"),
            },
            Eip::DoTry {
                steps,
                catches,
                finally,
            } => {
                // Java: the clauses are outputs of the try definition
                let mut parts: Vec<String> = steps.iter().map(|s| s.to_string()).collect();
                parts.extend(catches.iter().map(|c| c.to_string()));
                if let Some(fs) = finally {
                    parts.push(format!("DoFinally[{}]", Steps(fs)));
                }
                write!(f, "DoTry[[{}]]", parts.join(", "))
            }
        }
    }
}
