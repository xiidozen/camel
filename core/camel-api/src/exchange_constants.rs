//! The names of the well-known exchange headers and properties (the constants of the Java
//! `org.apache.camel.Exchange` interface), and the typed [`ExchangePropertyKey`]s of the
//! internal exchange properties.

/// The `CamelAuthentication` header or exchange property.
pub const AUTHENTICATION: &str = "CamelAuthentication";
/// The `CamelAuthenticationFailurePolicyId` header or exchange property.
pub const AUTHENTICATION_FAILURE_POLICY_ID: &str = "CamelAuthenticationFailurePolicyId";
/// The `CamelAggregatedSize` header or exchange property.
pub const AGGREGATED_SIZE: &str = "CamelAggregatedSize";
/// The `CamelAggregatedTimeout` header or exchange property.
pub const AGGREGATED_TIMEOUT: &str = "CamelAggregatedTimeout";
/// The `CamelAggregatedCompletedBy` header or exchange property.
pub const AGGREGATED_COMPLETED_BY: &str = "CamelAggregatedCompletedBy";
/// The `CamelAggregatedCorrelationKey` header or exchange property.
pub const AGGREGATED_CORRELATION_KEY: &str = "CamelAggregatedCorrelationKey";
/// The `CamelAggregatedCollectionGuard` header or exchange property.
pub const AGGREGATED_COLLECTION_GUARD: &str = "CamelAggregatedCollectionGuard";
/// The `CamelAggregationStrategy` header or exchange property.
pub const AGGREGATION_STRATEGY: &str = "CamelAggregationStrategy";
/// The `CamelAggregationCompleteCurrentGroup` header or exchange property.
pub const AGGREGATION_COMPLETE_CURRENT_GROUP: &str = "CamelAggregationCompleteCurrentGroup";
/// The `CamelAggregationCompleteAllGroups` header or exchange property.
pub const AGGREGATION_COMPLETE_ALL_GROUPS: &str = "CamelAggregationCompleteAllGroups";
/// The `CamelAggregationCompleteAllGroupsInclusive` header or exchange property.
pub const AGGREGATION_COMPLETE_ALL_GROUPS_INCLUSIVE: &str =
    "CamelAggregationCompleteAllGroupsInclusive";
