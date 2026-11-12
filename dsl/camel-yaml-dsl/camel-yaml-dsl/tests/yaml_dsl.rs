//! Ports of the YAML DSL tests (`dsl/camel-yaml-dsl/camel-yaml-dsl/src/test`): the model
//! created from the YAML (asserted with the Java `toString` forms of the Rust model) and the
//! routes running.

use camel_core::prelude::*;
use camel_yaml_dsl::YamlRoutes;
use std::sync::Arc;

fn steps(yaml: &str) -> Vec<String> {
    let routes = YamlRoutes::parse(yaml).unwrap();
    routes.route_definitions()[0]
        .steps
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn parse_error(yaml: &str) -> String {
    match YamlRoutes::parse(yaml) {
        Ok(_) => panic!("Should have thrown exception"),
        Err(e) => e.to_string(),
    }
}

async fn start(yaml: &str) -> Arc<DefaultCamelContext> {
    let routes = YamlRoutes::parse(yaml).unwrap();
    let context = camel_core::default_context();
    routes.bind_beans(context.as_ref()).unwrap();
    context.add_routes(routes).await.unwrap();
    context.start().await.unwrap();
    context
}

fn mock(context: &DefaultCamelContext, uri: &str) -> MockEndpoint {
    MockEndpoint::resolve(context, uri).unwrap()
}

#[test]
fn to_definition() {
    for yaml in [
        "- from:\n    uri: direct:start\n    steps:\n      - to: \"direct:start\"\n",
        "- from:\n    uri: direct:start\n    steps:\n      - to:\n          uri: \"direct:start\"\n",
        "- from:\n    uri: direct:start\n    steps:\n      - to:\n          uri: direct\n          parameters:\n            name: start\n",
        "- from:\n    uri: direct:start\n    steps:\n      - to:\n          parameters:\n            name: start\n          uri: direct\n",
    ] {
        let s = steps(yaml);
        // a uri with the name as a parameter (Java normalizes direct?name=start as direct:start)
        assert!(s[0] == "to[direct:start]" || s[0] == "to[direct?name=start]", "{s:?}");
    }
}

#[test]
fn loop_definition() {
    for yaml in [
        r#"
- from:
    uri: "direct:start"
    steps:
      - loop:
         constant: "3"
         do-while: true
      - to: "mock:result"
"#,
        r#"
- from:
    uri: "direct:start"
    steps:
      - loop:
          expression:
            constant: "3"
          do-while: true
      - to: "mock:result"
"#,
    ] {
        assert_eq!(vec!["Loop[constant{3} -> [to[mock:result]]]"], steps(yaml));
    }
}

#[tokio::test]
async fn loop_flow() {
    let context = start(
        r#"
- from:
    uri: "direct:route"
    steps:
      - loop:
         copy: true
         constant: "3"
      - to: "mock:result"
"#,
    )
    .await;
    let m = mock(&context, "mock:result");
    m.expected_message_count(3);
    context
        .create_producer_template()
        .send_body("direct:route", "a")
        .await
        .unwrap();
    m.assert_is_satisfied().await;
}

#[test]
fn pipeline_definition() {
    let s = steps(
        r#"
- from:
    uri: "direct:start"
    steps:
      - pipeline:
          steps:
            - log: "test"
"#,
    );
    assert_eq!(vec!["Pipeline[[Log[test]]]"], s);
}

#[test]
fn set_headers_definition() {
    let s = steps(
        r#"
- from:
    uri: "direct:start"
    steps:
      - setHeaders:
          headers:
            - name: testbody
              simple: "${body}"
            - name: testconstant
              constant: ABC
      - to: "mock:result"
"#,
    );
    assert_eq!(
        "SetHeaders[testbody=simple{${body}}, testconstant=constant{ABC}]",
        s[0]
    );
    let s = steps(
        r#"
- from:
    uri: "direct:start"
    steps:
      - setHeaders:
          headers:
            - name: foo
              simple: "${body}"
            - name: bar
              simple:
                expression: "${header.foo} > 10"
                resultType: "boolean"
      - to: "mock:result"
"#,
    );
    assert_eq!(
        "SetHeaders[foo=simple{${body}}, bar=simple{${header.foo} > 10}]",
        s[0]
    );
}

#[test]
fn recipient_list_throttle_delay_definitions() {
    for block in [
        "recipientList:\n          constant: \"direct:a,direct:b\"",
        "recipientList:\n          expression:\n            constant: \"direct:a,direct:b\"",
    ] {
        let yaml = format!(
            "- from:\n    uri: direct:start\n    steps:\n      - {block}\n          stopOnException: true\n          parallelProcessing: true\n      - to: mock:result\n"
        );
        assert_eq!(
            "RecipientList[constant{direct:a,direct:b}]",
            steps(&yaml)[0]
        );
    }
    for block in [
        "throttle:\n          constant: \"5\"",
        "throttle:\n          expression:\n            constant: \"5\"",
    ] {
        let yaml = format!(
            "- from:\n    uri: direct:start\n    steps:\n      - {block}\n          executor-service: myExecutor\n      - to: mock:result\n"
        );
        assert_eq!(
            "Throttle[constant{5} request per 1000 millis]",
            steps(&yaml)[0]
        );
    }
    for block in [
        "delay:\n          simple: \"${body}\"\n          async-delayed: true",
        "delay:\n          expression:\n            simple: \"${body}\"\n          asyncDelayed: true",
    ] {
        let yaml = format!(
            "- from:\n    uri: direct:start\n    steps:\n      - {block}\n      - to: mock:result\n"
        );
        assert!(steps(&yaml)[0].contains("simple{${body}}"), "{:?}", steps(&yaml));
    }
}

#[test]
fn throw_exception_and_remove_property_definitions() {
    let s = steps(
        r#"
- from:
    uri: "direct:start"
    steps:
      - throwException:
          exceptionType: "java.lang.IllegalArgumentException"
          message: "test"
"#,
    );
    assert_eq!("ThrowException[java.lang.IllegalArgumentException]", s[0]);
    let routes = YamlRoutes::parse(
        "- from:\n    uri: direct:start\n    steps:\n      - throwException:\n          exceptionType: java.lang.IllegalArgumentException\n          message: test\n",
    )
    .unwrap();
    match &routes.route_definitions()[0].steps[0].eip {
        camel_core::model::processor::Eip::ThrowException(e) => {
            assert_eq!(Some("test"), e.message());
            assert!(e.is(ErrorKind::IllegalArgument));
        }
        other => panic!("{other:?}"),
    }
    let s = steps(
        "- from:\n    uri: direct:start\n    steps:\n      - removeProperty:\n          name: test\n      - removeProperties:\n          pattern: toRemove\n          excludePattern: toExclude\n",
    );
    assert_eq!(
        vec!["RemoveProperty[test]", "RemoveProperties[toRemove]"],
        s
    );
}

#[tokio::test]
async fn idempotent_consumer() {
    let context = start(
        r#"
- beans:
  - name: myRepo
    type: org.apache.camel.support.processor.idempotent.MemoryIdempotentRepository
- from:
    uri: "direct:route"
    steps:
      - idempotentConsumer:
          simple: "${header.id}"
          idempotentRepository: "myRepo"
          steps:
            - to: "mock:idempotent"
      - to: "mock:route"
"#,
    )
    .await;
    mock(&context, "mock:idempotent").expected_bodies_received(["a", "b", "c"]);
    mock(&context, "mock:route").expected_bodies_received(["a", "b", "a2", "b2", "c"]);
    let t = context.create_producer_template();
    for (body, id) in [("a", "1"), ("b", "2"), ("a2", "1"), ("b2", "2"), ("c", "3")] {
        t.send_body_and_header("direct:route", body, "id", id)
            .await
            .unwrap();
    }
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[tokio::test]
async fn intercept() {
    let context = start(
        r#"
- intercept:
    steps:
      - to: "mock:intercepted"
- from:
    uri: "direct:start"
    steps:
      - to: "mock:foo"
      - setBody:
          constant: "Hello Bar"
      - to: "mock:bar"
      - to: "mock:result"
"#,
    )
    .await;
    mock(&context, "mock:intercepted").expected_bodies_received([
        "hello",
        "hello",
        "Hello Bar",
        "Hello Bar",
    ]);
    context
        .create_producer_template()
        .send_body("direct:start", "hello")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[tokio::test]
async fn intercept_on_when() {
    let context = start(
        r#"
- intercept:
    onWhen:
      simple: "${body} contains 'Camel'"
    steps:
      - to: "mock:intercepted"
- from:
    uri: "direct:start"
    steps:
      - to: "mock:foo"
      - to: "mock:bar"
      - to: "mock:result"
"#,
    )
    .await;
    mock(&context, "mock:intercepted").expected_bodies_received([
        "Hello Camel",
        "Hello Camel",
        "Hello Camel",
    ]);
    let t = context.create_producer_template();
    t.send_body("direct:start", "Hello World").await.unwrap();
    t.send_body("direct:start", "Hello Camel").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[tokio::test]
async fn intercept_from() {
    let context = start(
        r#"
- interceptFrom:
    steps:
      - to: "mock:intercepted"
- from:
    uri: "direct:start"
    steps:
      - to: "mock:foo"
      - setBody:
          constant: "Hello Bar"
      - to: "mock:bar"
      - to: "mock:result"
"#,
    )
    .await;
    mock(&context, "mock:intercepted").expected_bodies_received(["hello"]);
    context
        .create_producer_template()
        .send_body("direct:start", "hello")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[test]
fn expression_errors() {
    let when = |body: &str| {
        format!(
            "- from:\n    uri: direct:start\n    steps:\n      - choice:\n          when:\n{body}"
        )
    };
    let m = parse_error(&when(
        "            - simple: \"${body.size()} == 1\"\n              jq: \".size == 1\"\n              steps:\n                - to: log:when-a\n",
    ));
    assert!(m.contains("2 are valid"), "{m}");
    let m = parse_error(&when(
        "            - expression:\n                simple: \"${body.size()} == 1\"\n                jq: \".size == 1\"\n              steps:\n                - to: log:when-a\n",
    ));
    assert!(m.contains("2 are valid"), "{m}");
    let m = parse_error(&when(
        "            - simple: \"${body.size()} == 1\"\n              expression:\n                jq: \".size == 1\"\n              steps:\n                - to: log:when-a\n",
    ));
    assert!(m.contains("2 are valid"), "{m}");
    let m = parse_error(&when(
        "            - notsimple: \"${body.size()} == 1\"\n              steps:\n                - to: log:when-a\n",
    ));
    assert!(m.contains("additional properties"), "{m}");
    let m = parse_error(&when(
        "            - expression:\n                notsimple: \"${body.size()} == 1\"\n              steps:\n                - to: log:when-a\n",
    ));
    assert!(m.contains("0 are valid"), "{m}");
    let m = parse_error(&when(
        "            - steps:\n                - to: log:when-a\n",
    ));
    assert!(m.contains("when/0"), "{m}");
    // sort without expression is allowed
    let s = steps("- from:\n    uri: direct:start\n    steps:\n      - sort:\n          comparator: \"#myComparator\"\n      - to: log:sorted\n");
    assert_eq!("sort[body]", s[0]);
}

#[tokio::test]
async fn route_with_choice_and_error_handler() {
    let context = start(
        r#"
- errorHandler:
    deadLetterChannel:
      deadLetterUri: mock:dead
      redeliveryPolicy:
        maximumRedeliveries: 1
        redeliveryDelay: 0
- route:
    id: greetings
    from:
      uri: direct:start
      steps:
        - choice:
            when:
              - simple: "${header.lang} == 'fr'"
                steps:
                  - setBody:
                      constant: Bonjour
            otherwise:
              steps:
                - setBody:
                    simple: "Hello ${body}"
        - filter:
            simple: "${body} contains 'boom'"
            steps:
              - throwException:
                  exceptionType: java.lang.IllegalArgumentException
                  message: Forced
        - to: mock:result
"#,
    )
    .await;
    assert!(context.route("greetings").is_some());
    mock(&context, "mock:result").expected_bodies_received(["Bonjour", "Hello World"]);
    mock(&context, "mock:dead").expected_message_count(1);
    let t = context.create_producer_template();
    t.send_body_and_header("direct:start", "World", "lang", "fr")
        .await
        .unwrap();
    t.send_body("direct:start", "World").await.unwrap();
    t.send_body("direct:start", "boom").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[test]
fn empty_and_invalid_documents() {
    assert!(YamlRoutes::parse("")
        .unwrap()
        .route_definitions()
        .is_empty());
    assert!(parse_error("- from: [").contains("Invalid YAML"));
    assert!(parse_error("- rest: {}").contains("not yet supported"));
}

#[tokio::test]
async fn threads() {
    let context = start(
        r#"
- from:
    uri: "direct:start"
    steps:
      - threads:
          poolSize: 2
          maxPoolSize: 4
          rejectedPolicy: Abort
      - to: "mock:result"
"#,
    )
    .await;
    mock(&context, "mock:result").expected_bodies_received(["Hello"]);
    context
        .create_producer_template()
        .send_body("direct:start", "Hello")
        .await
        .unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}
