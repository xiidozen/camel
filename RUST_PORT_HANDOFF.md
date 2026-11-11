# Rust port — handoff instructions

This document is for whoever (human or AI agent) continues the conversion of Apache Camel
from Java to Rust on the `rust` branch. Read it fully before changing anything, then read
[`RUST_PORT_BACKLOG.md`](RUST_PORT_BACKLOG.md), which holds the per-module status, the ground
rules and the Java → Rust type mapping.

## 1. Goal and agreed scope

- The repository owner asked for the **entire repository** to be converted to Rust, across as
  many sessions as needed, including all unit tests and documentation.
- Work happens on the branch **`rust`**, pushed to **`origin`** (the owner's fork). Never push
  to `upstream` (`apache/camel`).
- **Ported Java code is deleted** in the same commit as its Rust replacement (the owner's
  explicit choice). The Maven build on this branch is therefore intentionally broken; only the
  Cargo workspace must build and pass.
- Push often (after every finished group of classes) so progress is visible on GitHub.
- **Routes are written in Rust, YAML or XML.** The owner decided the Rust framework does not keep
  the Java DSL: build a Rust route DSL (same EIP vocabulary as the Java `RouteBuilder`, in
  snake_case), keep the YAML and XML DSLs, mark Java-DSL-only modules (`core/camel-java-io`,
  `dsl/camel-java-joor-dsl`) as `n/a`, and rewrite Java route examples in docs as Rust.

## 2. Environment setup

```bash
# Rust toolchain (stable, edition 2021, MSRV 1.80); rustfmt and clippy are required
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c rustfmt,clippy
source ~/.cargo/env

git checkout rust && git pull
# git identity used so far for this branch (repository-local config)
git config user.name  "<the owner's GitHub login>"   # look it up: gh api /user --jq .login
git config user.email "<the owner's email>"          # ask the owner if unknown; never guess

./etc/rust/check.sh   # must pass before you start, and before every push
```

`etc/rust/check.sh` runs, in order: `cargo fmt --check`, `cargo build --all-targets`,
`cargo test`, `cargo clippy --all-targets -- -D warnings`, and
`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` for the whole workspace.

## 3. Where things are

| Path | What |
| --- | --- |
| `Cargo.toml` (root) | Cargo workspace; add every new crate to `members` and `[workspace.dependencies]` |
| `core/camel-util/` | First crate (`camel-util`), being ported in place of the Java module |
| `core/camel-util/src/*.rs` | One Rust module per Java class (or group of classes); `lib.rs` lists them alphabetically |
| `core/camel-util/tests/resources/` | Test resources moved from `src/test/resources` |
| `RUST_PORT_BACKLOG.md` | Status of all 703 Maven modules, ground rules, Java → Rust mapping |
| `etc/rust/check.sh` | The quality gate |

Crates live **in the directory of the Maven module they replace** (`core/camel-api/Cargo.toml`,
`core/camel-api/src/lib.rs`, ...). Crate name = Maven artifactId (`camel-api` → `camel_api`).

## 4. Current state (keep this section up to date)

- `core/camel-util`: **done** — all Java removed (Maven module deleted from `core/pom.xml`);
  ~430 unit tests, 16 integration tests (`tests/util_edge_cases.rs`) and ~90 doctests. Not
  applicable (reflection / class loading / Java serialization): `AnnotationHelper`,
  `ReflectionHelper`, `CastUtils`, `BufferCaster`, `ClassLoadingAwareObjectInputStream`,
  `PackageHelper`, the `java21`/`java25` multi-release sources and their tests.
- `core/camel-core/src/test/java/org/apache/camel/util/ObjectHelperTest.java` is kept on
  purpose: its `camel-util` parts are ported, the `camel-support` `ObjectHelper` parts are not.
- `core/camel-api`: **in progress** — done: enums, `CamelError`/`ErrorKind`, Exchange constants,
  `Message`, `Exchange`, `convert::FromValue`, async `Processor`, `Expression`, `Predicate`,
  `AggregationStrategy`, minimal `CamelContext`, `spi::uuid`, `Service` lifecycle + `ServiceState`,
  `Endpoint`/`Component`/`Producer`/`Consumer`/`PollingConsumer`, `Route`, `spi::{Registry, Language,
  Synchronization, HeaderFilterStrategy}`. The rest of `camel-api` (mostly `spi/`) is ported in
  **vertical slices**: each interface together with the engine/support code that implements it,
  so no trait is ported without a user and a test.
- `core/camel-support`: **in progress** (registry, header filter strategy, pattern helper).
- `core/camel-core-processor`: **in progress** — pipeline, filter, choice, message translators,
  stop, throw exception, log and send processors (`tests/processors.rs`); `multicast` (sequential
  multicast, split, delay), `dynamic` (toD, recipient list, wire tap, convertBodyTo,
  setExchangePattern, `ProducerCache`), `aggregate` (the aggregate EIP, strategies, memory
  repository; spi `AggregationRepository` in camel-api). Processor lifecycle:
  `Processor::start_processor`/`stop_processor` (default no-op, idempotent; named so to not
  clash with `Service::start` on producers); composites forward to their children and the
  route starts its processors before the consumer and stops them after it. The aggregator's
  timeout/interval checkers are tokio tasks started with the route and aborted on stop. Parallel processing,
  streaming and timeouts of multicast/split/recipient list are pending.
- `core/camel-base-engine`: **in progress** — `DefaultCamelContext`, `DefaultRoute`, `RouteSpec`,
  `ProducerTemplate` on tokio (`tests/context.rs`).
- `components/camel-direct`: **done** (first component; use it as the template for porting
  components: crate in the component folder, `create_endpoint` taking options with
  `component_support::get_and_remove_parameter`, `Weak` context in the endpoint, tests ported
  from camel-core into `tests/`).
- `components/camel-mock`: **in progress** — `MockEndpoint` with the common expectations; all of
  `MockEndpointTest` ported (`tests/mock_endpoint.rs`). Time clauses, file expectations and
  advice-based mocking are pending.
- `core/camel-core-model`, `camel-core-reifier`, `camel-core-engine`: **in progress** — the model,
  the Rust route DSL (`camel_core_model::builder`), the reifier and `ModelCamelContext::add_routes`.
- `core/camel-core`: **in progress** — facade crate (`default_context()`, `prelude`) and the home of
  the ported camel-core tests (`tests/<package>/main.rs`, `tests/support`).
- `core/camel-core-languages`: **in progress** — the simple language parsers and core functions
  (see the backlog row for what is pending), and the constant/header/exchangeProperty/variable
  languages; registered by `camel_core::default_context()`.
- Error handling: `doTry`, the default error handler, the dead letter channel and `onException`
  are ported (`camel_core_processor::errorhandler`; the reifier wraps each step as Java channels do).
- `components/camel-log`: **in progress** (formatter options, level, groupSize, log listeners).
- `components/camel-seda`: **in progress** (queues, concurrent consumers, InOut waiting).
- Test helpers in `core/camel-core/tests/support`: `ContextTestSupport`, `cause_of` (Java
  `getCause()`), `await_until` (Awaitility; never a bare sleep loop), `body_in_aggregating_strategy`.
- Unit of work: the exchange carries it (`Exchange::begin_unit_of_work`, `original_message`,
  `add_on_completion`, `handover_completions`); `camel_core_processor::unit_of_work::UnitOfWorkProcessor`
  wraps every route (on before/after route callbacks, done synchronizations FILO + by order).
  `Synchronization` is async and merges Java `SynchronizationVetoable` and
  `SynchronizationRouteAware`. The onCompletion EIP (`on_completion` processor; route scoped and
  route builder scoped, onCompleteOnly/onFailureOnly, onWhen, useOriginalMessage, modes
  before/after consumer, parallelProcessing) is ported. Pending: onCompletion with intercept,
  custom executor, shutdown awareness, seda handover tests.
- The error handler and `onException` support `use_original_message`/`use_original_body`.
- **Next:** the remaining onCompletion tests, parallel processing for multicast/split/recipient list, the
  rest of the simple functions, then camel-main and the YAML/XML DSLs.
- Java files still present in `core/camel-api/src/main/java` are the ones not ported yet.
- Everything else in the repository: **not started** (status `todo` in the backlog).

## 5. Order of work

Bottom-up by dependency, so each crate only depends on already-ported crates:

1. finish `core/camel-util`
2. `core/camel-api` (Exchange, Message, CamelContext, Endpoint, Component, Processor, Producer,
   Consumer, Expression, Predicate, Route, TypeConverter, ... — traits) — design the async
   processing model here (see §8)
3. `core/camel-support`, `core/camel-base`, `core/camel-base-engine`, `core/camel-management-api`
4. `core/camel-core-model`, `camel-core-processor`, `camel-core-reifier`, `camel-core-languages`
   (simple language), `camel-core-engine`
5. `core/camel-core` tests (3179 Java test files; port them as the features they test exist)
6. `core/camel-main`, the XML/YAML IO modules, `dsl/*`: the **Rust route DSL** replaces the Java DSL
   (Java-DSL-only modules are n/a); the YAML and XML DSLs parse into the same model; the endpoint DSL
   becomes generated typed Rust endpoint builders
7. `components/*` (start with dependency-free ones: direct, seda, timer, log, mock, file,
   bean, ...), then `test-infra`, `tooling`, `catalog`, `docs`

## 6. How to port one class (the loop)

1. **Read the Java class and all its tests** (in its module *and* in
   `core/camel-core/src/test/java/...` — many util tests live there).
2. Write `src/<snake_case_name>.rs`:
   - same behaviour as Java, including edge cases the tests pin down;
   - Java `null` → `Option`, checked/illegal-argument exceptions → `Result` (error message text
     kept identical to Java where tests assert on it);
   - `Object` → `camel_util::value::Value`; `Map<String, Object>` → `ValueMap` (insertion
     ordered) or `CaseInsensitiveMap` for headers;
   - **full rustdoc** converted from the javadoc (see §7);
   - translate every Java test into `#[cfg(test)] mod tests` (or `src/<module>/tests.rs` when
     large) keeping the Java test names in snake_case.
3. Add the module to `lib.rs` (alphabetical), run `cargo fmt --all` and `./etc/rust/check.sh`.
4. `git rm` the ported Java sources **and** tests; update the module row in the backlog.
5. Commit and push (see §9).

## 7. Documentation rules (the owner asked for this explicitly)

Javadoc must be converted so `cargo doc` produces equivalent documentation:

- summary + description → first lines of `///`; class/package docs → `//!` module docs
- `@param` → `# Arguments` list, `@return` → `# Returns`, `@throws` → `# Errors` / `# Panics`
- `<pre>` / `{@code}` examples → `# Examples` with a compiling doctest where practical
- `{@link Foo}` → intra-doc link ``[`Foo`]``; `@deprecated` → `#[deprecated(since, note)]`;
  `@since X.Y` → a `*Since Camel X.Y*` line
- every crate: `#![deny(missing_docs)]` and `#![deny(rustdoc::broken_intra_doc_links)]`
- also update user-facing docs (`docs/`, component `src/main/docs/*.adoc`) when the module they
  describe is ported, replacing Java examples with Rust ones.

## 8. Design decisions already taken (do not re-litigate)

| Java | Rust |
| --- | --- |
| `java.lang.Object` | `camel_util::value::Value` (+ `Value::Object` for any `Send + Sync` value, downcastable) |
| JVM system properties | `camel_util::system_properties` (registry + env fallback + built-in `java.io.tmpdir`, `user.dir`, ...) |
| `java.net.URI`, `URLEncoder`, `URLDecoder` | `camel_util::net` (exact JDK semantics — endpoint URI normalization depends on it) |
| `java.time.Duration` | `camel_util::time_utils::SignedDuration` |
| `java.nio.charset.Charset` | `camel_util::charset::Charset` (Java names; ISO-8859-1 is real Latin-1, not windows-1252) |
| `java.util.Properties` file format | `camel_util::properties::parse_properties` |
| headers map | `camel_util::case_insensitive_map::CaseInsensitiveMap<Value>` |
| reflection, class loading, Java serialization | not ported; explicit registration instead |
| Java DSL (`RouteBuilder`) | Rust route DSL; routes in Rust, YAML or XML only |
| internal locks in collections | `&mut self` APIs; callers wrap in `Mutex` when shared |

Processing model (decided in `camel-api`): `Processor::process(&self, &mut Exchange)` returns a
boxed `Send` future (`processor::BoxFuture`) — this replaces `AsyncProcessor` + `AsyncCallback`;
errors are returned (`CamelResult`) and the engine stores them on the exchange. `Exchange` and
`Message` are concrete structs (Java's `DefaultExchange`/`DefaultMessage` merged in). Expressions
and predicates are synchronous. The engine will run on `tokio`; `processor::block_on` exists for
tests and synchronous callers.

### Component specific exceptions

`ErrorKind` only has the camel-api exceptions. A component exception (such as
`DirectConsumerNotAvailableException`) uses the `ErrorKind` of its Java superclass plus
`CamelError::with_subtype("<full Java class name>")`; test it with `is_subtype("SimpleName")`.
Keep the Java message text.

### Error types

Java catches errors by class (`doCatch(IOException.class)`, `onException(...)`). Rust uses
`camel_api::error::ErrorType`: `Any` (Java `Exception`/`Throwable`), `Kind(ErrorKind)` (with the
kind hierarchy), or `Class(name)` for other Java classes (component subtypes, Rust I/O errors for
`java.io.IOException`). `ErrorType::from_java_name` maps the Java names used by YAML/XML routes.
The caught error is stored in the `CamelExceptionCaught` property as `Value::object_display`
of the `CamelError` (`camel_core_processor::try_catch::exception_caught` reads it back).

### Async runtime

`camel-api` has no runtime dependency (its `block_on` is a minimal executor for tests). The
engine and the components use tokio; tests of engine-level behaviour use `#[tokio::test]`.

### Component docs

When a component is ported, change the `Java::` tab of the examples in its
`src/main/docs/*.adoc` to a `Rust::` tab with `[source,rust]` written with the Rust DSL
(`routes.add(from(..)...)`), keeping the XML and YAML tabs. The mirrored copy under
`catalog/camel-catalog/src/generated/resources/.../docs/` is regenerated by the tooling
phase (see the backlog).

### Open design question: mutating expressions

`Expression::evaluate` takes `&Exchange`, but a few Java simple functions change the exchange
while evaluating (`setHeader`, `setVariable`, `listAdd`, `listRemove`, `mapAdd`, `mapRemove`).
They currently fail with "not supported yet by the Rust port". Options: an
`evaluate_mut(&mut Exchange)` default method on `Expression` used by processors that own the
exchange (setBody, transform, log, ...), or recording the changes and applying them after the
evaluation. Decide before porting those functions.

## 9. Commits, pushes and quality bar

- Commit message: `Rust port: <module> <what>` + a body listing what was ported and what was
  dropped as not applicable, ending with the `Co-Authored-By:` trailer of the AI used.
- Never push with a failing `./etc/rust/check.sh`: chain it as `./etc/rust/check.sh && git commit ... && git push`
  so a failure stops the commit. Never use `git push --force`.
- Push to `origin rust` after every finished group.

## 10. Pitfalls found so far

- **Scripted edits after `cargo fmt`:** a Python `str.replace` with text written before
  formatting silently does nothing once rustfmt has reflowed the code. Use a replacement that
  fails when the text is not found exactly once (or the Edit tool), and rerun the tests.

- **Match Java exactly, then test it.** Several "bugs" were wrong expectations about Java:
  `"2 secs"` is not a valid time pattern; a `.properties` continuation line drops its leading
  whitespace; `String.split` drops trailing empty strings. When unsure, reproduce the Java
  algorithm literally (see `lang.rs` for `java_trim`, `java_split`, identifier checks).
- Java string indices are UTF-16; the port uses `char` indices where Java indexes text, and byte
  slicing only at ASCII delimiters.
- Rust runs tests **in parallel**: tests touching global state (sanitize keywords, known header
  keys, env vars) use a `static Mutex` guard and reset the state on drop.
- Do not compare `&dyn Error` addresses for identity: a zero-sized cause shares its wrapper's
  address.
- `cargo fmt` rewrites files after you edit them; re-read before doing exact-text edits.
- Generated lists (sensitive keys, important headers, simple functions, mime types) keep their
  `START`/`END` markers so the future Rust tooling can regenerate them.

## 11. Updating this document

Update §4 (current state) and the backlog in the same commit as every port, so a new session
can always resume from the latest pushed commit.