/// The `CamelAsyncWait` header or exchange property.
pub const ASYNC_WAIT: &str = "CamelAsyncWait";
/// The `CamelAttachmentsSize` header or exchange property.
pub const ATTACHMENTS_SIZE: &str = "CamelAttachmentsSize";
/// The `CamelBatchIndex` header or exchange property.
pub const BATCH_INDEX: &str = "CamelBatchIndex";
/// The `CamelBatchSize` header or exchange property.
pub const BATCH_SIZE: &str = "CamelBatchSize";
/// The `CamelBatchComplete` header or exchange property.
pub const BATCH_COMPLETE: &str = "CamelBatchComplete";
/// The `CamelBinding` header or exchange property.
pub const BINDING: &str = "CamelBinding";
/// The `breadcrumbId` header or exchange property.
pub const BREADCRUMB_ID: &str = "breadcrumbId";
/// The `CamelCharsetName` header or exchange property.
pub const CHARSET_NAME: &str = "CamelCharsetName";
/// The `CamelCircuitBreakerState` header or exchange property.
pub const CIRCUIT_BREAKER_STATE: &str = "CamelCircuitBreakerState";
/// The `CamelClaimCheckRepository` header or exchange property.
pub const CLAIM_CHECK_REPOSITORY: &str = "CamelClaimCheckRepository";
/// The `Content-Encoding` header or exchange property.
pub const CONTENT_ENCODING: &str = "Content-Encoding";
/// The `Content-Length` header or exchange property.
pub const CONTENT_LENGTH: &str = "Content-Length";
/// The `Content-Type` header or exchange property.
pub const CONTENT_TYPE: &str = "Content-Type";
/// The `CamelCookieHandler` header or exchange property.
pub const COOKIE_HANDLER: &str = "CamelCookieHandler";
/// The `CamelCorrelationId` header or exchange property.
pub const CORRELATION_ID: &str = "CamelCorrelationId";
/// The `CamelContentSchema` header or exchange property.
pub const CONTENT_SCHEMA: &str = "CamelContentSchema";
/// The `CamelContentSchemaType` header or exchange property.
pub const CONTENT_SCHEMA_TYPE: &str = "CamelContentSchemaType";
/// The `CamelDataSetIndex` header or exchange property.
pub const DATASET_INDEX: &str = "CamelDataSetIndex";
/// The `org.apache.camel.default.charset` header or exchange property.
pub const DEFAULT_CHARSET_PROPERTY: &str = "org.apache.camel.default.charset";
/// The `CamelDebuggerSelfTime` header or exchange property.
pub const DEBUGGER_SELF_TIME: &str = "CamelDebuggerSelfTime";
/// The `CamelDestinationOverrideUrl` header or exchange property.
pub const DESTINATION_OVERRIDE_URL: &str = "CamelDestinationOverrideUrl";
/// The `CamelDisableHttpStreamCache` header or exchange property.
pub const DISABLE_HTTP_STREAM_CACHE: &str = "CamelDisableHttpStreamCache";
/// The `CamelDuplicateMessage` header or exchange property.
pub const DUPLICATE_MESSAGE: &str = "CamelDuplicateMessage";
/// The `CamelDocumentBuilderFactory` header or exchange property.
pub const DOCUMENT_BUILDER_FACTORY: &str = "CamelDocumentBuilderFactory";
/// The `CamelExceptionCaught` header or exchange property.
pub const EXCEPTION_CAUGHT: &str = "CamelExceptionCaught";
/// The `CamelExceptionHandled` header or exchange property.
pub const EXCEPTION_HANDLED: &str = "CamelExceptionHandled";
/// The `CamelEvaluateExpressionResult` header or exchange property.
pub const EVALUATE_EXPRESSION_RESULT: &str = "CamelEvaluateExpressionResult";
/// The `CamelErrorHandlerBridge` header or exchange property.
pub const ERRORHANDLER_BRIDGE: &str = "CamelErrorHandlerBridge";
/// The `CamelErrorHandlerCircuitDetected` header or exchange property.
pub const ERRORHANDLER_CIRCUIT_DETECTED: &str = "CamelErrorHandlerCircuitDetected";
/// The `CamelFailureHandled` header or exchange property.
pub const FAILURE_HANDLED: &str = "CamelFailureHandled";
/// The `CamelFailureEndpoint` header or exchange property.
pub const FAILURE_ENDPOINT: &str = "CamelFailureEndpoint";
/// The `CamelFailureRouteId` header or exchange property.
pub const FAILURE_ROUTE_ID: &str = "CamelFailureRouteId";
/// The `CamelFailureNodeId` header or exchange property.
pub const FAILURE_NODE_ID: &str = "CamelFailureNodeId";
/// The `CamelFailureLocation` header or exchange property.
pub const FAILURE_LOCATION: &str = "CamelFailureLocation";
/// The `CamelFatalFallbackErrorHandler` header or exchange property.
pub const FATAL_FALLBACK_ERROR_HANDLER: &str = "CamelFatalFallbackErrorHandler";
/// The `CamelFileContentType` header or exchange property.
pub const FILE_CONTENT_TYPE: &str = "CamelFileContentType";
/// The `CamelFileLocalWorkPath` header or exchange property.
pub const FILE_LOCAL_WORK_PATH: &str = "CamelFileLocalWorkPath";
/// The `CamelFileName` header or exchange property.
pub const FILE_NAME: &str = "CamelFileName";
/// The `CamelFileNameOnly` header or exchange property.
pub const FILE_NAME_ONLY: &str = "CamelFileNameOnly";
/// The checksum of the written file (`checksumFileAlgorithm`).
pub const FILE_CHECKSUM: &str = "CamelFileChecksum";
/// The `CamelFileNameProduced` header or exchange property.
pub const FILE_NAME_PRODUCED: &str = "CamelFileNameProduced";
/// The `CamelFileNameConsumed` header or exchange property.
pub const FILE_NAME_CONSUMED: &str = "CamelFileNameConsumed";
/// The `CamelFilePath` header or exchange property.
pub const FILE_PATH: &str = "CamelFilePath";
/// The `CamelFileParent` header or exchange property.
pub const FILE_PARENT: &str = "CamelFileParent";
/// The `CamelFileLastModified` header or exchange property.
pub const FILE_LAST_MODIFIED: &str = "CamelFileLastModified";
/// The `CamelFileLength` header or exchange property.
pub const FILE_LENGTH: &str = "CamelFileLength";
/// The `CamelFileLockFileAcquired` header or exchange property.
pub const FILE_LOCK_FILE_ACQUIRED: &str = "CamelFileLockFileAcquired";
/// The `CamelFileLockFileName` header or exchange property.
pub const FILE_LOCK_FILE_NAME: &str = "CamelFileLockFileName";
/// The `CamelFileLockIdempotentAcquired` header or exchange property.
pub const FILE_LOCK_IDEMPOTENT_ACQUIRED: &str = "CamelFileLockIdempotentAcquired";
/// The `CamelFileLockExclusiveLock` header or exchange property.
pub const FILE_LOCK_EXCLUSIVE_LOCK: &str = "CamelFileLockExclusiveLock";
/// The `CamelFileLockRandomAccessFile` header or exchange property.
pub const FILE_LOCK_RANDOM_ACCESS_FILE: &str = "CamelFileLockRandomAccessFile";
/// The `CamelFileLockChannelFile` header or exchange property.
pub const FILE_LOCK_CHANNEL_FILE: &str = "CamelFileLockChannelFile";
/// The `CamelFileExchangeFile` header or exchange property.
pub const FILE_EXCHANGE_FILE: &str = "CamelFileExchangeFile";
/// The `CamelFileIdempotentKey` header or exchange property.
pub const FILE_IDEMPOTENT_KEY: &str = "CamelFileIdempotentKey";
/// The `CamelFilterNonXmlChars` header or exchange property.
pub const FILTER_NON_XML_CHARS: &str = "CamelFilterNonXmlChars";
/// The `CamelGroupedExchange` header or exchange property.
pub const GROUPED_EXCHANGE: &str = "CamelGroupedExchange";
/// The `CamelHttpScheme` header or exchange property.
pub const HTTP_SCHEME: &str = "CamelHttpScheme";
/// The `CamelHttpHost` header or exchange property.
pub const HTTP_HOST: &str = "CamelHttpHost";
/// The `CamelHttpPort` header or exchange property.
pub const HTTP_PORT: &str = "CamelHttpPort";
/// The `CamelHttpBaseUri` header or exchange property.
pub const HTTP_BASE_URI: &str = "CamelHttpBaseUri";
/// The `CamelHttpCharacterEncoding` header or exchange property.
pub const HTTP_CHARACTER_ENCODING: &str = "CamelHttpCharacterEncoding";
/// The `CamelHttpMethod` header or exchange property.
pub const HTTP_METHOD: &str = "CamelHttpMethod";
/// The `CamelHttpPath` header or exchange property.
pub const HTTP_PATH: &str = "CamelHttpPath";
/// The `CamelHttpProtocolVersion` header or exchange property.
pub const HTTP_PROTOCOL_VERSION: &str = "CamelHttpProtocolVersion";
/// The `CamelHttpQuery` header or exchange property.
pub const HTTP_QUERY: &str = "CamelHttpQuery";
/// The `CamelHttpRawQuery` header or exchange property.
pub const HTTP_RAW_QUERY: &str = "CamelHttpRawQuery";
/// The `CamelHttpResponseCode` header or exchange property.
pub const HTTP_RESPONSE_CODE: &str = "CamelHttpResponseCode";
/// The `CamelHttpResponseText` header or exchange property.
pub const HTTP_RESPONSE_TEXT: &str = "CamelHttpResponseText";
/// The `CamelHttpUri` header or exchange property.
pub const HTTP_URI: &str = "CamelHttpUri";
/// The `CamelHttpUrl` header or exchange property.
pub const HTTP_URL: &str = "CamelHttpUrl";
/// The `CamelHttpChunked` header or exchange property.
pub const HTTP_CHUNKED: &str = "CamelHttpChunked";
/// The `CamelHttpServletRequest` header or exchange property.
pub const HTTP_SERVLET_REQUEST: &str = "CamelHttpServletRequest";
/// The `CamelHttpServletResponse` header or exchange property.
pub const HTTP_SERVLET_RESPONSE: &str = "CamelHttpServletResponse";
/// The `CamelInterceptedEndpoint` header or exchange property.
pub const INTERCEPTED_ENDPOINT: &str = "CamelInterceptedEndpoint";
/// The `CamelInterceptedNodeId` header or exchange property.
pub const INTERCEPTED_NODE_ID: &str = "CamelInterceptedNodeId";
/// The `CamelInterceptedParentEndpointUri` header or exchange property.
pub const INTERCEPTED_ROUTE_ENDPOINT_URI: &str = "CamelInterceptedParentEndpointUri";
/// The `CamelInterceptedRouteId` header or exchange property.
pub const INTERCEPTED_ROUTE_ID: &str = "CamelInterceptedRouteId";
/// The `CamelInterceptSendToEndpointWhenMatched` header or exchange property.
pub const INTERCEPT_SEND_TO_ENDPOINT_WHEN_MATCHED: &str = "CamelInterceptSendToEndpointWhenMatched";
/// The `CamelEntityManager` header or exchange property.
pub const JPA_ENTITY_MANAGER: &str = "CamelEntityManager";
/// The `CamelLanguageScript` header or exchange property.
pub const LANGUAGE_SCRIPT: &str = "CamelLanguageScript";
/// The `CamelLogDebugBodyMaxChars` header or exchange property.
pub const LOG_DEBUG_BODY_MAX_CHARS: &str = "CamelLogDebugBodyMaxChars";
/// The `CamelLogDebugStreams` header or exchange property.
pub const LOG_DEBUG_BODY_STREAMS: &str = "CamelLogDebugStreams";
/// The `CamelLogEipName` header or exchange property.
pub const LOG_EIP_NAME: &str = "CamelLogEipName";
/// The `CamelLogEipLanguage` header or exchange property.
pub const LOG_EIP_LANGUAGE: &str = "CamelLogEipLanguage";
/// The `CamelLoopIndex` header or exchange property.
pub const LOOP_INDEX: &str = "CamelLoopIndex";
/// The `CamelLoopSize` header or exchange property.
pub const LOOP_SIZE: &str = "CamelLoopSize";
/// The `CamelMaximumCachePoolSize` header or exchange property.
pub const MAXIMUM_CACHE_POOL_SIZE: &str = "CamelMaximumCachePoolSize";
/// The `CamelMaximumEndpointCacheSize` header or exchange property.
pub const MAXIMUM_ENDPOINT_CACHE_SIZE: &str = "CamelMaximumEndpointCacheSize";
/// The `CamelMaximumSimpleCacheSize` header or exchange property.
pub const MAXIMUM_SIMPLE_CACHE_SIZE: &str = "CamelMaximumSimpleCacheSize";
/// The `CamelMaximumTransformerCacheSize` header or exchange property.
pub const MAXIMUM_TRANSFORMER_CACHE_SIZE: &str = "CamelMaximumTransformerCacheSize";
/// The `CamelMaximumValidatorCacheSize` header or exchange property.
pub const MAXIMUM_VALIDATOR_CACHE_SIZE: &str = "CamelMaximumValidatorCacheSize";
/// The `CamelMessageHistory` header or exchange property.
pub const MESSAGE_HISTORY: &str = "CamelMessageHistory";
/// The `CamelMessageHistoryHeaderFormat` header or exchange property.
pub const MESSAGE_HISTORY_HEADER_FORMAT: &str = "CamelMessageHistoryHeaderFormat";
/// The `CamelMessageHistoryOutputFormat` header or exchange property.
pub const MESSAGE_HISTORY_OUTPUT_FORMAT: &str = "CamelMessageHistoryOutputFormat";
/// The `CamelMessageTimestamp` header or exchange property.
pub const MESSAGE_TIMESTAMP: &str = "CamelMessageTimestamp";
/// The `CamelMessageBodySize` header or exchange property.
pub const MESSAGE_BODY_SIZE: &str = "CamelMessageBodySize";
/// The `CamelMessageHeadersSize` header or exchange property.
pub const MESSAGE_HEADERS_SIZE: &str = "CamelMessageHeadersSize";
/// The `CamelMulticastIndex` header or exchange property.
pub const MULTICAST_INDEX: &str = "CamelMulticastIndex";
/// The `CamelMulticastComplete` header or exchange property.
pub const MULTICAST_COMPLETE: &str = "CamelMulticastComplete";
/// The `CamelOnCompletion` header or exchange property.
pub const ON_COMPLETION: &str = "CamelOnCompletion";
/// The `CamelOnCompletionRouteIds` header or exchange property.
pub const ON_COMPLETION_ROUTE_IDS: &str = "CamelOnCompletionRouteIds";
/// The `CamelOnCompletionFiredIds` header or exchange property.
pub const ON_COMPLETION_FIRED_IDS: &str = "CamelOnCompletionFiredIds";
/// The `CamelOnCompletionConfigRegisteredRoutes` header or exchange property.
pub const ON_COMPLETION_CONFIG_REGISTERED_ROUTES: &str = "CamelOnCompletionConfigRegisteredRoutes";
/// The `CamelOffset` header or exchange property.
pub const OFFSET: &str = "CamelOffset";
/// The `CamelOverruleFileName` header or exchange property.
pub const OVERRULE_FILE_NAME: &str = "CamelOverruleFileName";
/// The `CamelParentUnitOfWork` header or exchange property.
pub const PARENT_UNIT_OF_WORK: &str = "CamelParentUnitOfWork";
/// The `CamelStreamCacheUnitOfWork` header or exchange property.
pub const STREAM_CACHE_UNIT_OF_WORK: &str = "CamelStreamCacheUnitOfWork";
/// The `CamelRecipientListEndpoint` header or exchange property.
pub const RECIPIENT_LIST_ENDPOINT: &str = "CamelRecipientListEndpoint";
/// The `CamelReceivedTimestamp` header or exchange property.
pub const RECEIVED_TIMESTAMP: &str = "CamelReceivedTimestamp";
/// The `CamelRedelivered` header or exchange property.
pub const REDELIVERED: &str = "CamelRedelivered";
/// The `CamelRedeliveryCounter` header or exchange property.
pub const REDELIVERY_COUNTER: &str = "CamelRedeliveryCounter";
/// The `CamelRedeliveryMaxCounter` header or exchange property.
pub const REDELIVERY_MAX_COUNTER: &str = "CamelRedeliveryMaxCounter";
/// The `CamelRedeliveryDelay` header or exchange property.
pub const REDELIVERY_DELAY: &str = "CamelRedeliveryDelay";
/// The `CamelRestHttpUri` header or exchange property.
pub const REST_HTTP_URI: &str = "CamelRestHttpUri";
/// The `CamelRestHttpQuery` header or exchange property.
pub const REST_HTTP_QUERY: &str = "CamelRestHttpQuery";
/// The `CamelRestOpenAPI` header or exchange property.
pub const REST_OPENAPI: &str = "CamelRestOpenAPI";
/// The `Long-Running-Action` header or exchange property.
pub const SAGA_LONG_RUNNING_ACTION: &str = "Long-Running-Action";
/// The `CamelSchedulerPolledMessages` header or exchange property.
pub const SCHEDULER_POLLED_MESSAGES: &str = "CamelSchedulerPolledMessages";
/// The `CamelSkipGzipEncoding` header or exchange property.
pub const SKIP_GZIP_ENCODING: &str = "CamelSkipGzipEncoding";
/// The `CamelSkipOver` header or exchange property.
pub const SKIP_OVER: &str = "CamelSkipOver";
/// The `CamelSkipWwwFormUrlEncoding` header or exchange property.
pub const SKIP_WWW_FORM_URLENCODED: &str = "CamelSkipWwwFormUrlEncoding";
/// The `CamelSlipEndpoint` header or exchange property.
pub const SLIP_ENDPOINT: &str = "CamelSlipEndpoint";
/// The `CamelSlipProducer` header or exchange property.
pub const SLIP_PRODUCER: &str = "CamelSlipProducer";
/// The `CamelSplitIndex` header or exchange property.
pub const SPLIT_INDEX: &str = "CamelSplitIndex";
/// The `CamelSplitComplete` header or exchange property.
pub const SPLIT_COMPLETE: &str = "CamelSplitComplete";
/// The `CamelSplitSize` header or exchange property.
pub const SPLIT_SIZE: &str = "CamelSplitSize";
/// The `CamelSplitResult` header or exchange property.
pub const SPLIT_RESULT: &str = "CamelSplitResult";
/// The `CamelSplitWatermark` header or exchange property.
pub const SPLIT_WATERMARK: &str = "CamelSplitWatermark";
/// The `CamelStepId` header or exchange property.
pub const STEP_ID: &str = "CamelStepId";
/// The `CamelTimerCounter` header or exchange property.
pub const TIMER_COUNTER: &str = "CamelTimerCounter";
/// The `CamelTimerFiredTime` header or exchange property.
pub const TIMER_FIRED_TIME: &str = "CamelTimerFiredTime";
/// The `CamelTimerName` header or exchange property.
pub const TIMER_NAME: &str = "CamelTimerName";
/// The `CamelTimerPeriod` header or exchange property.
pub const TIMER_PERIOD: &str = "CamelTimerPeriod";
/// The `CamelTimerTime` header or exchange property.
pub const TIMER_TIME: &str = "CamelTimerTime";
/// The `CamelToEndpoint` header or exchange property.
pub const TO_ENDPOINT: &str = "CamelToEndpoint";
/// The `CamelTraceEvent` header or exchange property.
pub const TRACE_EVENT: &str = "CamelTraceEvent";
/// The `CamelTraceEventNodeId` header or exchange property.
pub const TRACE_EVENT_NODE_ID: &str = "CamelTraceEventNodeId";
/// The `CamelTraceEventTimestamp` header or exchange property.
pub const TRACE_EVENT_TIMESTAMP: &str = "CamelTraceEventTimestamp";
/// The `CamelTraceEventExchange` header or exchange property.
pub const TRACE_EVENT_EXCHANGE: &str = "CamelTraceEventExchange";
/// The `CamelTransactionContextData` header or exchange property.
pub const TRANSACTION_CONTEXT_DATA: &str = "CamelTransactionContextData";
/// The `TryRouteBlock` header or exchange property.
pub const TRY_ROUTE_BLOCK: &str = "TryRouteBlock";
/// The `Transfer-Encoding` header or exchange property.
pub const TRANSFER_ENCODING: &str = "Transfer-Encoding";
/// The `CamelUnitOfWorkExhausted` header or exchange property.
pub const UNIT_OF_WORK_EXHAUSTED: &str = "CamelUnitOfWorkExhausted";
/// The `CamelXsltFileName` header or exchange property.
pub const XSLT_FILE_NAME: &str = "CamelXsltFileName";
/// The `CamelXsltError` header or exchange property.
pub const XSLT_ERROR: &str = "CamelXsltError";
/// The `CamelXsltFatalError` header or exchange property.
pub const XSLT_FATAL_ERROR: &str = "CamelXsltFatalError";
/// The `CamelXsltWarning` header or exchange property.
pub const XSLT_WARNING: &str = "CamelXsltWarning";
/// The `OpenTracing.activeSpan` header or exchange property.
pub const OTEL_ACTIVE_SPAN: &str = "OpenTracing.activeSpan";
/// The `OpenTracing.closeClientScope` header or exchange property.
pub const OTEL_CLOSE_CLIENT_SCOPE: &str = "OpenTracing.closeClientScope";
/// The `CamelActivitySpanTags` header or exchange property.
pub const ACTIVITY_SPAN_TAGS: &str = "CamelActivitySpanTags";

