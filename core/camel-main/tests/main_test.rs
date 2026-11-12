//! Ports of `MainTest`, `MainPropertyPlaceholderTest`, `MainDurationAliasTest` and
//! `MainListenerTest` (the test resources are in `src/test/resources`, a classpath root).

use camel_core::api::processor::processor_fn;
use camel_core::prelude::*;
use camel_main::{Main, MainConfigurationProperties, MainListener};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn my_route_builder(r: &mut Routes) {
    r.add(from("direct:start").to("mock:results"));
}

fn placeholder(main: &Main, text: &str) -> CamelResult<Option<String>> {
    main.camel_context()
        .unwrap()
        .resolve_property_placeholders(text)
}

async fn assert_route_works(main: &Main) {
    let context = main.camel_context().unwrap();
    let endpoint = MockEndpoint::resolve(context.as_ref(), "mock:results").unwrap();
    endpoint.expected_minimum_message_count(1);
    main.camel_template()
        .unwrap()
        .send_body("direct:start", "<message>1</message>")
        .await
        .unwrap();
    endpoint.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_main() {
    for standby in [false, true] {
        let mut main = Main::new();
        main.configure().add_routes_builder(my_route_builder);
        if standby {
            main.enable_trace_standby();
        } else {
            main.enable_trace();
        }
        main.bind("foo", 31);
        main.start().await.unwrap();
        let context = main.camel_context().unwrap();
        assert_eq!(
            Some(Value::from(31)),
            context.simple_registry().lookup_by_name("foo"),
            "Could not find the registry bound object"
        );
        assert_route_works(&main).await;
        main.stop().await.unwrap();
    }
}

#[tokio::test]
async fn test_loading_options_from_command() {
    let mut main = Main::new();
    main.parse_arguments(&["-d", "2m", "-t", "-p", "prod"])
        .unwrap();
    assert_eq!(120, main.configuration().duration_max_seconds());
    assert!(main.is_tracing());
    assert_eq!(Some("prod"), main.configuration().profile());
    assert!(main.parse_arguments(&["-nope"]).is_err());
    assert!(main.parse_arguments(&["-d"]).is_err());
}

#[tokio::test]
async fn test_optional_properties_and_profile() {
    let mut main = Main::new();
    main.configure().add_routes_builder(my_route_builder);
    main.start().await.unwrap();
    assert_eq!(
        Some("World".into()),
        placeholder(&main, "{{hello}}").unwrap()
    );
    // camel.main.tracing = false in application.properties
    assert!(!main.is_tracing(), "Tracing should be disabled");
    main.stop().await.unwrap();

    let mut main = Main::new();
    main.configure().add_routes_builder(my_route_builder);
    main.configure().with_profile("prod");
    main.start().await.unwrap();
    assert_eq!(
        Some("Production World".into()),
        placeholder(&main, "{{hello}}").unwrap()
    );
    main.stop().await.unwrap();
}

#[tokio::test]
async fn test_duration_max_seconds() {
    let mut main = Main::new();
    main.configure().add_routes_builder(my_route_builder);
    main.configure().with_duration_max_seconds(1);
    let started = Instant::now();
    main.run().await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert!(main.camel_context().is_some());
}

#[tokio::test]
async fn test_default_property_placeholder_location() {
    for disabled in ["false", "classpath:additional-config.properties,false"] {
        let mut main = Main::new();
        main.set_default_property_placeholder_location(disabled);
        main.start().await.unwrap();
        let e = placeholder(&main, "{{hello}}").unwrap_err();
        assert!(e.is(ErrorKind::IllegalArgument), "{e}");
        main.stop().await.unwrap();
    }
    let mut main = Main::new();
    main.start().await.unwrap();
    assert_eq!(
        Some("World".into()),
        placeholder(&main, "{{hello}}").unwrap()
    );
    main.stop().await.unwrap();

    let mut main = Main::new();
    main.set_default_property_placeholder_location("classpath:default.properties");
    main.start().await.unwrap();
    assert_eq!(
        Some("default".into()),
        placeholder(&main, "{{hello}}").unwrap()
    );
    main.stop().await.unwrap();
}

#[tokio::test]
async fn test_properties_with_map() {
    let mut main = Main::new();
    main.set_initial_properties([("1", "val-init"), ("2", "val-init")]);
    main.set_override_properties([("1", "val-override"), ("3", "val-override")]);
    main.start().await.unwrap();
    assert_eq!(
        Some("val-override".into()),
        placeholder(&main, "{{1}}").unwrap()
    );
    assert_eq!(
        Some("val-init".into()),
        placeholder(&main, "{{2}}").unwrap()
    );
    assert_eq!(
        Some("val-override".into()),
        placeholder(&main, "{{3}}").unwrap()
    );
    main.stop().await.unwrap();
}

#[test]
fn test_duration_with_unit() {
    let mut config = MainConfigurationProperties::default();
    for (text, seconds) in [
        ("15s", 15),
        ("2m", 120),
        ("1h", 3600),
        ("500ms", 1),
        ("30", 30),
    ] {
        config.set_duration(text).unwrap();
        assert_eq!(seconds, config.duration_max_seconds(), "{text}");
    }
}

#[tokio::test]
async fn test_duration_as_property() {
    let mut main = Main::new();
    main.add_initial_property("camel.main.duration", "45s");
    main.configure().with_duration_max_messages(0);
    main.start().await.unwrap();
    assert_eq!(45, main.configuration().duration_max_seconds());
    main.stop().await.unwrap();
}

struct Recorder(Arc<Mutex<Vec<&'static str>>>);

impl MainListener for Recorder {
    fn before_initialize(&self, _: &Main) {
        self.0.lock().unwrap().push("beforeInitialize");
    }
    fn before_configure(&self, _: &Main) {
        self.0.lock().unwrap().push("beforeConfigure");
    }
    fn after_configure(&self, _: &Main) {
        self.0.lock().unwrap().push("afterConfigure");
    }
    fn before_start(&self, _: &Main) {
        self.0.lock().unwrap().push("beforeStart");
    }
    fn after_start(&self, main: &Main) {
        self.0.lock().unwrap().push("afterStart");
        main.completed();
    }
    fn before_stop(&self, _: &Main) {
        self.0.lock().unwrap().push("beforeStop");
    }
    fn after_stop(&self, _: &Main) {
        self.0.lock().unwrap().push("afterStop");
    }
}

#[tokio::test]
async fn test_event_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut main = Main::new();
    main.add_main_listener(Recorder(events.clone()));
    tokio::time::timeout(Duration::from_secs(10), main.run())
        .await
        .expect("Main did not complete within 10 seconds")
        .unwrap();
    assert_eq!(
        vec![
            "beforeInitialize",
            "beforeConfigure",
            "afterConfigure",
            "beforeStart",
            "afterStart",
            "beforeStop",
            "afterStop"
        ],
        *events.lock().unwrap()
    );
}

struct OverrideName;

impl MainListener for OverrideName {
    fn before_configure(&self, main: &Main) {
        main.camel_context()
            .unwrap()
            .properties()
            .set_override_properties([("camel.context.name", "my-ctx-override")]);
    }
}

#[tokio::test]
async fn test_before_configure() {
    let mut main = Main::new();
    main.set_default_property_placeholder_location("false");
    main.set_initial_properties([("camel.context.name", "my-ctx")]);
    main.start().await.unwrap();
    assert_eq!("my-ctx", main.camel_context().unwrap().name());
    main.stop().await.unwrap();

    let mut main = Main::new();
    main.set_default_property_placeholder_location("false");
    main.set_initial_properties([("camel.context.name", "my-ctx")]);
    main.add_main_listener(OverrideName);
    main.start().await.unwrap();
    assert_eq!("my-ctx-override", main.camel_context().unwrap().name());
    main.stop().await.unwrap();
}

#[tokio::test]
async fn test_duration_max_messages_and_idle_seconds() {
    // the timer fires twice, the second message completes the run
    let mut main = Main::new();
    main.configure().with_duration_max_messages(2);
    let list = Arc::new(Mutex::new(Vec::new()));
    let l = list.clone();
    main.configure().add_routes_builder(move |r: &mut Routes| {
        let l = l.clone();
        r.add(
            from("timer:foo?period=10&repeatCount=5")
                .set_body(simple("{{hello}}"))
                .process(Arc::new(processor_fn(move |e: &mut Exchange| {
                    l.lock().unwrap().push(e.message().body().to_string());
                    Ok(())
                }))),
        );
    });
    tokio::time::timeout(Duration::from_secs(10), main.run())
        .await
        .expect("the run completes after 2 messages")
        .unwrap();
    let list = list.lock().unwrap().clone();
    assert!(list.len() >= 2, "{list:?}");
    assert_eq!("World", list[0]);

    // Java testDurationIdleSeconds: no messages, stops after the idle time
    let mut main = Main::new();
    main.configure().add_routes_builder(my_route_builder);
    main.configure().with_duration_max_idle_seconds(1);
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(10), main.run())
        .await
        .expect("the run completes when idle")
        .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1));
}
