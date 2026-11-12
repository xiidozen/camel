# Apache Camel → Rust port backlog

> Continuing this work in a new session? Start with [`RUST_PORT_HANDOFF.md`](RUST_PORT_HANDOFF.md).

This file tracks the conversion of the Apache Camel code base from Java to Rust on the
`rust` branch. It is the single source of truth for what has been ported, what is in
progress and what is still to do. Update it in the same commit as each port.

## Ground rules

- **Route languages:** routes are written in **Rust** (a Rust route DSL), **YAML** or **XML**. The Java DSL is dropped on purpose (owner decision): Java-DSL-only modules are `n/a`, and docs/examples showing Java routes are rewritten with Rust routes.
- **Order:** bottom-up by dependency — `core/camel-util` → `core/camel-api` →
  `core/camel-support` → engine/processor/model/languages → `core/camel-core` tests →
  DSLs → components → test-infra → tooling/catalog/docs.
- **In-place replacement:** each Rust crate lives in the directory of the Maven module it
  replaces (`core/camel-util/Cargo.toml`, `src/lib.rs`, `tests/`). When a module is ported its
  Java sources, tests and `pom.xml` are deleted in the same commit.
- **Tests:** every Java unit test in a ported module is translated to a Rust test (unit tests
  in `#[cfg(test)]` modules or integration tests under `tests/`). A test that cannot apply to
  Rust (e.g. it exercises JVM class loading or Java serialization) is listed under
  *Not applicable* in the module's notes with the reason.
- **Green before push:** `cargo build --workspace` and `cargo test --workspace` must pass before
  every push.