/// Typed enum of the well-known exchange property keys used by camel-core (Java
/// `ExchangePropertyKey`).
///
/// Each constant wraps the corresponding string constant and provides a type-safe alternative
/// for reading and writing internal exchange properties.
///
/// *Since Camel 3.9*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExchangePropertyKey {
    /// The `CamelAggregatedCompletedBy` property.
    AggregatedCompletedBy,
    /// The `CamelAggregatedCorrelationKey` property.
    AggregatedCorrelationKey,
    /// The `CamelAggregatedSize` property.
    AggregatedSize,
    /// The `CamelAggregatedTimeout` property.
    AggregatedTimeout,
    /// The `CamelAggregationCompleteAllGroups` property.
    AggregationCompleteAllGroups,
    /// The `CamelAggregationCompleteCurrentGroup` property.
    AggregationCompleteCurrentGroup,
    /// The `CamelAggregationStrategy` property.
    AggregationStrategy,
    /// The `CamelAttachmentsSize` property.
    AttachmentsSize,
    /// The `CamelBatchComplete` property.
    BatchComplete,
    /// The `CamelBatchIndex` property.
    BatchIndex,
    /// The `CamelBatchSize` property.
    BatchSize,
    /// The `CamelCharsetName` property.
    CharsetName,
    /// The `CamelCircuitBreakerSuccessfulExecution` property.
    CircuitBreakerResponseSuccessfulExecution,
    /// The `CamelCircuitBreakerResponseFromFallback` property.
    CircuitBreakerResponseFromFallback,
    /// The `CamelCircuitBreakerResponseShortCircuited` property.
    CircuitBreakerResponseShortCircuited,
    /// The `CamelCircuitBreakerResponseTimedOut` property.
    CircuitBreakerResponseTimedOut,
    /// The `CamelCircuitBreakerResponseRejected` property.
    CircuitBreakerResponseRejected,
    /// The `CamelCircuitBreakerResponseIgnored` property.
    CircuitBreakerResponseIgnored,
    /// The `CamelCircuitBreakerState` property.
    CircuitBreakerResponseState,
    /// The `CamelClaimCheckRepository` property.
    ClaimCheckRepository,
    /// The `CamelCorrelationId` property.
    CorrelationId,
    /// The `CamelDebuggerSelfTime` property.
    DebuggerSelfTime,
    /// The `CamelDuplicateMessage` property.
    DuplicateMessage,
    /// The `CamelErrorHandlerBridge` property.
    ErrorhandlerBridge,
    /// The `CamelErrorHandlerCircuitDetected` property.
    ErrorhandlerCircuitDetected,
    /// The `CamelEvaluateExpressionResult` property.
    EvaluateExpressionResult,
    /// The `CamelExceptionCaught` property.
    ExceptionCaught,
    /// The `CamelExceptionHandled` property.
    ExceptionHandled,
    /// The `CamelFailureEndpoint` property.
    FailureEndpoint,
    /// The `CamelFailureHandled` property.
    FailureHandled,
    /// The `CamelFailureRouteId` property.
    FailureRouteId,
    /// The `CamelFailureNodeId` property.
    FailureNodeId,
    /// The `CamelFailureLocation` property.
    FailureLocation,
    /// The `CamelFatalFallbackErrorHandler` property.
    FatalFallbackErrorHandler,
    /// The `CamelFileExchangeFile` property.
    FileExchangeFile,
    /// The `CamelGroupedExchange` property.
    GroupedExchange,
    /// The `CamelInterceptedEndpoint` property.
    InterceptedEndpoint,
    /// The `CamelInterceptedNodeId` property.
    InterceptedNodeId,
    /// The `CamelInterceptedParentEndpointUri` property.
    InterceptedRouteEndpointUri,
    /// The `CamelInterceptedRouteId` property.
    InterceptedRouteId,
    /// The `CamelInterceptSendToEndpointWhenMatched` property.
    InterceptSendToEndpointWhenMatched,
    /// The `CamelLoopIndex` property.
    LoopIndex,
    /// The `CamelLoopSize` property.
    LoopSize,
    /// The `CamelMessageBodySize` property.
    MessageBodySize,
    /// The `CamelMessageHeadersSize` property.
    MessageHeadersSize,
    /// The `CamelMessageHistory` property.
    MessageHistory,
    /// The `CamelMulticastComplete` property.
    MulticastComplete,
    /// The `CamelMulticastIndex` property.
    MulticastIndex,
    /// The `CamelOnCompletion` property.
    OnCompletion,
    /// The `CamelOnCompletionRouteIds` property.
    OnCompletionRouteIds,
    /// The `CamelOnCompletionFiredIds` property.
    OnCompletionFiredIds,
    /// The `CamelOnCompletionConfigRegisteredRoutes` property.
    OnCompletionConfigRegisteredRoutes,
    /// The `CamelParentUnitOfWork` property.
    ParentUnitOfWork,
    /// The `CamelReceivedTimestamp` property.
    ReceivedTimestamp,
    /// The `CamelRecipientListEndpoint` property.
    RecipientListEndpoint,
    /// The `CamelSkipOver` property.
    SkipOver,
    /// The `CamelSlipEndpoint` property.
    SlipEndpoint,
    /// The `CamelSlipProducer` property.
    SlipProducer,
    /// The `CamelSplitComplete` property.
    SplitComplete,
    /// The `CamelSplitIndex` property.
    SplitIndex,
    /// The `CamelSplitResult` property.
    SplitResult,
    /// The `CamelSplitSize` property.
    SplitSize,
    /// The `CamelSplitWatermark` property.
    SplitWatermark,
    /// The `CamelStepId` property.
    StepId,
    /// The `CamelStreamCacheUnitOfWork` property.
    StreamCacheUnitOfWork,
    /// The `CamelToEndpoint` property.
    ToEndpoint,
    /// The `TryRouteBlock` property.
    TryRouteBlock,
    /// The `CamelUnitOfWorkExhausted` property.
    UnitOfWorkExhausted,
    /// The `OpenTracing.activeSpan` property.
    OtelActiveSpan,
    /// The `OpenTracing.closeClientScope` property.
    OtelCloseClientScope,
    /// The `CamelActivitySpanTags` property.
    ActivitySpanTags,
}

