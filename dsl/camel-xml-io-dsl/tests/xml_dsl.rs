//! Ports of `XmlLoadTest` and `XmlRoutesBuilderLoaderTest.canLoadRoutes`, and Rust tests of
//! the XML forms of the EIPs.

use camel_core::prelude::*;
use camel_xml_io_dsl::XmlRoutes;
use std::sync::Arc;

const RESOURCES: &str = "src/test/resources/org/apache/camel/dsl/xml/io";

fn resource(name: &str) -> String {
    std::fs::read_to_string(format!("{RESOURCES}/{name}")).unwrap()
}

async fn start(xml: &str) -> Arc<DefaultCamelContext> {
    let context = camel_core::default_context();
    context
        .add_routes(XmlRoutes::parse(xml).unwrap())
        .await
        .unwrap();
    context.start().await.unwrap();
    context
}

fn mock(context: &DefaultCamelContext, uri: &str) -> MockEndpoint {
    MockEndpoint::resolve(context, uri).unwrap()
}

#[tokio::test]
async fn test_load_routes_builder_from_xml() {
    let context = camel_core::default_context();
    context
        .add_routes(|r: &mut Routes| {
            r.add(from("direct:foo").route_id("foo").to("mock:foo"));
        })
        .await
        .unwrap();
    context.start().await.unwrap();
    assert!(
        context.route("foo").is_some(),
        "Existing foo route should be there"
    );
    assert_eq!(1, context.routes().len());
    let foo = mock(&context, "mock:foo");
    foo.expected_bodies_received(["Hello World"]);
    context
        .create_producer_template()
        .send_body("direct:foo", "Hello World")
        .await
        .unwrap();
    foo.assert_is_satisfied().await;
    // load the routes from XML and add them to the existing context
    let routes = XmlRoutes::parse(&resource("barRoute.xml")).unwrap();
    context.add_routes(routes).await.unwrap();
    context.start_route("bar").await.unwrap();
    assert!(
        context.route("bar").is_some(),
        "Loaded bar route should be there"
    );
    assert_eq!(2, context.routes().len());
    let bar = mock(&context, "mock:bar");
    bar.expected_bodies_received(["Bye World"]);
    context
        .create_producer_template()
        .send_body("direct:bar", "Bye World")
        .await
        .unwrap();
    bar.assert_is_satisfied().await;
}

#[tokio::test]
async fn test_load_routes_builder_from_xml_no_namespace() {
    let context = start(&resource("bar2.xml")).await;
    assert!(
        context.route("bar2").is_some(),
        "Loaded bar2 route should be there"
    );
    assert_eq!(1, context.routes().len());
    let bar = mock(&context, "mock:bar2");
    bar.expected_bodies_received(["Hi World"]);
    context
        .create_producer_template()
        .send_body("direct:bar2", "Hi World")
        .await
        .unwrap();
    bar.assert_is_satisfied().await;
}

#[test]
fn can_load_routes() {
    let routes = XmlRoutes::parse(
        r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
   <route id="xpath-route">
      <from uri="direct:test"/>
      <setBody>
         <xpath resultType="java.lang.String">
            /foo:orders/order[1]/country/text()
         </xpath>
      </setBody>
   </route>
</routes>"#,
    )
    .unwrap();
    let defs = routes.yaml_routes().route_definitions();
    assert_eq!(1, defs.len());
    assert_eq!(
        "Route(xpath-route)[From[direct:test] -> [SetBody[xpath{/foo:orders/order[1]/country/text()}]]]",
        defs[0].to_string()
    );
}

#[tokio::test]
async fn test_choice_filter_split_and_headers() {
    let context = start(
        r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <route id="r">
    <from uri="direct:start"/>
    <setHeaders>
      <setHeader name="a"><simple>${body}</simple></setHeader>
      <setHeader name="b"><constant>B</constant></setHeader>
    </setHeaders>
    <choice>
      <when>
        <simple>${body} == 'x,y'</simple>
        <split>
          <simple>${body}</simple>
          <to uri="mock:split"/>
        </split>
      </when>
      <otherwise>
        <to uri="mock:other"/>
      </otherwise>
    </choice>
    <filter>
      <simple>${header.b} == 'B'</simple>
    </filter>
    <to uri="mock:result"/>
  </route>
</routes>"#,
    )
    .await;
    mock(&context, "mock:split").expected_bodies_received(["x", "y"]);
    mock(&context, "mock:other").expected_bodies_received(["z"]);
    let result = mock(&context, "mock:result");
    result.expected_message_count(2);
    result.expected_header_received("b", "B");
    let t = context.create_producer_template();
    t.send_body("direct:start", "x,y").await.unwrap();
    t.send_body("direct:start", "z").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[tokio::test]
async fn test_do_try_and_on_exception() {
    let context = start(
        r#"<routes xmlns="http://camel.apache.org/schema/xml-io">
  <onException>
    <exception>java.lang.IllegalStateException</exception>
    <handled><constant>true</constant></handled>
    <to uri="mock:handled"/>
  </onException>
  <route>
    <from uri="direct:try"/>
    <doTry>
      <throwException exceptionType="java.lang.IllegalArgumentException" message="Forced"/>
      <doCatch>
        <exception>java.lang.IllegalArgumentException</exception>
        <to uri="mock:catch"/>
      </doCatch>
      <doFinally>
        <to uri="mock:finally"/>
      </doFinally>
    </doTry>
  </route>
  <route>
    <from uri="direct:fail"/>
    <throwException exceptionType="java.lang.IllegalStateException" message="Boom"/>
  </route>
</routes>"#,
    )
    .await;
    mock(&context, "mock:catch").expected_message_count(1);
    mock(&context, "mock:finally").expected_message_count(1);
    mock(&context, "mock:handled").expected_message_count(1);
    let t = context.create_producer_template();
    t.send_body("direct:try", "a").await.unwrap();
    t.send_body("direct:fail", "b").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[tokio::test]
async fn test_load_balance_and_loop() {
    let context = start(
        r#"<routes>
  <route>
    <from uri="direct:start"/>
    <loop copy="true">
      <constant>2</constant>
      <to uri="mock:loop"/>
    </loop>
    <loadBalance>
      <roundRobin/>
      <to uri="mock:x"/>
      <to uri="mock:y"/>
    </loadBalance>
  </route>
</routes>"#,
    )
    .await;
    mock(&context, "mock:loop").expected_message_count(4);
    mock(&context, "mock:x").expected_message_count(1);
    mock(&context, "mock:y").expected_message_count(1);
    let t = context.create_producer_template();
    t.send_body("direct:start", "a").await.unwrap();
    t.send_body("direct:start", "b").await.unwrap();
    MockEndpoint::assert_is_satisfied_all(context.endpoints()).await;
}

#[test]
fn test_invalid_documents() {
    assert!(XmlRoutes::parse("<routes><route>").is_err());
    assert!(XmlRoutes::parse("<rests/>").is_err());
    assert!(XmlRoutes::parse("<routes><route><to uri=\"x\"/></route></routes>").is_err());
}