- **Documentation (javadoc → rustdoc):** every public item carries the content of its javadoc,
  so `cargo doc` produces equivalent API docs. Mapping:
  - summary sentence and description paragraphs → first lines of the `///` block
  - `@param x desc` → `# Arguments` section with `* `x` - desc`
  - `@return desc` → `# Returns` section
  - `@throws X when` → `# Errors` (for `Result`) or `# Panics`
  - `<pre>` / `{@code}` examples → `# Examples` with a compiling doctest where practical
  - `{@link Foo}` → intra-doc link `[`Foo`]`; `@see` → "See also" line with links
  - `@deprecated` → `#[deprecated(note = "...")]`; `@since X.Y` → `*Since Camel X.Y*` line
  - package-info / class javadoc → `//!` module docs
  Each crate sets `#![deny(missing_docs)]` and `#![deny(rustdoc::broken_intra_doc_links)]`, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` must pass before every push.
- **Maven build:** because ported Java modules are removed, the remaining Java/Maven build on this
  branch is no longer expected to compile. Only the Cargo workspace is built and tested.

## Status legend

`todo` · `wip` · `done` · `n/a` (no Rust equivalent; replaced by a redesign or dropped — see notes)

## JVM-specific areas (need a redesign, not a translation)

| Java concept | Rust approach |
| --- | --- |
| Reflection / bean binding / `@Handler` dispatch | Explicit trait-based processors and closures registered by name |
| Class loading, `ObjectHelper.loadClass`, service discovery via `META-INF/services` | Compile-time registration (component/language factories registered in a registry) |
| Java serialization (`ObjectInputStream`) | Not ported; serde-based formats instead |
| `java.util.concurrent` executors | `camel_util::concurrent` (std threads); `tokio` where async I/O is needed |
| Java DSL (`RouteBuilder`) | **Rust route DSL** (builder with the same EIP names in snake_case). Routes are written in **Rust, YAML or XML**; the Java DSL is intentionally not kept (owner decision) |
| Groovy / jOOR / MVEL / OGNL languages | `n/a` unless a Rust interpreter exists |
| `java.lang.Object` (bodies, headers, `Map<String, Object>`) | `camel_util::value::Value` enum (Null/Bool/Int/Float/Text/Bytes/List/Map) + `Value::Object` holding any `Send + Sync` Rust value, downcastable |
| JVM system properties (`System.getProperty`) | `camel_util::system_properties` process-wide registry with environment variable fallback |
| `java.net.URI`, `URLEncoder`, `URLDecoder` | `camel_util::net` (exact JDK parsing semantics) |
| `java.time.Duration` (signed) | `camel_util::time_utils::SignedDuration` |
| `null` return values | `Option<T>`; `IllegalArgumentException` and friends become `Result<T, E>` |
| Components wrapping Java-only SDKs | Port onto the closest maintained Rust crate, or mark `n/a` with reason |

## Modules

| Module | Java main files | Java test files | Status | Notes |
| --- | ---: | ---: | --- | --- |
| `core/camel-api` | 546 | 0 | wip | Done: enums, all exceptions (`error::CamelError` + `ErrorKind` keeping the Java hierarchy and messages), `Exchange` constants + `ExchangePropertyKey`, `Message` and `Exchange` (concrete structs), `convert::FromValue`, async `Processor` (replaces AsyncProcessor/AsyncCallback), `Expression`, `Predicate`, `AggregationStrategy` (+ closure adapters), minimal `CamelContext` trait, `spi::UuidGenerator` (+ DefaultUuidGenerator), async `Service` lifecycle (+ `ServiceState` state machine), `Endpoint`/`Component`/`Producer`/`Consumer`/`PollingConsumer`, `Route`, `spi::{Registry, Language, Synchronization (+ SynchronizationVetoable, SynchronizationRouteAware), HeaderFilterStrategy}`, the unit of work state on `Exchange` (original message, on completions, handover). camel-api is ported in vertical slices, driven by what the engine needs. Next: TypeConverter, Registry, rest of spi. `spi::event` (`CamelEvent`, `EventType`, `EventNotifier`, `EventNotifierSupport` with the ignore options, `notify`; the exchange events are sent by the unit of work (created, completed, failed), `to`, the dynamic sends and the producer template (sending, sent); the error handler sends redelivery and failure handling/handled events, doCatch the failure handling/handled ones; pending: timestamps, the service events). `message_history` (`MessageHistory` entries in the `CamelMessageHistory` property, `MessageHistoryFactory` plugin: node pattern, copy message, body size, node ids). |
| `core/camel-base` | 100 | 0 | wip | Rust crate `camel-base`: the properties component (`properties::DefaultPropertiesComponent`, Java `PropertiesComponent` + `DefaultPropertiesParser` + `DefaultPropertiesLookup`: `{{key}}`, `{{key:default}}`, `{{?key}}`, `{{!key}}`, nested placeholders, `?nested=`, escapes, circular references, ignore missing property/location, env/sys modes, initial/override properties, `PropertiesSource`s, locations `classpath:` (files under the classpath roots), `file:`, `ref:`; functions `env`, `sys`, `service`, `service.host`, `service.port`, `boolean`). The SPI is camel-api `spi::properties`, resolved by `CamelContext::resolve_property_placeholders`; `DefaultCamelContext::properties()`. Placeholders are resolved in endpoint uris (camel-support `endpoint_helper`, unresolved optional parameters removed), language expression texts and route ids. Pending: local properties (route templates), lookup listeners, resolved values, reload, sensitive masking in logs, the rest of camel-base (type converters registry, ...). Tests: 15 Java tests of `component/properties` ported (`tests/component/properties`). |
| `core/camel-base-engine` | 112 | 0 | wip | `DefaultCamelContext` (components by name / registry / registered factory, endpoints by normalized uri, routes with startup order, start/stop/suspend/resume, rollback on start failure), `DefaultRoute`, `ProducerTemplate` (send/request body, `CamelExecutionException` wrapping). Runs on tokio. Pending: error handler, unit of work, route controller/supervising, shutdown strategy, type converter registry, properties component, management. Events: `add_event_notifier`, context `suspend`/`resume` (the routes, with the context suspending/suspended/resuming/resumed events), the context/route events (initializing, starting, routes starting, route added/starting/started, ... stopped; RouteRemoved when the context stops as Java removes the routes then). `ConsumerTemplate` (`create_consumer_template`: receive, receive_timeout, receive_no_wait, receive_body*, done_uow). |
| `core/camel-cluster` | 6 | 0 | todo | |
| `core/camel-console` | 57 | 69 | todo | |
| `core/camel-core` | 0 | 3179 | wip | Rust crate `camel-core`: facade (`default_context()` with the core components, `prelude`). Tests ported into `tests/<java package>/main.rs` (one module per Java test class) with `tests/support` (Java `ContextTestSupport`). Ported: `processor.FilterTest`, `processor.PipelineTest`, `processor.ChoiceNoOtherwiseTest`, `processor.TryProcessorTest`, `processor.MulticastTest`, `processor.SplitterTest` (Java kept: parallel/streaming cases), `processor.DelayerTest` (Java kept: bean cases), `processor.ToDynamicTest`, `processor.ToDynamicEmptyTest`, `processor.RecipientListTest`, `processor.RecipientListWithDelimiterTest` (Java kept: tokenize case), `processor.RecipientListWithStringDelimitedHeaderTest`, `processor.RecipientListIgnoreInvalidEndpointsTest`, `processor.RecipientListStopOnExceptionTest`, `processor.WireTapTest`, `processor.WireTapLogTest`, `processor.WireTapExpressionTest`, `processor.WireTapPatternTest`, `processor.WireTapIgnoreInvalidEndpointTest`, `processor.converter.ConvertBodyTest` (Java kept: charset and custom converter cases), `processor.aggregator.{AggregatorTest, AggregateSimpleSizeTest, AggregateSimplePredicateTest, AggregateSimpleTimeoutTest, AggregateCompletionIntervalTest, AggregateExpressionSizeTest, AggregateGroupedExchangeSizeTest, AggregateIgnoreInvalidCorrelationKeysTest, AggregateClosedCorrelationKeyTest, AggregateForceCompletionOnStopTest, AggregateCompleteAllOnStopTest}`, `processor.TryProcessorMultipleExceptionTest`, `processor.SimpleTryCatchFinallyTest`, `processor.DeadLetterChannelRedeliveryTest`, `processor.DefaultErrorHandlerRedeliveryTest`, `processor.onexception.OnExceptionContinueTest`, `processor.onexception.OnExceptionHandledTest`, `component.log.LogEndpointTest`, `component.seda.{SedaRouteTest, SedaInOnlyTest, SedaInOutTest, SedaInOutChainedTest, SedaInOutWithErrorTest, SedaDiscardWhenFullTest, SameSedaQueueSizeAndNoSizeTest, SedaDiscardIfNoConsumerTest (Java kept: its unit of work case)}` (Java kept: `testLogEndpoint` uses a Java `CamelLogProcessor` subclass), `language.simple.SimpleParserExpressionTest`, `language.simple.SimpleParserPredicateTest` (but `testSimpleIn`), `component.direct.*` (in camel-direct), `component.mock.MockEndpointTest` (in camel-mock)., `impl.event.{EventNotifierExchangeCompletedTest, EventNotifierExchangeSentTest, EventNotifierIgnoreCamelContextInitEventsTest, EventNotifierFailureHandledEventsTest, EventNotifierRedeliveryEventsTest, EventNotifierEventsTest, SimpleEventNotifierEventsTest, MultipleEventNotifierEventsTest, EventNotifierExchangeSentParallelTest}` (`tests/event`)., `processor.{ThreadsDefaultTest, ThreadsCorePoolTest, ThreadsCoreAndMaxPoolTest, ThreadsCoreAndMaxPoolKeepAliveTimeTest, ThreadsMaxQueueSizeTest, ThreadsZeroMaxQueueSizeTest, ThreadsRejectedPolicyTest, ThreadsCoreAndMaxPoolInvalidTest, ThreadsZeroInCoreAndMaxPoolTest, ThreadsRequestReplyTest, Camel715ThreadProcessorTest}` and the `ThreadsRejectedExecutionTest` cases (Java kept: the custom executor and caller-not-runs cases need the early release of the consumer) (`tests/processor/threads_test.rs`)., `language.simple.{SimpleInitBlockTest, SimpleInitBlockEdgeCasesTest, SimpleInitBlockOverlappingKeysTest, SimpleInitBlockFunctionTest, SimpleIncHeaderTest, SimpleDecHeaderTest, SimpleParserRegexpPredicateTest, SimpleWhiteSpaceTest}`., `language.simple.{SimpleCustomFunctionTest, SimpleResourceTest, SimpleResourceWithoutSchemeTest, SimpleResourceDynamicTest}`., `language.simple.{SimpleNestedTest, SimplePrettyTest}`., `language.simple.{SimpleBackwardsCompatibleTest, SimpleSetHeaderPredicateTest, SimpleRouteExpressionAsPredicateTest, BodyOriginalMessageTest}` and the `SimpleNotOperatorTest` cases (Java kept: its cases on the Java AST nodes)., `language.simple.{SimpleParserExpressionInvalidTest, SimpleParserPredicateInvalidTest, SimplePredicateParserLogicalTest}`., `processor.{MessageHistoryCopyExchangeTest, MessageHistoryStepOnlyTest, MessageHistoryCopyMessageTest, MessageHistoryBodyTypeAndSizeTest, MessageHistoryDumpRoutingTest, DefaultErrorHandlerLogExhaustedMessageHistoryTest, DeadLetterChannelLogExhaustedMessageHistoryTest, DeadLetterChannelLogExhaustedMessageHistoryWithBodyTest}`, `language.simple.{SimpleMessageHistoryTest, SimpleMessageHistoryNotDetailedTest}` (`tests/processor/message_history_test.rs`), `language.simple.SimpleFunctionArgumentsTest` and `SimpleCacheExpressionTest` cases (Java kept: the Java bean cases)., `processor.{SplitterMapTest, SplitterPropertyContinuedTest, SplitterCompletedPropertyTest, SplitterCollateTest, SplitWithDelimiterTest, SplitterSkipTest, SplitterStopOnExceptionTest, SplitterNoStopOnExceptionTest, SplitterThrowExceptionFromExpressionTest, SplitterOnPrepareTest, SplitterOnPrepareExceptionTest, SplitterNoAggregationStrategyTest, SplitterWithMulticastTest}` (`tests/processor/splitter_misc_test.rs`)., `language.TokenizerTest`, `processor.{SplitTokenizerGroupTest, SplitTokenizerGroupDynamicTest, SplitGroupSkipFirstTest, SplitGroupMultiLinesTest, SplitTokenizerRegexpGroupTest}` and the `SplitTokenizerTest` cases (Java kept: its xpath case)., `processor.{RecipientListInvalidEndpointErrorHandlerTest, RecipientListInvalidEndpointIgnoreInvalidEndpointsTest, RecipientListAggregationStrategyTest, RecipientListFunctionalTest, RecipientListStopOnExceptionWithOnExceptionTest, RecipientListDoNotStopOnExceptionTest, RecipientListWithArrayHeaderTest, RecipientListNoErrorHandlerTest, RecipientListThrowExceptionFromExpressionTest, RecipientListWithStringDelimitedPropertyTest, RecipientListWithSimpleExpressionTest, RecipientListExchangePropertyTest, RecipientListMEPTest, RecipientListWithDelimiterTest, RecipientListOnPrepareTest, RecipientListParallelStopOnExceptionWithOnExceptionTest}` (`tests/processor/recipient_list_misc_test.rs`)., `processor.{MulticastWithOnExceptionTest, MulticastWithOnExceptionFirstTest, MulticastWithOnExceptionLastTest, MulticastStopOnExceptionWithOnExceptionTest, MulticastStopOnExceptionLastWithOnExceptionTest, MulticastStopOnExceptionTest, MulticastNoStopOnExceptionTest, MulticastCompletedPropertyTest, MulticastKeepOriginalMessageUnchangedTest, MulticastDslTest, MulticastFineGrainedErrorHandlingTest, MulticastUseOriginalNotPropagateExceptionTest, MulticastGroupedExchangeExceptionTest}` and the `MulticastParallelFailureEndpointTest` case without doTry (`tests/processor/multicast_misc_test.rs`)., `processor.{ChoiceWithSimpleExpressionTest, ChoiceNoActionTest, ChoiceNoErrorHandlerTest, ChoiceWhenNotPredicateTest, ChoiceWhenNoOutputTest, ChoiceWithTranfromTest, ChoiceCompoundPredicateTest, ChoicePredicateThrowExceptionTest}` and the `ChoiceWithEndTest` cases (Java kept: the processor navigation case) (`tests/processor/choice_misc_test.rs`)., `processor.{WireTapLogEndpointTest, WireTapOnPrepareTest, WireTapOnPrepareRefTest, WireTapNoErrorHandlerTest, WireTapVariableTest, WireTapUsingMulticastTest}` (`tests/processor/wire_tap_misc_test.rs`)., `processor.{ToVariableTest, ToVariableReceiveTest, ToDynamicIgnoreTest, ToDynamicRawTest, ToDynamicPropertyPlaceholderTest}` (`tests/processor/to_variable_test.rs`)., `processor.{LoopThousandTest, LoopAsyncCopyTest, LoopAsyncNoCopyTest, LoopCopyPropagateVariableAsResultTestTest, LoopCopyErrorTest, LoopDoWhileDelayTest}` (`tests/processor/loop_misc_test.rs`)., `processor.{TransformProcessorTest, TransformProcessorWithHeaderTest, TransformViaDSLTest, TransformTest, SetPropertyTest, SetVariableTest, RemoveHeaderTest, RemovePropertyTest, RemoveVariableTest, RemoveHeadersExcludeTest, RemovePropertiesWithoutExclusionTest, RemovePropertiesWithExclusionTest, RemoveHeadersAllTest, RemoveHeadersTest}` and the `SetBodyProcessorTest` case with a header (`tests/processor/set_remove_transform_test.rs`)., `component.file.{FileProducerFilenameConstantTest, NewFileProduceTest, FileProducerFileExistIgnoreTest, FileProducerFileExistOverrideTest, FileProducerFileExistTryRenameTest, FileProduceAppendCharsTest, FileHeaderFileNameProducedTest, FileConsumerProducerRouteTest, FileConsumerIncludeExtTest, FileConsumerExcludeExtTest, FileConsumeSimpleRelativeMoveToRelativeTest, FileConsumerDeleteAndFailureTest, FileConsumerDeleteAndMoveFailedTest, FromFileMoveFileIfProcessFailsTest, FileConsumeNoopIdempotentAutoTest, FileConsumeNoopIdempotentEnabledTest, FileConsumeNoopIdempotentDisabledTest, FileConsumerFileNameFilterTest, FileProducerFileExistAppendNoFileBeforeTest, FileConsumeSingleDirectoryOnlyTest, FileConsumerConsumedFileNameTest, FileProduceGeneratedFileNameTest, FileConsumerAbsolutePathTest, FileConsumerAbsolutePathWithRelativeMoveTest, FileConsumeSimpleAbsoluteMoveToRelativeTest, FileConsumeSimpleRelativeMoveToAbsoluteTest, FileConsumerDirectoryNotMatchedTest, FileConvertBodyToUTF8Test, FilerProducerAbsoluteTest, PollEnrichSimpleMoveTest, FileConsumeTemplateTest, FileConsumerTemplateTest, ConsumerTemplateFileShutdownTest, FileConsumerPreMoveTest, FileConsumerPreMoveDeleteTest, FileConsumerPreMoveNoopTest, FileConsumeMaxMessagesPerPollTest, FileProducerFileExistFailTest, FileProducerFileExistOverrideNoFileBeforeTest, FileConsumerIdempotentTest, FileConsumerIdempotentKeyTest, FileIdempotentReadSameFileAgainTest, FileConsumeCharsetTest, FileConsumeWithDollarInPathTest, FileConsumerPathWithAmpersandTest, FileMoveErrorOnExceptionNotHandledTest, FromFileMoveDoneCustomHeaderTest, FileConsumerNoopTest, FileConsumerIncludeNameTest, FileMEPInOutTest, FileProduceAppendCharsEmptyMessageTest, FileConsumerAbsolutePathDefaultMoveTest, FileConsumerMoveAndMoveFailureTest, FileConsumerIncludeAndExcludeNameTest, FileConsumeHiddenFilesTest, FileConsumeMoveRelativeNameTest, FileConsumerBatchTest, FilePollEnrichTest, FileConsumerIdempotentOnExceptionHandledTest, FileConsumerExcludeNameTest, FileConsumerIdempotentKeyNameAndSizeTest, FileConsumeFileAsDirectoryTest, FileProduceAppendTest, FileConsumerInterceptEmptyFileTest, FileProduceAppendCharsSingleMessageTest, FileConsumeSimpleAbsoluteMoveToAbsoluteTest, FileConsumerAbsolutePathWithAbsoluteMoveTest, PollSimpleMoveTest, FileConsumerCommitRenameAbsolutePathStrategyTest, FileRenameFileOnCommitIssueTest, FileConsumerIdleMessageTest, FileConsumerPreMoveLastModifiedTest, FileConsumerMessageTimestampTest, FileMoveAndMoveFailedIssueTest, FileProducerNoForcedWritesTest, FileProducerToDMoveExistingTest, FileConsumeFilesAndDeleteTest, FileConsumeHiddenDirsTest, FileConsumerSkipDotFilesTest, FromFileDoNotDeleteFileIfProcessFailsTest, FromFileDoNotMoveFileIfProcessFailsTest, FileAbsolutePathIssueTest, FileProduceOverruleOnlyOnceTest, FilerProducerFileNamesTest, FileProducerJailStartingDirectoryTest, FileConsumerIdempotentKeyHeaderMutationTest, FilePollEnrichNoWaitTest, FileConsumerPreMoveIssueTest, FileRecursiveNoopTest, FileLanguageCBRTest, FromFilePollThirdTimeOkTest, FileAbsoluteAndRelativeConsumerTest, FileConsumePollEnrichFileTest, FileConsumerMoveFailureTest, FileConsumerMoveFailureOnCompletionTest, FileEagerDeleteTargetFileTest, FileProduceOverruleExpressionTest, FileProducerAllowNullBodyFileAlreadyExistsTest, DirectoryCreateIssueTest, FileConsumeDoneFileIssueTest, FileConsumeDynamicDoneFileNameWithTwoDotsTest, FileConsumeSimpleDynamicDoneFileNameWithTwoDotsTest, FilerConsumerDoneFileNameDeleteTest, FilerConsumerDoneFileNamePrefixTest, FilerConsumerDoneFileNameSimplePrefixTest, FilerConsumerDoneFileNameSuffixTest, FilerConsumerDoneFileNameTest, FilerConsumerDoneFileNoopTest, FilerConsumerDualDoneFileNameTest, FilerConsumerMoveFailedDoneFileNameTest, FilerConsumerPreMoveDoneFileNameTest, FilerConsumerRetryDoneFileNameTest, FilerConsumerShouldSkipDoneFilePrefixTest, FilerConsumerShouldSkipDoneFileSuffixTest, FilerConsumerShouldSkipDoneFileTest, FilerProducerDoneFileNameRouteTest, FilerProducerDoneFileNameTest}` and the `FileProducerFileExistAppendTest` case with a text body (`tests/component/file`)., the `component.file.FileProducerMoveExistingTest` cases (Java kept: the dynamic sub directory cases) and the `FileProduceTempFileNameTest` route cases (Java kept: the createTempFileName cases)., `processor.enricher.PollTest` and the `PollDynamicFileNameTest` cases (Java kept: the single file endpoint check needs PollDynamicAware). |
| `core/camel-core-all` | 0 | 0 | todo | |
| `core/camel-core-catalog` | 7 | 0 | todo | |
| `core/camel-core-engine` | 11 | 0 | wip | `ModelCamelContext::add_routes` / `add_route_definitions` (model → reifier → engine). `DefaultCamelContext` itself lives in `camel-base-engine`. Pending: model registry (route definitions kept for dumping/reloading), route templates, dumpers. |
| `core/camel-core-languages` | 90 | 0 | wip | Rust crate `camel-core-languages`: `constant`, `header`, `exchangeProperty`, `variable` languages and the **simple** language: tokenizer (full), expression parser (templates, functions, nested functions, `++`/`--`), predicate parser (all binary operators, `&&`/`||`, `!`, quoted literals, numbers, booleans, null; Java error messages with location), functions: the direct functions (`id`, `exchangeId`, `camelId`, `routeId`, `exception*`, `threadName`, `hostname`, `null`, ...), `body`/`bodyAs`/`mandatoryBodyAs`/`bodyType`/`bodyOneLine`, `header(s)`/`headerAs`, `exchangeProperty`, `variable(s)`/`variableAs`, OGNL navigation (`simple::ognl`, Java `BeanExpression.invokeOgnlMethod`: the methods of String/List/Map/numbers, getters and properties of `camel_util::value::Bean` values such as `Exchange`, `Message` and `CamelError`, map keys, list indexes with the Java errors), `${exchange}`/`${exchange.OGNL}`, `${exception}` (an object)/`${exception.OGNL}`, `${camelContext.OGNL}`, `getClass()` (a class bean with `name`/`simpleName`/`canonicalName`), keyed OGNL on headers/properties/variables as Java `KeyedOgnlExpressionAdapter` (`${headers[a.b].name}`, nested functions in keys and method arguments), `${logExchange}`, `int:`/`long:`/`boolean:`/`string:` prefixes, the string functions (`replace`, `substring*`, `contains`, `trim`, `val`, `capitalize`, `pad`, `concat`, `quote`, `safeQuote`, `unquote`, `uppercase`, `lowercase`, `length`, `size`, `escape`, `normalizeWhitespace`), the math functions (`abs`, `floor`, `ceil`, `sum`, `max`, `min`, `average`), the misc functions (`isEmpty`, `isAlpha`, `isAlphaNumeric`, `isNumeric`, `not`, `kindOfType`, `throwException`, `assert`, `convertTo` (no OGNL suffix yet), `uuid`, `hash`, `empty`/`newEmpty`, `iif`; pending `messageHistory`, `load`), the collection functions (`range`, `distinct`, `reverse`, `shuffle`, `split`, `sort`, `forEach`, `filter`, `list`, `map`, the mutating `setHeader`, `setVariable`, `listAdd`, `listRemove`, `mapAdd`, `mapRemove` with `Expression::evaluate_mut`, see the handoff), `join`, `collate`, `skip`, `random`, `sys.`/`sysenv.`/`env.`, the date functions (`date:`/`date-with-timezone:` with offsets and Java `SimpleDateFormat` patterns, `camel_util::date`), the file functions (`file:name`, `file:onlyname`, `file:ext`, `file:parent`, `file:size`, ...), `${bean:...}` (`simple::bean_functions`, through the `bean` language of the context), `${properties:key:default}`, `${propertiesExist:[!]key}`, `${ref:name}` (`simple::properties_functions`). the ternary operator in functions (`${header.foo > 0 ? 'a' : 'b'}`, nested) and in predicates (`${header.foo} > 0 ? 'a' : 'b'`), the elvis operator `?:`, the chain operators `~>`/`?~>` with `$param` (`simple::other_operators`; custom functions in chains pending), predicates inside the braces (`${body != null && body.size() > 0}`, CAMEL-24921), `simple::syntax_hints` (Java `SimpleSyntaxHints`: did-you-mean for unknown functions/operators, wrapping), the OGNL hints (closest method, missing map key, method without parentheses). Pending: init blocks, the type/message/output function factories, resources (`resource:`), the file language. Tests: `SimpleParserExpressionTest`, `SimpleParserPredicateTest` (except `testSimpleIn`, needs `ref:`), `SimplePredicateInBracesTest` and `SimpleSyntaxHintsTest` (except the `properties:` and `semantic` cases), `SimpleStringMathDateFunctionsTest`, `SimpleOperatorTest`, `SimpleParserEdgeCasesTest`, `functions/BeanFunctionFactoryTest` (Java removed), `functions/DateFunctionFactoryTest` (except the custom type converter case), `SimpleTest` in progress (`tests/language/simple_test.rs`: the tests of the ported functions; Java arrays are `Value::List`, so `length()` of a list is its text length as for a Java `List`). Init blocks (`simple::init_block`: `$init{ $a := ...; $f ~:= chain; }init$`, the variables set on the exchange, the init functions called as `${function(name[,value])}`). Custom functions (`camel_api::spi::simple_function::SimpleFunctionRegistry`, a context plugin; `${function(name[,exp])}`, `${name}`, `${name(exp)}`; the language gets its context through `Language::set_camel_context`), `resource:scheme:path` expressions (`camel_support::resource_helper`). Expression options (`ExpressionDefinition::Configured`: `nested`, `result_type`, `trim`, `pretty`; `camel_util::pretty` JSON and XML printers). The tokenize language (`TokenizeLanguage`; the tokenizers in `camel_support::tokenizer::TokenizeExpression`: token, regex, pair, XML with inherited namespaces, group, skip first, source; DSL `tokenize`, `tokenize_regex`, `tokenize_pair`, `tokenize_xml`; pending: the `<*>` wrap mode). |
| `core/camel-core-model` | 402 | 5 | wip | `RouteDefinition`, `ProcessorDefinition` + `Eip` (to, filter, choice/when/otherwise, pipeline, setBody, transform, setHeader/Property/Variable, removeHeader(s), log, stop, process, throwException, doTry/doCatch/doFinally, split, multicast, delay, toD, recipientList, wireTap, convertBodyTo, setExchangePattern, aggregate, bean, setHeaders, setVariables, removeProperty, removeProperties, removeVariable, script, loop (count, doWhile, copy), routingSlip, dynamicRouter (uriDelimiter, ignoreInvalidEndpoints), sort, step, validate, idempotentConsumer, loadBalance (roundRobin, random, sticky, topic, failover, weighted), enrich, pollEnrich, poll, sample, throttle, claimCheck, rollback/markRollbackOnly(Last), convertHeaderTo, convertVariableTo), resequence (batch: size, timeout, allowDuplicates, reverse; stream: capacity, timeout, rejectOld; ignoreInvalidExchanges), the interceptors of route builders (`intercept`, `interceptFrom`, `interceptSendToEndpoint` with onWhen, skipSendToOriginalEndpoint, afterUri; `intercept` module), error handlers (default, dead letter channel, none) and `onException` on routes and route builders, with the Java `toString()` forms, `ExpressionDefinition` (language text, or Rust expression/predicate), and the **Rust route DSL** (`builder`: `from(..)`, `Dsl` steps, closures for nested blocks, `RouteBuilder`/`Routes`). Pending: the other ~80 EIPs, data formats, language definitions, rest DSL, route templates, route configurations. |
| `core/camel-core-processor` | 170 | 0 | wip | Done (Java removed): `resequence.ResequenceProcessor` (Java `Resequencer` + `StreamResequencer`/`ResequencerEngine` on a tokio task; pending: batch consumer, completion predicate, custom comparators, deliveryAttemptInterval), `intercept.{InterceptProcessor, InterceptFromProcessor, InterceptSendToEndpointProcessor}` (bound by the reifier: each step, the start of the route, the `to` steps matching the uri pattern; pending: intercepting the sends of producer templates and dynamic `toD`, route configurations), `throttle.ThrottleProcessor` (total requests per period and concurrent requests, correlation key, rejectExecution; pending: asyncDelayed, callerRunsWhenRejected), `claim_check.ClaimCheckProcessor` (+ `ClaimCheckAggregationStrategy` filter rules), `RollbackProcessor`, `dynamic.ConvertValueProcessor` (convertHeaderTo/convertVariableTo), `enricher.{EnrichProcessor, PollEnrichProcessor}` (aggregation strategy or copy, aggregateOnException, ignoreInvalidEndpoint, poll timeout; pending: shareUnitOfWork, variables, cacheSize, bridge error handler), `routing.SamplingProcessor`, `idempotent.IdempotentConsumerProcessor` (eager, completionEager, skipDuplicate, removeOnFailure, the on completion), `loadbalancer.LoadBalancerProcessor` (round robin, random, sticky, topic, failover with exceptions/roundRobin/sticky/maximumFailoverAttempts, weighted round robin/random; pending: custom balancers, inheritErrorHandler), `ValidateProcessor` (pending: predicate exception factory), `routing.{LoopProcessor, RoutingSlipProcessor (also the dynamic router), SortProcessor, StepProcessor}` (pending: loop onPrepare/breakOnShutdown, routing slip error handler per step and cache size), `SetHeadersProcessor`, `SetVariablesProcessor`, `RemovePropertyProcessor`, `RemovePropertiesProcessor`, `RemoveVariableProcessor`, `ScriptProcessor`, `Pipeline` + `PipelineHelper`, `FilterProcessor`, `ChoiceProcessor`, `SetBody`/`SetHeader`/`SetProperty`/`SetVariable`/`RemoveHeader`/`RemoveHeaders`/`Stop` processors, `TryProcessor`/`CatchProcessor`/`FinallyProcessor` (`doTry`), `MulticastProcessor`/`Splitter`/`Delayer` (sequential and parallel on tokio tasks, ordered or streaming aggregation, timeout with `AggregationStrategy::timeout`, UseLatest default; pending: lazy streaming iteration, share unit of work, custom thread pools, onPrepare), `aggregate.UseLatestAggregationStrategy`, `aggregate.GroupedExchangeAggregationStrategy`, `aggregate.GroupedBodyAggregationStrategy`, `aggregate.MemoryAggregationRepository`, `AggregateProcessor` (size/size expression/predicate/timeout/timeout expression/interval/strategy/force completion, eager check, closed and invalid correlation keys, discard options; pending: parallel processing, optimistic locking, recoverable repositories, batch consumer completion; forceCompletionOnStop/completeAllOnStop complete on stop without the Java shutdown-strategy waiting), `SendDynamicProcessor` (toD), `RecipientList` (sequential or parallel, endpoints resolved before sending), `WireTapProcessor` (tokio task; pending: thread pool, onPrepare, dynamic/no cache options), `ConvertBodyProcessor` (pending: charset), `SetExchangePatternProcessor`, `OnCompletionProcessor` (Java kept: custom executor and shutdown awareness pending), the unit of work (`unit_of_work::UnitOfWorkProcessor`, Java `CamelInternalProcessor` UoW advice), `errorhandler` (`RedeliveryErrorHandler` as default error handler and dead letter channel: redelivery with delays/back off/delay pattern/retryWhile, `onException` policies with the Java closest-type selection, handled/continued, onRedelivery, onExceptionOccurred, onPrepareFailure, the Java log messages; useOriginalMessage/useOriginalBody; pending: message history in logs, async delayed redelivery, shutdown handling, exchange formatter). Partial (Java kept until complete): `SendProcessor` (no ProducerCache/event notifier yet), `LogProcessor` (no masking/log listeners), `TransformProcessor` (in `SetBodyProcessor`), `ThrowExceptionProcessor` (takes a ready error; no class + message expression). Errors are carried on the exchange as in Java (`process_and_capture`). |
| `core/camel-core-reifier` | 171 | 0 | wip | `reify_route`, `RouteReifier` (the EIPs of the model), `reify_expression`/`reify_predicate` (languages from the context). Errors as Java `FailedToCreateRouteException ... at: >>> node <<< in route: ...`. |
| `core/camel-core-xml` | 32 | 1 | todo | |
| `core/camel-health` | 12 | 0 | todo | |
| `core/camel-java-io` | 12 | 9 | n/a | Reads Java DSL sources (LwJavaParser); routes are written in Rust/YAML/XML in the Rust port |
| `core/camel-main` | 69 | 114 | wip | Rust crate `camel-main` (`Main`, `MainConfigurationProperties`, `MainListener`): properties locations (default `application.properties`, the profile `application-{profile}.properties` first, `false` disabling them), initial/override properties, `camel.main.*` options (name, profile, duration (with units), durationMaxSeconds/Messages/IdleSeconds, tracing, tracingStandby, messageHistory) and `camel.context.name`, bind, routes builders, listeners, start/stop, run until completed/duration/Ctrl-C, command line options; `DefaultCamelContext::set_name`. Pending: `camel.component.*`/beans/rest configuration, route discovery and reload, file configurations, health, vaults, SSL, tracer. Tests: MainPropertyPlaceholderTest, MainDurationAliasTest (Java removed), MainTest and MainListenerTest cases (`tests/main_test.rs`). |
| `core/camel-management` | 137 | 291 | todo | |
| `core/camel-management-api` | 127 | 0 | todo | |
| `core/camel-support` | 318 | 13 | wip | Done: `SimpleRegistry`, `DefaultHeaderFilterStrategy`, `PatternHelper` (+ test), `DefaultUuidGenerator` (in camel-api `spi::uuid`), `DefaultComponent.createEndpoint` (`component_support`), the UUID generators (`ClassicUuidGenerator`, `ShortUuidGenerator`, `SimpleUuidGenerator`, `RandomUuidGenerator`, `OffUuidGenerator`), `ExchangeHelper.copyResults`/`createCorrelatedCopy` (`exchange_helper`), `MemoryIdempotentRepository` (`idempotent`; spi `IdempotentRepository` in camel-api), `ObjectHelper` type coercion (`typeCoerceEquals`/`Compare`/`Contains`, `isNumber`, `isFloatingNumber`, `compare`), `processor.DefaultExchangeFormatter` + `MessageHelper.extractValueForLogging` (`exchange_formatter`; stream/file/future options pending), `builder.{ExpressionBuilder, PredicateBuilder, ValueBuilder, BinaryPredicateSupport}` (`in` compares text values with a number by value as `inValueExpression`) (header/body/exchangeProperty/variable/constant, comparisons, contains, regex, and/or/not, append/prepend, convertTo). |
| `core/camel-util` | 89 | 35 | done | Done: `AntPathMatcher`, `StringHelper`, `StringQuoteHelper`, `URISupport`, `URIScanner`, `UnsafeUriCharactersEncoder`, `CamelURIParser`, `SensitiveUtils`, `Pair`, `TimeUtils` (+ signed `SignedDuration` for `java.time.Duration`), `UnitUtils`, `StopWatch`, `OgnlHelper`, `KeyValueHolder`, `ReferenceCount`, `EscapeHelper`, `CollectionHelper`, `ObjectHelper` (portable part; reflection/class loading helpers n/a), `backoff.*` (BackOff, SimpleBackOffTimer), `concurrent.*` (std-thread ThreadPoolExecutor, ScheduledThreadPoolExecutor, Sized/Bounded/Synchronous executors, AsyncCompletionService, ContextValue, CountDownLatch; n/a: LockHelper, NamedThreadLocal, java21/java25 virtual-thread variants), `ArtifactUtils` (hints say "dependencies" instead of "classpath"), `SecurityUtils`, `MimeTypeHelper`, `Scanner` (streaming regex tokenizer), `HomeHelper`, `SecurityViolation`, `SecureRandomHelper` (getrandom), `LocationHelper`, `ImportantHeaderUtils`, `SimpleUtils`, `HostUtils` + `InetAddressUtil` (gethostname, if-addrs), `FilterIterator`, `SkipIterator`, `function.Predicates`; n/a: `UnwrapHelper` (Quarkus proxies), `StreamUtils` (Rust iterators), `DoubleMap` (deprecated, identity keys); `FileUtil`, `FilePathResolver`, `IOHelper` (+ `charset` module replacing java.nio.charset: Java names, true Latin-1, streaming transcoders), n/a: `SkipLastByteInputStream` (deprecated, unused), `OrderedProperties`, `CamelCaseOrderedProperties`, `OrderedLocationProperties`, `PropertiesHelper` (+ `.properties` format parser), `CaseInsensitiveMap` (Arc<str> known-key dedup; `testSerialization` n/a: Java serialization) (+ camel-core `CaseInsensitiveMapTest`, `InetAddressUtilTest`, `FileUtilTest`, `FilePathResolverTest`, `StingQuoteHelperTest`, `UnsafeCharactersEncoderTest`, `OgnlHelperTest`, `KeyValueHolderTest`, `ReferenceCountTest`). New `net` module reproduces `java.net.URI`/`URLEncoder`/`URLDecoder`; new `date` module reproduces `java.util.Date`, `TimeZone` (bundled IANA database via `jiff`) and `SimpleDateFormat` formatting. Tooling TODO: regenerate `SENSITIVE_KEYS`, `IMPORTANT_HEADER_KEYS`, `SIMPLE_FUNCTIONS`, the security option tables, the artifact tables and the MIME table from the catalog. camel-core `ObjectHelperTest`: camel-util parts ported, `camel-support` `ObjectHelper` parts pending. |
| `core/camel-xml-io` | 9 | 6 | todo | |
| `core/camel-xml-io-util` | 4 | 2 | todo | |
| `core/camel-xml-jaxb` | 3 | 0 | todo | |
| `core/camel-xml-jaxp` | 23 | 3 | todo | |
| `core/camel-xml-jaxp-util` | 1 | 1 | todo | |
| `core/camel-yaml-io` | 5 | 9 | todo | |
| `dsl/camel-cli-connector` | 12 | 7 | todo | |
| `dsl/camel-cli-debug` | 1 | 0 | todo | |
| `dsl/camel-componentdsl` | 2 | 1 | todo | Becomes generated, typed Rust component builders |
| `dsl/camel-dsl-modeline` | 3 | 1 | todo | |
| `dsl/camel-dsl-support` | 4 | 0 | todo | |
| `dsl/camel-endpointdsl` | 4 | 47 | todo | Becomes generated, typed Rust endpoint builders for the Rust route DSL |
| `dsl/camel-endpointdsl-support` | 1 | 0 | todo | |
| `dsl/camel-java-joor-dsl` | 2 | 30 | n/a | Java DSL compiled at runtime with jOOR; replaced by the Rust route DSL |
| `dsl/camel-jbang` | 0 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-console` | 3 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-core` | 304 | 236 | todo | |
| `dsl/camel-jbang/camel-jbang-it` | 0 | 44 | todo | |
| `dsl/camel-jbang/camel-jbang-main` | 1 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-mcp` | 49 | 39 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-edit` | 3 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-generate` | 4 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-kubernetes` | 68 | 15 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-mcp` | 2 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-route-parser` | 2 | 0 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-test` | 4 | 1 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-tui` | 239 | 204 | todo | |
| `dsl/camel-jbang/camel-jbang-plugin-validate` | 8 | 2 | todo | |
| `dsl/camel-jbang/camel-launcher` | 8 | 17 | todo | |
| `dsl/camel-jbang/camel-launcher-container` | 0 | 0 | todo | |
| `dsl/camel-kamelet-main` | 67 | 24 | todo | |
| `dsl/camel-kamelet-main-support` | 5 | 0 | todo | |
| `dsl/camel-xml-io-dsl` | 3 | 28 | wip | Rust crate `camel-xml-io-dsl` (`XmlRoutes`, on roxmltree): `<routes>`/`<camel>`/`<route>` documents (with or without the xml-io namespace) converted to the tree of the YAML DSL (attributes as options, language elements as expressions, option elements such as `<when>`, `<doCatch>`, `<exception>`, `<onWhen>`, `<handled>`, the other elements as nested steps); top level `onException`, `intercept*`, `errorHandler`. Pending: rests, route templates, route configurations, beans, kamelets. Tests: `XmlLoadTest` (routes cases), `XmlRoutesBuilderLoaderTest.canLoadRoutes` and Rust tests (`tests/xml_dsl.rs`). |
| `dsl/camel-xml-jaxb-dsl` | 2 | 8 | todo | |
| `dsl/camel-xml-jaxb-dsl-test` | 0 | 0 | todo | |
| `dsl/camel-xml-jaxb-dsl-test/definition` | 0 | 9 | todo | |
| `dsl/camel-xml-jaxb-dsl-test/spring` | 0 | 1 | todo | |
| `dsl/camel-yaml-dsl` | 0 | 18 | todo | |
| `dsl/camel-yaml-dsl/camel-yaml-dsl` | 3 | 141 | wip | Rust crate `camel-yaml-dsl` (`YamlRoutes`, a `RouteBuilder`, on serde_yaml): the top level `route`, `from`, `errorHandler`, `onException`, `intercept`, `interceptFrom`, `interceptSendToEndpoint`, `beans` (known framework types); the steps of the EIPs of the Rust model with camelCase/kebab-case keys, expressions inline or in an `expression` block (exactly one, the Java schema errors), endpoint `parameters`, an EIP with outputs without `steps` taking the following steps (as Java). Pending: rest, routeTemplate, routeConfiguration, kamelets, data formats, the schema validation of all options, YAML dumping, route reloading. Tests: ToTest, LoopTest, PipelineTest, SetHeadersTest, ThrowExceptionTest, RemovePropertyTest, IdempotentConsumerTest, InterceptTest, InterceptFromTest (Java removed), and the ExpressionTest, RecipientListTest, ThrottleTest, DelayTest cases (`tests/yaml_dsl.rs`). |
| `dsl/camel-yaml-dsl/camel-yaml-dsl-common` | 21 | 2 | todo | |
| `dsl/camel-yaml-dsl/camel-yaml-dsl-deserializers` | 15 | 1 | todo | |
| `dsl/camel-yaml-dsl/camel-yaml-dsl-maven-plugin` | 9 | 0 | todo | |
| `dsl/camel-yaml-dsl/camel-yaml-dsl-validator` | 12 | 27 | todo | |
| `dsl/camel-yaml-dsl/camel-yaml-dsl-validator-maven-plugin` | 2 | 2 | todo | |
| `components/camel-activemq` | 10 | 4 | todo | |
| `components/camel-activemq6` | 10 | 4 | todo | |
| `components/camel-ai` | 0 | 0 | todo | |
| `components/camel-ai/camel-a2a` | 98 | 43 | todo | |
| `components/camel-ai/camel-ai-observability` | 6 | 12 | todo | |
| `components/camel-ai/camel-ai-observability-api` | 11 | 6 | todo | |
| `components/camel-ai/camel-ai-resource` | 10 | 6 | todo | |
| `components/camel-ai/camel-ai-tool` | 13 | 13 | todo | |
| `components/camel-ai/camel-chatscript` | 6 | 2 | todo | |
| `components/camel-ai/camel-djl` | 38 | 18 | todo | |
| `components/camel-ai/camel-docling` | 15 | 22 | todo | |
| `components/camel-ai/camel-huggingface` | 19 | 6 | todo | |
| `components/camel-ai/camel-kserve` | 6 | 3 | todo | |
| `components/camel-ai/camel-langchain4j-agent` | 9 | 46 | todo | |
| `components/camel-ai/camel-langchain4j-agent-api` | 26 | 16 | todo | |
| `components/camel-ai/camel-langchain4j-chat` | 9 | 6 | todo | |
| `components/camel-ai/camel-langchain4j-core` | 3 | 2 | todo | |
| `components/camel-ai/camel-langchain4j-embeddings` | 9 | 12 | todo | |
| `components/camel-ai/camel-langchain4j-embeddingstore` | 7 | 7 | todo | |
| `components/camel-ai/camel-langchain4j-embeddingstore-api` | 1 | 0 | todo | |
| `components/camel-ai/camel-langchain4j-ingest` | 13 | 13 | todo | |
| `components/camel-ai/camel-langchain4j-tokenizer` | 11 | 6 | todo | |
| `components/camel-ai/camel-langchain4j-web-search` | 6 | 4 | todo | |
| `components/camel-ai/camel-mcp-server` | 7 | 18 | todo | |
| `components/camel-ai/camel-mcp-server-api` | 13 | 12 | todo | |
| `components/camel-ai/camel-milvus` | 16 | 4 | todo | |
| `components/camel-ai/camel-neo4j` | 12 | 4 | todo | |
| `components/camel-ai/camel-openai` | 48 | 79 | todo | |
| `components/camel-ai/camel-pgvector` | 10 | 1 | todo | |
| `components/camel-ai/camel-pinecone` | 9 | 4 | todo | |
| `components/camel-ai/camel-qdrant` | 14 | 11 | todo | |
| `components/camel-ai/camel-semantic` | 34 | 26 | todo | |
| `components/camel-ai/camel-tensorflow-serving` | 6 | 3 | todo | |
| `components/camel-ai/camel-typesafe-ai` | 10 | 16 | todo | |
| `components/camel-ai/camel-weaviate` | 8 | 5 | todo | |
| `components/camel-ai/camel-wolf-defender` | 3 | 6 | todo | |
| `components/camel-alibaba` | 0 | 0 | todo | |
| `components/camel-alibaba/camel-alibaba-common` | 2 | 0 | todo | |
| `components/camel-alibaba/camel-alibaba-eventbridge` | 13 | 3 | todo | |
| `components/camel-alibaba/camel-alibaba-fc` | 8 | 2 | todo | |
| `components/camel-alibaba/camel-alibaba-kms` | 8 | 2 | todo | |
| `components/camel-alibaba/camel-alibaba-mns` | 9 | 7 | todo | |
| `components/camel-alibaba/camel-alibaba-oss` | 10 | 10 | todo | |
| `components/camel-alibaba/camel-alibaba-ots` | 8 | 9 | todo | |
| `components/camel-alibaba/camel-alibaba-sls` | 8 | 7 | todo | |
| `components/camel-alibaba/camel-alibaba-sms` | 8 | 2 | todo | |
| `components/camel-amqp` | 7 | 9 | todo | |
| `components/camel-arangodb` | 6 | 14 | todo | |
| `components/camel-as2` | 0 | 0 | todo | |
| `components/camel-as2/camel-as2-api` | 67 | 25 | todo | |
| `components/camel-as2/camel-as2-component` | 10 | 25 | todo | |
| `components/camel-asn1` | 4 | 5 | todo | |
| `components/camel-asterisk` | 8 | 3 | todo | |
| `components/camel-atmosphere-websocket` | 10 | 12 | todo | |
| `components/camel-atom` | 15 | 17 | todo | |
| `components/camel-attachments` | 10 | 10 | todo | |
| `components/camel-avro` | 3 | 9 | todo | |
| `components/camel-avro-rpc` | 0 | 0 | todo | |
| `components/camel-avro-rpc/camel-avro-rpc-component` | 17 | 25 | todo | |
| `components/camel-avro-rpc/camel-avro-rpc-jetty` | 1 | 0 | todo | |
| `components/camel-avro-rpc/camel-avro-rpc-spi` | 1 | 0 | todo | |
| `components/camel-aws` | 0 | 0 | todo | |
| `components/camel-aws/camel-aws-bedrock` | 32 | 22 | todo | |
| `components/camel-aws/camel-aws-cloudtrail` | 7 | 1 | todo | |
| `components/camel-aws/camel-aws-common` | 3 | 1 | todo | |
| `components/camel-aws/camel-aws-config` | 8 | 7 | todo | |
| `components/camel-aws/camel-aws-parameter-store` | 9 | 11 | todo | |
| `components/camel-aws/camel-aws-secrets-manager` | 11 | 18 | todo | |
| `components/camel-aws/camel-aws-security-hub` | 8 | 3 | todo | |
| `components/camel-aws/camel-aws2-athena` | 10 | 11 | todo | |
| `components/camel-aws/camel-aws2-comprehend` | 8 | 4 | todo | |
| `components/camel-aws/camel-aws2-cw` | 8 | 6 | todo | |
| `components/camel-aws/camel-aws2-ddb` | 38 | 48 | todo | |
| `components/camel-aws/camel-aws2-ec2` | 8 | 12 | todo | |
| `components/camel-aws/camel-aws2-ecs` | 8 | 9 | todo | |
| `components/camel-aws/camel-aws2-eks` | 8 | 8 | todo | |
| `components/camel-aws/camel-aws2-eventbridge` | 9 | 19 | todo | |
| `components/camel-aws/camel-aws2-iam` | 8 | 15 | todo | |
| `components/camel-aws/camel-aws2-kinesis` | 25 | 20 | todo | |
| `components/camel-aws/camel-aws2-kms` | 7 | 13 | todo | |
| `components/camel-aws/camel-aws2-lambda` | 8 | 21 | todo | |
| `components/camel-aws/camel-aws2-mq` | 8 | 8 | todo | |
| `components/camel-aws/camel-aws2-msk` | 8 | 8 | todo | |
| `components/camel-aws/camel-aws2-polly` | 8 | 5 | todo | |
| `components/camel-aws/camel-aws2-redshift` | 7 | 5 | todo | |
| `components/camel-aws/camel-aws2-rekognition` | 8 | 7 | todo | |
| `components/camel-aws/camel-aws2-s3` | 14 | 61 | todo | |
| `components/camel-aws/camel-aws2-s3-vectors` | 8 | 4 | todo | |
| `components/camel-aws/camel-aws2-ses` | 7 | 10 | todo | |
| `components/camel-aws/camel-aws2-sns` | 15 | 20 | todo | |
| `components/camel-aws/camel-aws2-sqs` | 18 | 39 | todo | |
| `components/camel-aws/camel-aws2-step-functions` | 8 | 7 | todo | |
| `components/camel-aws/camel-aws2-sts` | 7 | 10 | todo | |
| `components/camel-aws/camel-aws2-textract` | 8 | 3 | todo | |
| `components/camel-aws/camel-aws2-timestream` | 13 | 12 | todo | |
| `components/camel-aws/camel-aws2-transcribe` | 8 | 7 | todo | |
| `components/camel-aws/camel-aws2-translate` | 9 | 6 | todo | |
| `components/camel-azure` | 0 | 0 | todo | |
| `components/camel-azure/camel-azure-common` | 2 | 2 | todo | |
| `components/camel-azure/camel-azure-cosmosdb` | 17 | 13 | todo | |
| `components/camel-azure/camel-azure-eventgrid` | 5 | 1 | todo | |
| `components/camel-azure/camel-azure-eventhubs` | 11 | 12 | todo | |
| `components/camel-azure/camel-azure-files` | 18 | 5 | todo | |
| `components/camel-azure/camel-azure-functions` | 9 | 3 | todo | |
| `components/camel-azure/camel-azure-key-vault` | 9 | 4 | todo | |
| `components/camel-azure/camel-azure-schema-registry` | 1 | 0 | todo | |
| `components/camel-azure/camel-azure-servicebus` | 15 | 8 | todo | |
| `components/camel-azure/camel-azure-storage-blob` | 24 | 21 | todo | |
| `components/camel-azure/camel-azure-storage-datalake` | 23 | 10 | todo | |
| `components/camel-azure/camel-azure-storage-queue` | 16 | 9 | todo | |
| `components/camel-barcode` | 4 | 7 | todo | |
| `components/camel-base64` | 4 | 14 | todo | |
| `components/camel-bean` | 43 | 1 | wip | Rust crate `camel-bean`: the bean language (`BeanLanguage`, `BeanExpression`: `bean.method`, `bean?method=`, `bean::method`, parameters, OGNL on the result, validation as `MethodNotFoundException`, headers/properties copied back by `evaluate_mut`) and `MethodBean` (a bean made of closures). Bean binding API in camel-api `bean_binding` (`ExchangeBean`, `BeanInvocation`). n/a: `type:` beans (class loading), proxies (`ProxyHelper`, `CamelInvocationHandler`), parameter annotations (the bean method reads the exchange). The bean component (`bean:name?method=`, producer only), `BeanProcessor` (result as body unless a void method, processors bound as beans, validation when the route is created), `DefaultBeanProcessorFactory` (the context plugin the reifier uses for the bean EIP). Pending: `class:`, `@Handler`-like method choice, parameter binding by type, bean properties (`bean.*`). Tests: `language/BeanTest` (type cases with instances), the `bean:` cases of the simple tests, `BeanExplicitMethodTest`, `BeanInvokeSingleMethodNoBodyTest`, `BeanRefNotFoundTest`, `BeanRefMethodNotFoundTest`, `MethodCallRefOrBeanPrefixTest` (`tests/component/bean`). n/a: `BeanMethodBeanTest` (reflection), `BeanMapPutTest` (a registry value changed in place). |
| `components/camel-bean-validator` | 7 | 16 | todo | |
| `components/camel-beanio` | 7 | 19 | todo | |
| `components/camel-bindy` | 60 | 171 | todo | |
| `components/camel-bonita` | 17 | 10 | todo | |
| `components/camel-box` | 0 | 0 | todo | |
| `components/camel-box/camel-box-api` | 11 | 0 | todo | |
| `components/camel-box/camel-box-component` | 8 | 13 | todo | |
| `components/camel-braintree` | 8 | 19 | todo | |
| `components/camel-browse` | 2 | 0 | todo | |
| `components/camel-caffeine` | 17 | 21 | todo | |
| `components/camel-camunda` | 25 | 3 | todo | |
| `components/camel-cassandraql` | 17 | 21 | todo | |
| `components/camel-cbor` | 2 | 6 | todo | |
| `components/camel-chunk` | 3 | 11 | todo | |
| `components/camel-clickhouse` | 6 | 3 | todo | |
| `components/camel-clickup` | 26 | 6 | todo | |
| `components/camel-cloudevents` | 3 | 2 | todo | |
| `components/camel-cm-sms` | 25 | 17 | todo | |
| `components/camel-coap` | 9 | 23 | todo | |
| `components/camel-cometd` | 6 | 14 | todo | |
| `components/camel-consul` | 32 | 24 | todo | |
| `components/camel-controlbus` | 3 | 0 | todo | |
| `components/camel-couchbase` | 7 | 13 | todo | |
| `components/camel-couchdb` | 8 | 6 | todo | |
| `components/camel-cron` | 5 | 3 | todo | |
| `components/camel-crypto` | 12 | 10 | todo | |
| `components/camel-crypto-pgp` | 10 | 8 | todo | |
| `components/camel-csv` | 8 | 35 | todo | |
| `components/camel-cxf` | 0 | 0 | todo | |
| `components/camel-cxf/camel-cxf-common` | 20 | 1 | todo | |
| `components/camel-cxf/camel-cxf-rest` | 23 | 42 | todo | |
| `components/camel-cxf/camel-cxf-soap` | 28 | 104 | todo | |
| `components/camel-cxf/camel-cxf-spring-common` | 2 | 0 | todo | |
| `components/camel-cxf/camel-cxf-spring-rest` | 7 | 41 | todo | |
| `components/camel-cxf/camel-cxf-spring-soap` | 4 | 130 | todo | |
| `components/camel-cxf/camel-cxf-spring-transport` | 4 | 9 | todo | |
| `components/camel-cxf/camel-cxf-transport` | 11 | 7 | todo | |
| `components/camel-cyberark-vault` | 10 | 4 | todo | |
| `components/camel-dapr` | 26 | 14 | todo | |
| `components/camel-dataformat` | 2 | 0 | todo | |
| `components/camel-dataset` | 11 | 0 | todo | |
| `components/camel-datasonnet` | 6 | 14 | todo | |
| `components/camel-dataweave` | 5 | 3 | todo | |
| `components/camel-debezium` | 0 | 0 | todo | |
| `components/camel-debezium/camel-debezium-common` | 0 | 0 | todo | |
| `components/camel-debezium/camel-debezium-common/camel-debezium-common-component` | 8 | 11 | todo | |
| `components/camel-debezium/camel-debezium-common/camel-debezium-maven-plugin` | 6 | 5 | todo | |
| `components/camel-debezium/camel-debezium-db2` | 2 | 2 | todo | |
| `components/camel-debezium/camel-debezium-mongodb` | 2 | 2 | todo | |
| `components/camel-debezium/camel-debezium-mysql` | 2 | 2 | todo | |
| `components/camel-debezium/camel-debezium-oracle` | 2 | 2 | todo | |
| `components/camel-debezium/camel-debezium-postgres` | 2 | 2 | todo | |
| `components/camel-debezium/camel-debezium-sqlserver` | 2 | 2 | todo | |
| `components/camel-debug` | 1 | 0 | todo | |
| `components/camel-dfdl` | 6 | 2 | todo | |
| `components/camel-dhis2` | 0 | 0 | todo | |
| `components/camel-dhis2/camel-dhis2-api` | 7 | 5 | todo | |
| `components/camel-dhis2/camel-dhis2-component` | 8 | 8 | todo | |
| `components/camel-diagram` | 10 | 9 | todo | |
| `components/camel-direct` | 5 | 0 | done | Rust crate `camel-direct` (block/timeout wait with tokio, failIfNoConsumers, one consumer per endpoint, suspend/resume). Java removed; `direct.json` metadata and the doc kept. Tests ported from camel-core (`tests/direct.rs`); still Java: `DirectNoConsumerTest` (`testWireTapFailIfNoConsumerFalse` needs wireTap), `DirectEndpointRouteInlinedTest`, `DirectNoToTypeTest`, `DirectQueueTest`, `DirectShouldUseSameThreadTest`, `DirectSynchronousTest`, `DirectProducerSuspendedConsumerTest`, `SendingToAlotOfDifferentDirectEndpointTest` (need mock / Rust DSL). Doc: Java tab replaced by a Rust tab. Generic endpoint options (`exchangePattern`, `bridgeErrorHandler`, `lazyStartProducer`) are not handled yet. |
| `components/camel-disruptor` | 17 | 69 | todo | |
| `components/camel-dns` | 13 | 10 | todo | |
| `components/camel-docker` | 14 | 49 | todo | |
| `components/camel-drill` | 6 | 3 | todo | |
| `components/camel-dropbox` | 27 | 10 | todo | |
| `components/camel-duckdb` | 11 | 4 | todo | |
| `components/camel-dynamic-router` | 18 | 22 | todo | |
| `components/camel-ehcache` | 10 | 16 | todo | |
| `components/camel-elasticsearch` | 9 | 11 | todo | |
| `components/camel-elasticsearch-rest-client` | 5 | 4 | todo | |
| `components/camel-event` | 5 | 10 | todo | |
| `components/camel-exec` | 13 | 14 | todo | |
| `components/camel-fastjson` | 1 | 4 | todo | |
| `components/camel-fhir` | 0 | 0 | todo | |
| `components/camel-fhir/camel-fhir-api` | 14 | 0 | todo | |
| `components/camel-fhir/camel-fhir-component` | 11 | 30 | todo | |
| `components/camel-file` | 67 | 3 | wip | Rust crate `camel-file` (`FileComponent`, `FileEndpoint`, `FileProducer`, `FileConsumer`, `GenericFile` with the `CamelFile*` headers): producer fileName/CamelOverruleFileName/CamelFileName/message id, fileExist (Override, Append, Fail, Ignore, Move with moveExisting (a directory keeps the name, `${file:parent}` is the target directory, eagerDeleteTargetFile), TryRename), tempPrefix/tempFileName, autoCreate, flatten, jailStartingDirectory, appendChars, allowNullBody, charset; polling consumer initialDelay/delay, noop (idempotent; idempotent, idempotentKey), delete, move (default `.camel`; a plain directory is relative to the file directory), preMove, moveFailed, include/exclude (case insensitive), includeExt/excludeExt, recursive, min/maxDepth, maxMessagesPerPoll (with the batch properties), fileName, hidden files, startingDirectoryMustExist/directoryMustExist; `FilePollingConsumer` (pollEnrich; the process strategy runs when the exchange is done; an in-progress set keeps a taken file from being taken again). Dynamic uris (toD, enrich, pollEnrich, wireTap) keep their `${file:..}` functions for the endpoint (Java skipFileFunctions, reifier `uri_expr`). Pending: read locks, shareUnitOfWork (FileMulticastDeleteTest), sortBy (but file:name and reverse:file:name), ant filters, checksum, permissions, the file language variants. Tests in `core/camel-core/tests/component/file` (Java tests in camel-core `component/file`). A regular file as the directory of a consumer is refused (Java CAMEL-24839). Producer forceWrites (fsync, default true) and keepLastModified; consumer sendEmptyMessageWhenIdle and the CamelMessageTimestamp header (the last modified). includeHiddenDirs; the producer keeps the CamelFileName header and uses CamelOverruleFileName once. Done files (doneFileName with `${file:name}` / `${file:name.noext}`: written by the producer, required and deleted by the consumer). |
| `components/camel-file-watch` | 7 | 7 | todo | |
| `components/camel-flatpack` | 10 | 23 | todo | |
| `components/camel-flink` | 14 | 5 | todo | |
| `components/camel-flowable` | 6 | 7 | todo | |
| `components/camel-fop` | 6 | 5 | todo | |
| `components/camel-fory` | 1 | 2 | todo | |
| `components/camel-freemarker` | 3 | 16 | todo | |
| `components/camel-ftp` | 33 | 333 | todo | |
| `components/camel-ftp-common` | 16 | 0 | todo | |
| `components/camel-geocoder` | 8 | 8 | todo | |
| `components/camel-git` | 12 | 5 | todo | |
| `components/camel-github2` | 19 | 6 | todo | |
| `components/camel-google` | 0 | 0 | todo | |
| `components/camel-google/camel-google-bigquery` | 13 | 22 | todo | |
| `components/camel-google/camel-google-calendar` | 15 | 12 | todo | |
| `components/camel-google/camel-google-common` | 2 | 1 | todo | |
| `components/camel-google/camel-google-drive` | 11 | 14 | todo | |
| `components/camel-google/camel-google-firestore` | 8 | 6 | todo | |
| `components/camel-google/camel-google-functions` | 7 | 7 | todo | |
| `components/camel-google/camel-google-mail` | 17 | 16 | todo | |
| `components/camel-google/camel-google-pubsub` | 14 | 25 | todo | |
| `components/camel-google/camel-google-secret-manager` | 10 | 5 | todo | |
| `components/camel-google/camel-google-sheets` | 18 | 10 | todo | |
| `components/camel-google/camel-google-speech-to-text` | 7 | 3 | todo | |
| `components/camel-google/camel-google-storage` | 10 | 19 | todo | |
| `components/camel-google/camel-google-text-to-speech` | 7 | 3 | todo | |
| `components/camel-google/camel-google-vertexai` | 9 | 7 | todo | |
| `components/camel-google/camel-google-vision` | 7 | 2 | todo | |
| `components/camel-graphql` | 3 | 16 | todo | |
| `components/camel-grok` | 2 | 6 | todo | |
| `components/camel-groovy` | 14 | 37 | todo | |
| `components/camel-grpc` | 30 | 20 | todo | |
| `components/camel-gson` | 2 | 17 | todo | |
| `components/camel-hashicorp-vault` | 9 | 11 | todo | |
| `components/camel-hazelcast` | 66 | 59 | todo | |
| `components/camel-hivemq` | 12 | 21 | todo | |
| `components/camel-hl7` | 28 | 26 | todo | |
| `components/camel-http` | 24 | 120 | todo | |
| `components/camel-http-base` | 13 | 5 | todo | |
| `components/camel-http-common` | 22 | 10 | todo | |
| `components/camel-huawei` | 0 | 0 | todo | |
| `components/camel-huawei/camel-huaweicloud-common` | 2 | 0 | todo | |
| `components/camel-huawei/camel-huaweicloud-dms` | 22 | 27 | todo | |
| `components/camel-huawei/camel-huaweicloud-frs` | 6 | 24 | todo | |
| `components/camel-huawei/camel-huaweicloud-functiongraph` | 8 | 17 | todo | |
| `components/camel-huawei/camel-huaweicloud-iam` | 6 | 20 | todo | |
| `components/camel-huawei/camel-huaweicloud-imagerecognition` | 6 | 13 | todo | |
| `components/camel-huawei/camel-huaweicloud-obs` | 11 | 39 | todo | |
| `components/camel-huawei/camel-huaweicloud-smn` | 9 | 15 | todo | |
| `components/camel-ibm` | 0 | 0 | todo | |
| `components/camel-ibm/camel-ibm-cos` | 7 | 10 | todo | |
| `components/camel-ibm/camel-ibm-secrets-manager` | 9 | 6 | todo | |
| `components/camel-ibm/camel-ibm-watson-discovery` | 6 | 3 | todo | |
| `components/camel-ibm/camel-ibm-watson-language` | 6 | 3 | todo | |
| `components/camel-ibm/camel-ibm-watson-speech-to-text` | 6 | 3 | todo | |
| `components/camel-ibm/camel-ibm-watson-text-to-speech` | 6 | 3 | todo | |
| `components/camel-ibm/camel-ibm-watsonx-ai` | 24 | 16 | todo | |
| `components/camel-ibm/camel-ibm-watsonx-data` | 6 | 1 | todo | |
| `components/camel-ical` | 2 | 1 | todo | |
| `components/camel-iggy` | 9 | 7 | todo | |
| `components/camel-ignite` | 35 | 11 | todo | |
| `components/camel-infinispan` | 0 | 0 | todo | |
| `components/camel-infinispan/camel-infinispan` | 23 | 22 | todo | |
| `components/camel-infinispan/camel-infinispan-common` | 17 | 4 | todo | |
| `components/camel-infinispan/camel-infinispan-embedded` | 15 | 25 | todo | |
| `components/camel-influxdb` | 7 | 7 | todo | |
| `components/camel-influxdb2` | 12 | 4 | todo | |
| `components/camel-iso8583` | 2 | 4 | todo | |
| `components/camel-jackson` | 14 | 53 | todo | |
| `components/camel-jackson-avro` | 6 | 10 | todo | |
| `components/camel-jackson-protobuf` | 6 | 9 | todo | |
| `components/camel-jackson3` | 14 | 53 | todo | |
| `components/camel-jackson3-avro` | 6 | 9 | todo | |
| `components/camel-jackson3-protobuf` | 6 | 8 | todo | |
| `components/camel-jackson3xml` | 5 | 30 | todo | |
| `components/camel-jacksonxml` | 5 | 31 | todo | |
| `components/camel-jactl` | 2 | 2 | todo | |
| `components/camel-jandex` | 1 | 0 | todo | |
| `components/camel-jasypt` | 2 | 10 | todo | |
| `components/camel-javascript` | 4 | 7 | todo | |
| `components/camel-jaxb` | 13 | 69 | todo | |
| `components/camel-jcache` | 19 | 23 | todo | |
| `components/camel-jcr` | 7 | 13 | todo | |
| `components/camel-jdbc` | 14 | 30 | todo | |
| `components/camel-jetty` | 5 | 227 | todo | |
| `components/camel-jetty-common` | 10 | 0 | todo | |
| `components/camel-jfr` | 16 | 8 | todo | |
| `components/camel-jgroups` | 9 | 12 | todo | |
| `components/camel-jgroups-raft` | 12 | 9 | todo | |
| `components/camel-jira` | 25 | 20 | todo | |
| `components/camel-jms` | 56 | 335 | todo | |
| `components/camel-jmx` | 9 | 24 | todo | |
| `components/camel-jolt` | 5 | 4 | todo | |
| `components/camel-jooq` | 6 | 11 | todo | |
| `components/camel-joor` | 15 | 22 | todo | |
| `components/camel-jpa` | 20 | 61 | todo | |
| `components/camel-jq` | 5 | 21 | todo | |
| `components/camel-jsch` | 8 | 5 | todo | |
| `components/camel-jslt` | 3 | 13 | todo | |
| `components/camel-json-validator` | 9 | 6 | todo | |
| `components/camel-jsonapi` | 1 | 5 | todo | |
| `components/camel-jsonata` | 5 | 6 | todo | |
| `components/camel-jsonb` | 1 | 10 | todo | |
| `components/camel-jsonpath` | 11 | 63 | todo | |
| `components/camel-jsoup` | 2 | 1 | todo | |
| `components/camel-jt400` | 13 | 18 | todo | |
| `components/camel-jta` | 4 | 4 | todo | |
| `components/camel-jte` | 5 | 5 | todo | |
| `components/camel-kafka` | 91 | 105 | todo | |
| `components/camel-kafka-common` | 19 | 3 | todo | |
| `components/camel-kafka-share` | 11 | 6 | todo | |
| `components/camel-kamelet` | 14 | 69 | todo | |
| `components/camel-keycloak` | 18 | 21 | todo | |
| `components/camel-knative` | 0 | 0 | todo | |
| `components/camel-knative/camel-knative-api` | 7 | 1 | todo | |
| `components/camel-knative/camel-knative-component` | 10 | 3 | todo | |
| `components/camel-knative/camel-knative-http` | 12 | 6 | todo | |
| `components/camel-kubernetes` | 102 | 85 | todo | |
| `components/camel-kudu` | 6 | 6 | todo | |
| `components/camel-language` | 4 | 0 | todo | |
| `components/camel-ldap` | 4 | 6 | todo | |
| `components/camel-ldif` | 3 | 8 | todo | |
| `components/camel-log` | 3 | 0 | wip | Rust crate `camel-log`: `LogComponent`/`LogEndpoint`/producer with the `DefaultExchangeFormatter` options, `level`, `groupSize` throughput logging and the context `LogListener`s; tests in camel-core `tests/component/log` (`LogEndpointTest`). Java kept until done: `groupInterval`/`groupDelay`/`groupActiveOnly`, `logMask`, `marker`, `exchangeFormatter`, `sourceLocationLoggerName`, stream/file options. |
| `components/camel-lra` | 7 | 13 | todo | |
| `components/camel-lucene` | 12 | 5 | todo | |
| `components/camel-lumberjack` | 13 | 7 | todo | |
| `components/camel-lzf` | 1 | 1 | todo | |
| `components/camel-mail` | 27 | 99 | todo | |
| `components/camel-mail-microsoft-oauth` | 1 | 0 | todo | |
| `components/camel-mapstruct` | 5 | 8 | todo | |
| `components/camel-master` | 3 | 7 | todo | |
| `components/camel-mdc` | 2 | 13 | todo | |
| `components/camel-metrics` | 20 | 29 | todo | |
| `components/camel-micrometer` | 38 | 38 | todo | |
| `components/camel-micrometer-observability` | 3 | 20 | todo | |
| `components/camel-micrometer-prometheus` | 3 | 0 | todo | |
| `components/camel-microprofile` | 0 | 0 | todo | |
| `components/camel-microprofile/camel-microprofile-config` | 1 | 1 | todo | |
| `components/camel-microprofile/camel-microprofile-fault-tolerance` | 6 | 23 | todo | |
| `components/camel-microprofile/camel-microprofile-health` | 4 | 8 | todo | |
| `components/camel-milo` | 26 | 20 | todo | |
| `components/camel-mina` | 12 | 61 | todo | |
| `components/camel-mina-sftp` | 13 | 78 | todo | |
| `components/camel-minio` | 8 | 14 | todo | |
| `components/camel-mllp` | 33 | 78 | todo | |
| `components/camel-mock` | 12 | 0 | wip | Rust crate `camel-mock`: `MockComponent`, `MockEndpoint` (count/minimum count, bodies in order / any order with type coercion, headers, properties, variables, values in any order, predicates per message / all messages, exchange pattern, ascending/descending/no duplicates, reply processors, reporter, retainFirst/Last, assertPeriod, resultWaitTime, resultMinimumWaitTime, sleepForEmptyTest, Java assertion messages). Tests in `tests/mock_endpoint.rs`. Java kept until done: `AssertionClause`/`MockValueBuilder`/`MockExpressionClause` (the Java clause builder; Rust uses `message_matches` / `all_messages_match` with `camel_support::builder` predicates), `TimeClause`, `expectedFileExists`, `InterceptSendToMockEndpointStrategy` (mock endpoints by advice), `failFast`, `browseLimit`, the exchange formatter. `MockEndpointTest` fully ported (77 tests; Java removed); the clause builder is written `message_matches(i, header("foo").is_equal_to(123))`. |
| `components/camel-mongodb` | 29 | 36 | todo | |
| `components/camel-mongodb-gridfs` | 7 | 3 | todo | |
| `components/camel-mustache` | 3 | 3 | todo | |
| `components/camel-mvel` | 8 | 9 | todo | |
| `components/camel-mybatis` | 12 | 38 | todo | |
| `components/camel-nats` | 8 | 31 | todo | |
| `components/camel-netty` | 52 | 90 | todo | |
| `components/camel-netty-http` | 46 | 161 | todo | |
| `components/camel-oaipmh` | 13 | 11 | todo | |
| `components/camel-oauth` | 36 | 17 | todo | |
| `components/camel-observability-services` | 0 | 0 | todo | |
| `components/camel-observation` | 5 | 16 | todo | |
| `components/camel-ocsf` | 2 | 2 | todo | |
| `components/camel-odata` | 7 | 3 | todo | |
| `components/camel-ognl` | 5 | 10 | todo | |
| `components/camel-olingo2` | 0 | 0 | todo | |
| `components/camel-olingo2/camel-olingo2-api` | 15 | 0 | todo | |
| `components/camel-olingo2/camel-olingo2-component` | 9 | 4 | todo | |
| `components/camel-olingo4` | 0 | 0 | todo | |
| `components/camel-olingo4/camel-olingo4-api` | 10 | 1 | todo | |
| `components/camel-olingo4/camel-olingo4-component` | 9 | 6 | todo | |
| `components/camel-once` | 3 | 11 | todo | |
| `components/camel-opa` | 16 | 17 | todo | |
| `components/camel-openapi-java` | 13 | 49 | todo | |
| `components/camel-openapi-validator` | 2 | 3 | todo | |
| `components/camel-openfeature` | 6 | 3 | todo | |
| `components/camel-openfga` | 15 | 10 | todo | |
| `components/camel-opensearch` | 9 | 11 | todo | |
| `components/camel-openstack` | 41 | 39 | todo | |
| `components/camel-opentelemetry` | 12 | 20 | todo | |
| `components/camel-opentelemetry-metrics` | 24 | 36 | todo | |
| `components/camel-opentelemetry2` | 7 | 34 | todo | |
| `components/camel-optaplanner` | 9 | 5 | todo | |
| `components/camel-paho` | 9 | 10 | todo | |
| `components/camel-paho-mqtt5` | 9 | 11 | todo | |
| `components/camel-parquet-avro` | 5 | 6 | todo | |
| `components/camel-pdf` | 21 | 8 | todo | |
| `components/camel-pg-replication-slot` | 3 | 3 | todo | |
| `components/camel-pgevent` | 7 | 3 | todo | |
| `components/camel-platform-http` | 20 | 19 | todo | |
| `components/camel-platform-http-jolokia` | 1 | 1 | todo | |
| `components/camel-platform-http-main` | 9 | 17 | todo | |
| `components/camel-platform-http-vertx` | 18 | 44 | todo | |
| `components/camel-plc4x` | 7 | 10 | todo | |
| `components/camel-pqc` | 58 | 91 | todo | |
| `components/camel-printer` | 9 | 1 | todo | |
| `components/camel-protobuf` | 3 | 5 | todo | |
| `components/camel-pubnub` | 6 | 8 | todo | |
| `components/camel-pulsar` | 25 | 27 | todo | |
| `components/camel-python` | 3 | 7 | todo | |
| `components/camel-python3` | 4 | 11 | todo | |
| `components/camel-quartz` | 20 | 70 | todo | |
| `components/camel-quickfix` | 12 | 8 | todo | |
| `components/camel-quickjs` | 4 | 11 | todo | |
| `components/camel-reactive-streams` | 27 | 32 | todo | |
| `components/camel-reactor` | 4 | 6 | todo | |
| `components/camel-redis` | 2 | 5 | todo | |
| `components/camel-ref` | 2 | 0 | todo | |
| `components/camel-resilience4j` | 5 | 38 | todo | |
| `components/camel-resilience4j-micrometer` | 1 | 1 | todo | |
| `components/camel-resourceresolver-github` | 4 | 2 | todo | |
| `components/camel-rest` | 10 | 10 | todo | |
| `components/camel-rest-openapi` | 14 | 23 | todo | |
| `components/camel-rest-postman` | 32 | 15 | todo | |
| `components/camel-robotframework` | 6 | 3 | todo | |
| `components/camel-rocketmq` | 14 | 6 | todo | |
| `components/camel-rss` | 10 | 14 | todo | |
| `components/camel-rxjava` | 4 | 6 | todo | |
| `components/camel-saga` | 4 | 0 | todo | |
| `components/camel-salesforce` | 0 | 0 | todo | |
| `components/camel-salesforce/camel-salesforce-codegen` | 6 | 0 | todo | |
| `components/camel-salesforce/camel-salesforce-component` | 208 | 96 | todo | |
| `components/camel-salesforce/camel-salesforce-maven-plugin` | 4 | 21 | todo | |
| `components/camel-sap-netweaver` | 4 | 3 | todo | |
| `components/camel-saxon` | 9 | 49 | todo | |
| `components/camel-scheduler` | 4 | 0 | todo | |
| `components/camel-schematron` | 10 | 7 | todo | |
| `components/camel-seda` | 13 | 0 | wip | Rust crate `camel-seda`: shared per-uri queues with size, concurrent consumer tasks (tokio), waitForTaskToComplete with timeout (ExchangeTimedOutException) and the results copied back, blockWhenFull/offerTimeout/discardWhenFull/failIfNoConsumers/discardIfNoConsumers/purgeWhenStopping, handover of the on completions to the queued copy (InOnly), Java messages. Tests in camel-core `tests/component/seda`. Java kept until done: multipleConsumers, custom queues/factories, polling consumer, pollTimeout, browse, virtual threads; Java tests left: those needing delay, the unit of work, suspend/resume of consumers, multicast, ... |
| `components/camel-servicenow` | 0 | 0 | todo | |
| `components/camel-servicenow/camel-servicenow-component` | 41 | 12 | todo | |
| `components/camel-servicenow/camel-servicenow-maven-plugin` | 1 | 2 | todo | |
| `components/camel-servlet` | 6 | 58 | todo | |
| `components/camel-shell` | 3 | 3 | todo | |
| `components/camel-shiro` | 6 | 7 | todo | |
| `components/camel-sjms` | 45 | 89 | todo | |
| `components/camel-sjms2` | 4 | 10 | todo | |
| `components/camel-slack` | 8 | 4 | todo | |
| `components/camel-smb` | 18 | 70 | todo | |
| `components/camel-smooks` | 11 | 15 | todo | |
| `components/camel-smpp` | 29 | 30 | todo | |
| `components/camel-snakeyaml` | 2 | 13 | todo | |
| `components/camel-snmp` | 13 | 9 | todo | |
| `components/camel-soap` | 12 | 27 | todo | |
| `components/camel-solr` | 9 | 5 | todo | |
| `components/camel-spiffe` | 11 | 9 | todo | |
| `components/camel-splunk-hec` | 5 | 5 | todo | |
| `components/camel-spring-parent` | 0 | 0 | todo | |
| `components/camel-spring-parent/camel-spring` | 32 | 0 | todo | |
| `components/camel-spring-parent/camel-spring-ai` | 0 | 0 | todo | |
| `components/camel-spring-parent/camel-spring-ai/camel-spring-ai-chat` | 9 | 29 | todo | |
| `components/camel-spring-parent/camel-spring-ai/camel-spring-ai-embeddings` | 6 | 3 | todo | |
| `components/camel-spring-parent/camel-spring-ai/camel-spring-ai-image` | 7 | 1 | todo | |
| `components/camel-spring-parent/camel-spring-ai/camel-spring-ai-vector-store` | 7 | 2 | todo | |
| `components/camel-spring-parent/camel-spring-batch` | 8 | 8 | todo | |
| `components/camel-spring-parent/camel-spring-cloud-config` | 4 | 0 | todo | |
| `components/camel-spring-parent/camel-spring-jdbc` | 3 | 3 | todo | |
| `components/camel-spring-parent/camel-spring-ldap` | 4 | 3 | todo | |
| `components/camel-spring-parent/camel-spring-main` | 1 | 12 | todo | |
| `components/camel-spring-parent/camel-spring-rabbitmq` | 19 | 22 | todo | |
| `components/camel-spring-parent/camel-spring-redis` | 13 | 20 | todo | |
| `components/camel-spring-parent/camel-spring-security` | 6 | 2 | todo | |
| `components/camel-spring-parent/camel-spring-ws` | 22 | 54 | todo | |
| `components/camel-spring-parent/camel-spring-xml` | 29 | 824 | todo | |
| `components/camel-spring-parent/camel-undertow-spring-security` | 5 | 5 | todo | |
| `components/camel-sql` | 48 | 156 | todo | |
| `components/camel-ssh` | 13 | 16 | todo | |
| `components/camel-state-store` | 0 | 0 | todo | |
| `components/camel-state-store/camel-state-store` | 5 | 4 | todo | |
| `components/camel-stax` | 10 | 32 | todo | |
| `components/camel-stitch` | 18 | 13 | todo | |
| `components/camel-stream` | 8 | 16 | todo | |
| `components/camel-stringtemplate` | 3 | 7 | todo | |
| `components/camel-stripe` | 6 | 7 | todo | |
| `components/camel-stub` | 5 | 0 | todo | |
| `components/camel-swift` | 5 | 4 | todo | |
| `components/camel-syslog` | 9 | 12 | todo | |
| `components/camel-tahu` | 19 | 9 | todo | |
| `components/camel-tarfile` | 5 | 16 | todo | |
| `components/camel-telegram` | 151 | 46 | todo | |
| `components/camel-telemetry` | 113 | 94 | todo | |
| `components/camel-telemetry-dev` | 5 | 15 | todo | |
| `components/camel-test` | 0 | 0 | todo | |
| `components/camel-test/camel-test-junit5` | 38 | 39 | todo | |
| `components/camel-test/camel-test-junit6` | 38 | 38 | todo | |
| `components/camel-test/camel-test-main-junit5` | 9 | 51 | todo | |
| `components/camel-test/camel-test-main-junit6` | 9 | 51 | todo | |
| `components/camel-test/camel-test-spring-junit5` | 26 | 35 | todo | |
| `components/camel-test/camel-test-spring-junit6` | 27 | 35 | todo | |
| `components/camel-thrift` | 15 | 23 | todo | |
| `components/camel-thymeleaf` | 4 | 24 | todo | |
| `components/camel-tika` | 6 | 3 | todo | |
| `components/camel-timer` | 4 | 0 | wip | Rust crate `camel-timer` on a tokio task: period/delay (millis or duration text), repeatCount, fixedRate, negative delay, includeMetadata, time (default patterns), timerName, runLoggingLevel; graceful stop waits for the message in flight. Pending: `pattern`, context suspend, `exceptionHandler`. Tests: 14 camel-core timer tests (`tests/component/timer`). |
| `components/camel-toon` | 1 | 1 | todo | |
| `components/camel-tracing` | 63 | 27 | todo | |
| `components/camel-twilio` | 8 | 3 | todo | |
| `components/camel-twitter` | 32 | 16 | todo | |
| `components/camel-ubl` | 1 | 1 | todo | |
| `components/camel-undertow` | 32 | 109 | todo | |
| `components/camel-univocity-parsers` | 7 | 18 | todo | |
| `components/camel-validator` | 6 | 0 | todo | |
| `components/camel-velocity` | 5 | 21 | todo | |
| `components/camel-vertx` | 0 | 0 | todo | |
| `components/camel-vertx/camel-vertx` | 4 | 10 | todo | |
| `components/camel-vertx/camel-vertx-common` | 6 | 0 | todo | |
| `components/camel-vertx/camel-vertx-http` | 12 | 39 | todo | |
| `components/camel-vertx/camel-vertx-websocket` | 15 | 24 | todo | |
| `components/camel-wal` | 16 | 10 | todo | |
| `components/camel-wasm` | 9 | 3 | todo | |
| `components/camel-weather` | 14 | 9 | todo | |
| `components/camel-web3j` | 7 | 12 | todo | |
| `components/camel-webhook` | 9 | 10 | todo | |
| `components/camel-whatsapp` | 45 | 5 | todo | |
| `components/camel-wordpress` | 80 | 16 | todo | |
| `components/camel-workday` | 8 | 2 | todo | |
| `components/camel-xchange` | 8 | 4 | todo | |
| `components/camel-xj` | 13 | 13 | todo | |
| `components/camel-xmlsecurity` | 37 | 20 | todo | |
| `components/camel-xmpp` | 11 | 13 | todo | |
| `components/camel-xpath` | 8 | 0 | todo | |
| `components/camel-xslt` | 25 | 0 | todo | |
| `components/camel-xslt-saxon` | 6 | 27 | todo | |
| `components/camel-zendesk` | 8 | 2 | todo | |
| `components/camel-zip-deflater` | 2 | 6 | todo | |
| `components/camel-zipfile` | 5 | 22 | todo | |
| `components/camel-zookeeper` | 30 | 23 | todo | |
| `components/camel-zookeeper-master` | 32 | 8 | todo | |
| `test-infra/camel-test-infra-all` | 0 | 0 | todo | |
| `test-infra/camel-test-infra-arangodb` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-artemis` | 22 | 0 | todo | |
| `test-infra/camel-test-infra-aws-common` | 5 | 0 | todo | |
| `test-infra/camel-test-infra-aws-v2` | 27 | 0 | todo | |
| `test-infra/camel-test-infra-azure-common` | 8 | 0 | todo | |
| `test-infra/camel-test-infra-azure-storage-blob` | 4 | 0 | todo | |
| `test-infra/camel-test-infra-azure-storage-datalake` | 3 | 0 | todo | |
| `test-infra/camel-test-infra-azure-storage-queue` | 4 | 0 | todo | |
| `test-infra/camel-test-infra-cassandra` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-chatscript` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-cli` | 9 | 0 | todo | |
| `test-infra/camel-test-infra-clickhouse` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-common` | 18 | 1 | todo | |
| `test-infra/camel-test-infra-consul` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-core` | 16 | 0 | todo | |
| `test-infra/camel-test-infra-couchbase` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-couchdb` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-cyberark-vault` | 5 | 0 | todo | |
| `test-infra/camel-test-infra-dispatch-router` | 1 | 0 | todo | |
| `test-infra/camel-test-infra-docling` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-duckdb` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-elasticsearch` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-fhir` | 8 | 0 | todo | |
| `test-infra/camel-test-infra-ftp` | 15 | 0 | todo | |
| `test-infra/camel-test-infra-google-pubsub` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-hashicorp-vault` | 5 | 0 | todo | |
| `test-infra/camel-test-infra-hazelcast` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-hivemq` | 8 | 0 | todo | |
| `test-infra/camel-test-infra-ibmmq` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-iggy` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-ignite` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-infinispan` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-jaeger` | 7 | 3 | todo | |
| `test-infra/camel-test-infra-jdbc` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-jetty` | 5 | 0 | todo | |
| `test-infra/camel-test-infra-kafka` | 13 | 0 | todo | |
| `test-infra/camel-test-infra-keycloak` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-mcp-everything` | 9 | 0 | todo | |
| `test-infra/camel-test-infra-messaging-common` | 9 | 0 | todo | |
| `test-infra/camel-test-infra-microprofile-lra` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-milvus` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-minio` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-mongodb` | 8 | 1 | todo | |
| `test-infra/camel-test-infra-mosquitto` | 8 | 0 | todo | |
| `test-infra/camel-test-infra-nats` | 10 | 0 | todo | |
| `test-infra/camel-test-infra-neo4j` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-observability` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-ollama` | 14 | 1 | todo | |
| `test-infra/camel-test-infra-opa` | 7 | 1 | todo | |
| `test-infra/camel-test-infra-openai-mock` | 44 | 0 | todo | |
| `test-infra/camel-test-infra-openfga` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-openldap` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-opensearch` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-parent` | 0 | 0 | todo | |
| `test-infra/camel-test-infra-pinecone` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-postgres` | 10 | 0 | todo | |
| `test-infra/camel-test-infra-pulsar` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-qdrant` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-rabbitmq` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-redis` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-rocketmq` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-smb` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-solr` | 8 | 0 | todo | |
| `test-infra/camel-test-infra-spiffe` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-tensorflow-serving` | 5 | 0 | todo | |
| `test-infra/camel-test-infra-triton` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-typesafe-ai-mock` | 1 | 1 | todo | |
| `test-infra/camel-test-infra-weaviate` | 6 | 0 | todo | |
| `test-infra/camel-test-infra-wolf-defender` | 1 | 1 | todo | |
| `test-infra/camel-test-infra-xmpp` | 7 | 0 | todo | |
| `test-infra/camel-test-infra-zookeeper` | 7 | 0 | todo | |
| `catalog/camel-allcomponents` | 0 | 0 | todo | |
| `catalog/camel-catalog` | 9 | 9 | todo | |
| `catalog/camel-catalog-common` | 2 | 0 | todo | |
| `catalog/camel-catalog-console` | 1 | 0 | todo | |
| `catalog/camel-catalog-maven` | 5 | 3 | todo | |
| `catalog/camel-catalog-suggest` | 1 | 1 | todo | |
| `catalog/camel-report-maven-plugin` | 20 | 21 | todo | |
| `catalog/camel-route-parser` | 26 | 58 | todo | |
| `catalog/dummy-component` | 4 | 0 | todo | |
| `tooling/camel-exe` | 0 | 1 | todo | |
| `tooling/camel-tooling-maven` | 9 | 5 | todo | |
| `tooling/camel-tooling-model` | 27 | 5 | todo | |
| `tooling/camel-tooling-util` | 15 | 5 | todo | |
| `tooling/camel-util-json` | 8 | 5 | todo | |
| `tooling/enforcer-rules` | 0 | 0 | todo | |
| `tooling/maven` | 0 | 0 | todo | |
| `tooling/maven/bom-generator-maven-plugin` | 5 | 0 | todo | |
| `tooling/maven/camel-api-component-maven-plugin` | 14 | 12 | todo | |
| `tooling/maven/camel-component-maven-plugin` | 1 | 0 | todo | |
| `tooling/maven/camel-eip-documentation-enricher-maven-plugin` | 6 | 6 | todo | |
| `tooling/maven/camel-maven-plugin` | 5 | 0 | todo | |
| `tooling/maven/camel-package-maven-plugin` | 77 | 37 | todo | |
| `tooling/maven/camel-repackager-maven-plugin` | 1 | 1 | todo | |
| `tooling/maven/camel-restdsl-openapi-plugin` | 7 | 0 | todo | |
| `tooling/maven/sync-properties-maven-plugin` | 1 | 0 | todo | |
| `tooling/openapi-rest-dsl-generator` | 16 | 15 | todo | |
| `tooling/parent` | 0 | 0 | todo | |
| `tooling/spi-annotations` | 35 | 0 | todo | |
| `apache-camel` | 0 | 0 | todo | |
| `archetypes` | 0 | 0 | todo | |
| `archetypes/camel-archetype-api-component` | 9 | 0 | todo | |
| `archetypes/camel-archetype-component` | 6 | 0 | todo | |
| `archetypes/camel-archetype-dataformat` | 2 | 0 | todo | |
| `archetypes/camel-archetype-java` | 2 | 0 | todo | |
| `archetypes/camel-archetype-main` | 5 | 0 | todo | |
| `bom` | 0 | 0 | todo | |
| `bom/camel-bom` | 0 | 0 | todo | |
| `buildingtools` | 0 | 0 | todo | |
| `camel-dependencies` | 0 | 0 | todo | |
| `catalog` | 0 | 0 | todo | |
| `components` | 0 | 0 | todo | |
| `core` | 0 | 0 | todo | |
| `coverage` | 0 | 0 | todo | |
| `docs` | 0 | 0 | todo | |
| `dsl` | 0 | 0 | todo | |
| `etc` | 0 | 0 | todo | |
| `parent` | 0 | 0 | todo | |
| `test-infra` | 0 | 0 | todo | |
| `tests` | 0 | 0 | todo | |
| `tests/camel-itest` | 0 | 65 | todo | |
| `tests/camel-streamcaching-test` | 0 | 1 | todo | |
| `tests/test-bundles` | 0 | 0 | todo | |
| `tests/test-bundles/camel-validator-test-resources` | 0 | 0 | todo | |
| `tooling` | 0 | 0 | todo | |