impl ExchangePropertyKey {
    /// All the keys, in declaration order.
    pub const VALUES: &'static [ExchangePropertyKey] = &[
        ExchangePropertyKey::AggregatedCompletedBy,
        ExchangePropertyKey::AggregatedCorrelationKey,
        ExchangePropertyKey::AggregatedSize,
        ExchangePropertyKey::AggregatedTimeout,
        ExchangePropertyKey::AggregationCompleteAllGroups,
        ExchangePropertyKey::AggregationCompleteCurrentGroup,
        ExchangePropertyKey::AggregationStrategy,
        ExchangePropertyKey::AttachmentsSize,
        ExchangePropertyKey::BatchComplete,
        ExchangePropertyKey::BatchIndex,
        ExchangePropertyKey::BatchSize,
        ExchangePropertyKey::CharsetName,
        ExchangePropertyKey::CircuitBreakerResponseSuccessfulExecution,
        ExchangePropertyKey::CircuitBreakerResponseFromFallback,
        ExchangePropertyKey::CircuitBreakerResponseShortCircuited,
        ExchangePropertyKey::CircuitBreakerResponseTimedOut,
        ExchangePropertyKey::CircuitBreakerResponseRejected,
        ExchangePropertyKey::CircuitBreakerResponseIgnored,
        ExchangePropertyKey::CircuitBreakerResponseState,
        ExchangePropertyKey::ClaimCheckRepository,
        ExchangePropertyKey::CorrelationId,
        ExchangePropertyKey::DebuggerSelfTime,
        ExchangePropertyKey::DuplicateMessage,
        ExchangePropertyKey::ErrorhandlerBridge,
        ExchangePropertyKey::ErrorhandlerCircuitDetected,
        ExchangePropertyKey::EvaluateExpressionResult,
        ExchangePropertyKey::ExceptionCaught,
        ExchangePropertyKey::ExceptionHandled,
        ExchangePropertyKey::FailureEndpoint,
        ExchangePropertyKey::FailureHandled,
        ExchangePropertyKey::FailureRouteId,
        ExchangePropertyKey::FailureNodeId,
        ExchangePropertyKey::FailureLocation,
        ExchangePropertyKey::FatalFallbackErrorHandler,
        ExchangePropertyKey::FileExchangeFile,
        ExchangePropertyKey::GroupedExchange,
        ExchangePropertyKey::InterceptedEndpoint,
        ExchangePropertyKey::InterceptedNodeId,
        ExchangePropertyKey::InterceptedRouteEndpointUri,
        ExchangePropertyKey::InterceptedRouteId,
        ExchangePropertyKey::InterceptSendToEndpointWhenMatched,
        ExchangePropertyKey::LoopIndex,
        ExchangePropertyKey::LoopSize,
        ExchangePropertyKey::MessageBodySize,
        ExchangePropertyKey::MessageHeadersSize,
        ExchangePropertyKey::MessageHistory,
        ExchangePropertyKey::MulticastComplete,
        ExchangePropertyKey::MulticastIndex,
        ExchangePropertyKey::OnCompletion,
        ExchangePropertyKey::OnCompletionRouteIds,
        ExchangePropertyKey::OnCompletionFiredIds,
        ExchangePropertyKey::OnCompletionConfigRegisteredRoutes,
        ExchangePropertyKey::ParentUnitOfWork,
        ExchangePropertyKey::ReceivedTimestamp,
        ExchangePropertyKey::RecipientListEndpoint,
        ExchangePropertyKey::SkipOver,
        ExchangePropertyKey::SlipEndpoint,
        ExchangePropertyKey::SlipProducer,
        ExchangePropertyKey::SplitComplete,
        ExchangePropertyKey::SplitIndex,
        ExchangePropertyKey::SplitResult,
        ExchangePropertyKey::SplitSize,
        ExchangePropertyKey::SplitWatermark,
        ExchangePropertyKey::StepId,
        ExchangePropertyKey::StreamCacheUnitOfWork,
        ExchangePropertyKey::ToEndpoint,
        ExchangePropertyKey::TryRouteBlock,
        ExchangePropertyKey::UnitOfWorkExhausted,
        ExchangePropertyKey::OtelActiveSpan,
        ExchangePropertyKey::OtelCloseClientScope,
        ExchangePropertyKey::ActivitySpanTags,
    ];

    /// The name of the property.
    pub fn name(&self) -> &'static str {
        match self {
            ExchangePropertyKey::AggregatedCompletedBy => "CamelAggregatedCompletedBy",
            ExchangePropertyKey::AggregatedCorrelationKey => "CamelAggregatedCorrelationKey",
            ExchangePropertyKey::AggregatedSize => "CamelAggregatedSize",
            ExchangePropertyKey::AggregatedTimeout => "CamelAggregatedTimeout",
            ExchangePropertyKey::AggregationCompleteAllGroups => {
                "CamelAggregationCompleteAllGroups"
            }
            ExchangePropertyKey::AggregationCompleteCurrentGroup => {
                "CamelAggregationCompleteCurrentGroup"
            }
            ExchangePropertyKey::AggregationStrategy => "CamelAggregationStrategy",
            ExchangePropertyKey::AttachmentsSize => "CamelAttachmentsSize",
            ExchangePropertyKey::BatchComplete => "CamelBatchComplete",
            ExchangePropertyKey::BatchIndex => "CamelBatchIndex",
            ExchangePropertyKey::BatchSize => "CamelBatchSize",
            ExchangePropertyKey::CharsetName => "CamelCharsetName",
            ExchangePropertyKey::CircuitBreakerResponseSuccessfulExecution => {
                "CamelCircuitBreakerSuccessfulExecution"
            }
            ExchangePropertyKey::CircuitBreakerResponseFromFallback => {
                "CamelCircuitBreakerResponseFromFallback"
            }
            ExchangePropertyKey::CircuitBreakerResponseShortCircuited => {
                "CamelCircuitBreakerResponseShortCircuited"
            }
            ExchangePropertyKey::CircuitBreakerResponseTimedOut => {
                "CamelCircuitBreakerResponseTimedOut"
            }
            ExchangePropertyKey::CircuitBreakerResponseRejected => {
                "CamelCircuitBreakerResponseRejected"
            }
            ExchangePropertyKey::CircuitBreakerResponseIgnored => {
                "CamelCircuitBreakerResponseIgnored"
            }
            ExchangePropertyKey::CircuitBreakerResponseState => "CamelCircuitBreakerState",
            ExchangePropertyKey::ClaimCheckRepository => "CamelClaimCheckRepository",
            ExchangePropertyKey::CorrelationId => "CamelCorrelationId",
            ExchangePropertyKey::DebuggerSelfTime => "CamelDebuggerSelfTime",
            ExchangePropertyKey::DuplicateMessage => "CamelDuplicateMessage",
            ExchangePropertyKey::ErrorhandlerBridge => "CamelErrorHandlerBridge",
            ExchangePropertyKey::ErrorhandlerCircuitDetected => "CamelErrorHandlerCircuitDetected",
            ExchangePropertyKey::EvaluateExpressionResult => "CamelEvaluateExpressionResult",
            ExchangePropertyKey::ExceptionCaught => "CamelExceptionCaught",
            ExchangePropertyKey::ExceptionHandled => "CamelExceptionHandled",
            ExchangePropertyKey::FailureEndpoint => "CamelFailureEndpoint",
            ExchangePropertyKey::FailureHandled => "CamelFailureHandled",
            ExchangePropertyKey::FailureRouteId => "CamelFailureRouteId",
            ExchangePropertyKey::FailureNodeId => "CamelFailureNodeId",
            ExchangePropertyKey::FailureLocation => "CamelFailureLocation",
            ExchangePropertyKey::FatalFallbackErrorHandler => "CamelFatalFallbackErrorHandler",
            ExchangePropertyKey::FileExchangeFile => "CamelFileExchangeFile",
            ExchangePropertyKey::GroupedExchange => "CamelGroupedExchange",
            ExchangePropertyKey::InterceptedEndpoint => "CamelInterceptedEndpoint",
            ExchangePropertyKey::InterceptedNodeId => "CamelInterceptedNodeId",
            ExchangePropertyKey::InterceptedRouteEndpointUri => "CamelInterceptedParentEndpointUri",
            ExchangePropertyKey::InterceptedRouteId => "CamelInterceptedRouteId",
            ExchangePropertyKey::InterceptSendToEndpointWhenMatched => {
                "CamelInterceptSendToEndpointWhenMatched"
            }
            ExchangePropertyKey::LoopIndex => "CamelLoopIndex",
            ExchangePropertyKey::LoopSize => "CamelLoopSize",
            ExchangePropertyKey::MessageBodySize => "CamelMessageBodySize",
            ExchangePropertyKey::MessageHeadersSize => "CamelMessageHeadersSize",
            ExchangePropertyKey::MessageHistory => "CamelMessageHistory",
            ExchangePropertyKey::MulticastComplete => "CamelMulticastComplete",
            ExchangePropertyKey::MulticastIndex => "CamelMulticastIndex",
            ExchangePropertyKey::OnCompletion => "CamelOnCompletion",
            ExchangePropertyKey::OnCompletionRouteIds => "CamelOnCompletionRouteIds",
            ExchangePropertyKey::OnCompletionFiredIds => "CamelOnCompletionFiredIds",
            ExchangePropertyKey::OnCompletionConfigRegisteredRoutes => {
                "CamelOnCompletionConfigRegisteredRoutes"
            }
            ExchangePropertyKey::ParentUnitOfWork => "CamelParentUnitOfWork",
            ExchangePropertyKey::ReceivedTimestamp => "CamelReceivedTimestamp",
            ExchangePropertyKey::RecipientListEndpoint => "CamelRecipientListEndpoint",
            ExchangePropertyKey::SkipOver => "CamelSkipOver",
            ExchangePropertyKey::SlipEndpoint => "CamelSlipEndpoint",
            ExchangePropertyKey::SlipProducer => "CamelSlipProducer",
            ExchangePropertyKey::SplitComplete => "CamelSplitComplete",
            ExchangePropertyKey::SplitIndex => "CamelSplitIndex",
            ExchangePropertyKey::SplitResult => "CamelSplitResult",
            ExchangePropertyKey::SplitSize => "CamelSplitSize",
            ExchangePropertyKey::SplitWatermark => "CamelSplitWatermark",
            ExchangePropertyKey::StepId => "CamelStepId",
            ExchangePropertyKey::StreamCacheUnitOfWork => "CamelStreamCacheUnitOfWork",
            ExchangePropertyKey::ToEndpoint => "CamelToEndpoint",
            ExchangePropertyKey::TryRouteBlock => "TryRouteBlock",
            ExchangePropertyKey::UnitOfWorkExhausted => "CamelUnitOfWorkExhausted",
            ExchangePropertyKey::OtelActiveSpan => "OpenTracing.activeSpan",
            ExchangePropertyKey::OtelCloseClientScope => "OpenTracing.closeClientScope",
            ExchangePropertyKey::ActivitySpanTags => "CamelActivitySpanTags",
        }
    }

    /// The key for the property name, or `None` if it is not a well-known key (Java
    /// `asExchangePropertyKey`).
    pub fn as_exchange_property_key(name: &str) -> Option<ExchangePropertyKey> {
        Self::VALUES.iter().copied().find(|k| k.name() == name)
    }
}

impl std::fmt::Display for ExchangePropertyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!("CamelFileName", FILE_NAME);
        assert_eq!(
            "CamelExceptionCaught",
            ExchangePropertyKey::ExceptionCaught.name()
        );
        assert_eq!(
            Some(ExchangePropertyKey::CharsetName),
            ExchangePropertyKey::as_exchange_property_key(CHARSET_NAME)
        );
        assert_eq!(None, ExchangePropertyKey::as_exchange_property_key("foo"));
        for k in ExchangePropertyKey::VALUES {
            assert_eq!(
                Some(*k),
                ExchangePropertyKey::as_exchange_property_key(k.name())
            );
        }
    }
}
