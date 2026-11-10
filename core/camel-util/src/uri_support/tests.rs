//! Port of `URISupportTest` (camel-util) and `URIScannerTest`.

use super::*;
use std::sync::Mutex;

/// The sanitize keywords are global, so the tests that change them run one at a time and
/// reset them afterwards (the Java test used an `@AfterEach`).
static KEYWORDS_LOCK: Mutex<()> = Mutex::new(());

struct ResetKeywords<'a>(#[allow(dead_code)] std::sync::MutexGuard<'a, ()>);

impl Drop for ResetKeywords<'_> {
    fn drop(&mut self) {
        reset_sanitize_keywords();
    }
}

fn keywords_guard() -> ResetKeywords<'static> {
    ResetKeywords(KEYWORDS_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
}

fn n(uri: &str) -> String {
    normalize_uri(uri).unwrap_or_else(|e| panic!("{uri}: {e}"))
}

fn uri(s: &str) -> JavaUri {
    JavaUri::parse(s).unwrap()
}

fn params(entries: &[(&str, &str)]) -> Parameters {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), ParamValue::from(*v)))
        .collect()
}

#[test]
fn test_normalize_endpoint_uri() {
    assert_eq!(
        n("smtp://localhost?username=davsclaus&password=secret"),
        n("smtp://localhost?password=secret&username=davsclaus")
    );
    assert_eq!(
        n("smtp://localhost?username=davsclaus&password=secret"),
        n("smtp:localhost?password=secret&username=davsclaus")
    );
    assert_eq!(
        n("smtp:localhost?password=secret&username=davsclaus"),
        n("smtp://localhost?username=davsclaus&password=secret")
    );
    assert_eq!(
        n("seda:foo?concurrentConsumer=2"),
        n("seda:foo?concurrentConsumer=2")
    );
    assert_ne!(n("seda:foo?concurrentConsumer=2"), n("seda:foo"));
    assert_eq!("foo://?test=1", n("foo://?test=1"));
    assert_eq!(n("foo:?test=1"), n("foo://?test=1"));
}

#[test]
fn test_normalize_endpoint_uri_no_param() {
    assert_eq!(n("direct:foo"), n("direct:foo"));
    assert_eq!(n("direct://foo"), n("direct://foo"));
    assert_eq!(n("direct:foo"), n("direct://foo"));
    assert_eq!(n("direct://foo"), n("direct:foo"));
    assert_ne!(n("direct://foo"), n("direct:bar"));
}

#[test]
fn test_normalize_endpoint_uri_with_fragments() {
    assert_eq!(
        n("irc://someserver/#camel?user=davsclaus"),
        n("irc:someserver/#camel?user=davsclaus")
    );
    assert_ne!(
        n("irc://someserver/#camel?user=davsclaus"),
        n("irc:someserver/#camel?user=hadrian")
    );
}

#[test]
fn test_normalize_http_endpoint() {
    let out1 = n("http://www.google.com?q=Camel");
    let out2 = n("http:www.google.com?q=Camel");
    assert_eq!(out1, out2);
    assert!(out1.starts_with("http://"));
    assert!(out2.starts_with("http://"));
}

#[test]
fn test_normalize_ipv6_http_endpoint() {
    assert_eq!(
        "http://[2a00:8a00:6000:40::1413]:30300/test",
        n("http://[2a00:8a00:6000:40::1413]:30300/test")
    );
}

#[test]
fn test_normalize_http_endpoint_unicoded_parameter() {
    assert_eq!(
        "http://www.google.com?q=S%C3%B8ren",
        n("http://www.google.com?q=S\u{F8}ren")
    );
}

#[test]
fn test_parse_parameters_unicoded_value() {
    let out = n("http://www.google.com?q=S\u{F8}ren");
    let parameters = parse_parameters(&uri(&out)).unwrap();
    assert_eq!(1, parameters.len());
    assert_eq!("S\u{F8}ren", parameters["q"]);
}

#[test]
fn test_normalize_http_endpoint_url_encoded_parameter() {
    assert_eq!(
        "http://www.google.com?q=S%C3%B8ren+Hansen",
        n("http://www.google.com?q=S%C3%B8ren%20Hansen")
    );
}

#[test]
fn test_normalize_space_the_same_in_every_spelling() {
    assert_eq!("log://foo?marker=a+b", n("log:foo?marker=a+b"));
    assert_eq!("log://foo?marker=a+b", n("log:foo?marker=a%20b"));
    assert_eq!("log://foo?marker=a+b", n("log:foo?marker=a b"));
    assert_eq!(
        "log://foo?marker=a+b&showAll=true",
        n("log:foo?showAll=true&marker=a+b")
    );
    assert_eq!("log://foo?marker=a++b", n("log:foo?marker=a++b"));
}

#[test]
fn test_normalize_value_with_percent_escape_form_encodes_the_query() {
    assert_eq!(
        "log://foo?secretKey=abc%2Fdef%3D%3D",
        n("log:foo?secretKey=abc/def==")
    );
    assert_eq!("log://foo?marker=a%23b%2Fc", n("log:foo?marker=a#b/c"));
    assert_eq!(
        "jms://queue:foo?foo=bar&selector=somekey%3D%27somevalue%27",
        n("jms:queue:foo?foo=bar&selector=somekey='somevalue'")
    );
    assert_eq!(
        "foo://bar?host=http://h&produces=application/json",
        n("foo:bar?produces=application/json&host=http://h")
    );
}

#[test]
fn test_normalize_space_in_key() {
    assert_eq!("log://foo?a+b=1", n("log:foo?a+b=1"));
    assert_eq!("log://foo?a+b=1&c=2", n("log:foo?a+b=1&c=2"));
}

#[test]
fn test_normalize_twice_gives_the_same_uri() {
    let uris = [
        "http://localhost:8080/foo?a=1&b=2",
        "http://localhost:8080/foo?b=hello world&a=1",
        "http://localhost:8080/foo?q=a+b",
        "http://localhost:8080/foo?q=a%20b",
        "http://localhost:8080/foo?q=a+b&x=1",
        "ftp://user@host.com:21/dir?password=se+cret&binary=true",
        "ftp://user@host.com:21/dir?password=RAW(se+cret)&binary=true",
        "log:foo?level=INFO&showAll=true",
        "timer:tick?period=1s&delay=2s",
        "direct:start?b=\u{f8}&a=1",
        "file:target/in?include=.*\\.txt&noop=true",
        "http://h/p?x=a&x=b&y=1",
        "mock:a?b=x+y+z&a=1",
        "jms:queue:foo?selector=somekey='somevalue'&foo=bar",
        "log:foo?secretKey=abc/def==",
        "log:foo?webhookExternalUrl=https://example.com/hook?token=abc",
        "log:foo?marker=a#b/c",
        "log:foo?a+b=1",
        "foo:bar?host=http://h&produces=application/json&x=a=b",
    ];
    for u in uris {
        let once = n(u);
        assert_eq!(once, n(&once), "normalizing {u} twice");
    }
}

#[test]
fn test_normalize_twice_gives_the_same_uri_with_hash_or_two_at_in_path() {
    assert_eq!(
        "sftp://me%40example.com@host/in?password=pa%3Dss",
        n("sftp://me@example.com@host/in?password=pa=ss")
    );
    assert_eq!(
        "sql://select+*+from+t+where+id=:%23id?dataSource=%23ds",
        n("sql:select+*+from+t+where+id=:#id?dataSource=#ds")
    );
    assert_eq!(
        "imaps://me%40example.com@imap.example.com?password=x&sslContextParameters=%23ssl",
        n("imaps://me@example.com@imap.example.com?password=x&sslContextParameters=#ssl")
    );
    assert_eq!(
        "sftp://me@example.com@host/in?binary=true",
        n("sftp://me@example.com@host/in?binary=true")
    );
    assert_eq!(
        "sql://select+*+from+t+where+id=:#id?dataSource=ds",
        n("sql:select+*+from+t+where+id=:#id?dataSource=ds")
    );
    let uris = [
        "sftp://me@example.com@host/in?password=pa=ss",
        "sql:select+*+from+t+where+id=:#id?dataSource=#ds",
        "imaps://me@example.com@imap.example.com?password=x&sslContextParameters=#ssl",
        "ftp://me@example.com@host/in/a@b?password=se=cret&binary=true",
        "http://host/a#b?q=#x",
        "log:a#b?marker=x=y&showAll=true",
    ];
    for u in uris {
        let once = n(u);
        assert_eq!(once, n(&once), "normalizing {u} twice");
    }
}

#[test]
fn test_parse_parameters_url_encoded_value() {
    let out = n("http://www.google.com?q=S%C3%B8ren%20Hansen");
    let parameters = parse_parameters(&uri(&out)).unwrap();
    assert_eq!(1, parameters.len());
    assert_eq!("S\u{F8}ren Hansen", parameters["q"]);
}

#[test]
fn test_normalize_uri_where_parameter_is_faulty() {
    assert!(
        normalize_uri("stream:uri?file:///d:/temp/data/log/quickfix.log&scanStream=true").is_ok()
    );
}

#[test]
fn test_create_remaining_uri() {
    let new_uri =
        create_remaining_uri(&uri("http://camel.apache.org"), &params(&[("foo", "123")])).unwrap();
    assert_eq!("http://camel.apache.org?foo=123", new_uri.to_string());
}

#[test]
fn test_create_uri_with_query_has_one_fragment() {
    let result = create_uri_with_query(&uri("smtp://localhost#fragmentOne"), None).unwrap();
    assert_eq!("smtp://localhost#fragmentOne", result.to_string());
}

#[test]
fn test_create_uri_with_query_has_one_fragment_and_query_parameter() {
    let result = create_uri_with_query(
        &uri("smtp://localhost#fragmentOne"),
        Some("utm_campaign=launch"),
    )
    .unwrap();
    assert_eq!(
        "smtp://localhost?utm_campaign=launch#fragmentOne",
        result.to_string()
    );
}

#[test]
fn test_create_uri_with_query_empty_query_string() {
    let result =
        create_uri_with_query(&uri("https://api.example.com/users/myuser/repos"), Some(""))
            .unwrap();
    assert_eq!(
        "https://api.example.com/users/myuser/repos",
        result.to_string()
    );
}

#[test]
fn test_normalize_endpoint_with_equal_sign_in_parameter() {
    assert_eq!(
        "jms://queue:foo?foo=bar&selector=somekey%3D%27somevalue%27",
        n("jms:queue:foo?selector=somekey='somevalue'&foo=bar")
    );
}

#[test]
fn test_normalize_endpoint_with_percent_sign_in_parameter() {
    assert_eq!(
        "http://someendpoint?password=%25test&username=james",
        n("http://someendpoint?username=james&password=%25test")
    );
}

#[test]
fn test_parse_parameters() {
    let p = parse_parameters(&uri(
        "quartz:myGroup/myTimerName?cron=0%200%20*%20*%20*%20?",
    ))
    .unwrap();
    assert_eq!(1, p.len());
    assert_eq!("0 0 * * * ?", p["cron"]);
    let p = parse_parameters(&uri(
        "quartz:myGroup/myTimerName?cron=0%200%20*%20*%20*%20?&bar=123",
    ))
    .unwrap();
    assert_eq!(2, p.len());
    assert_eq!("0 0 * * * ?", p["cron"]);
    assert_eq!("123", p["bar"]);
}

#[test]
fn test_create_remaining_uri_encoding() {
    let u =
        "http://localhost:23271/myapp/mytest?columns=name%2Ctotalsens%2Cupsens&username=apiuser";
    let map = params(&[
        ("foo", "abc def"),
        ("bar", "123,456"),
        ("name", "S\u{F8}ren"),
    ]);
    let out = create_remaining_uri(&uri(u), &map).unwrap();
    assert_eq!(
        "http://localhost:23271/myapp/mytest?foo=abc+def&bar=123%2C456&name=S%C3%B8ren",
        out.to_string()
    );
    assert_eq!(
        "http://localhost:23271/myapp/mytest?foo=abc+def&bar=123%2C456&name=S%C3%B8ren",
        out.to_ascii_string()
    );
}

#[test]
fn test_normalize_endpoint_uri_with_dual_parameters() {
    assert_eq!(
        "smtp://localhost?from=me&to=foo&to=bar",
        n("smtp://localhost?to=foo&to=bar&from=me")
    );
    assert_eq!(
        "smtp://localhost?from=me&from=you&to=foo&to=bar",
        n("smtp://localhost?to=foo&to=bar&from=me&from=you")
    );
}

#[test]
fn test_normalize_endpoint_uri_sort() {
    let out1 = n("smtp://localhost?to=foo&from=me");
    assert_eq!("smtp://localhost?from=me&to=foo", out1);
    let out2 = n("smtp://localhost?from=me&to=foo");
    assert_eq!("smtp://localhost?from=me&to=foo", out2);
    assert_eq!(out1, out2);
}

#[test]
fn test_normalize_endpoint_uri_order_independent_with_colon_value() {
    let out1 = n("kafka:mytopic?brokers=localhost:19092&groupId=mygroup");
    let out2 = n("kafka:mytopic?groupId=mygroup&brokers=localhost:19092");
    assert_eq!(out1, out2);
    assert_eq!(
        "kafka://mytopic?brokers=localhost:19092&groupId=mygroup",
        out1
    );
}

#[test]
fn test_normalize_endpoint_uri_order_independent_with_three_parameters() {
    let permutations = [
        "kafka:mytopic?brokers=localhost:19092&groupId=mygroup&clientId=myclient",
        "kafka:mytopic?brokers=localhost:19092&clientId=myclient&groupId=mygroup",
        "kafka:mytopic?groupId=mygroup&brokers=localhost:19092&clientId=myclient",
        "kafka:mytopic?groupId=mygroup&clientId=myclient&brokers=localhost:19092",
        "kafka:mytopic?clientId=myclient&brokers=localhost:19092&groupId=mygroup",
        "kafka:mytopic?clientId=myclient&groupId=mygroup&brokers=localhost:19092",
    ];
    let expected = "kafka://mytopic?brokers=localhost:19092&clientId=myclient&groupId=mygroup";
    for u in permutations {
        assert_eq!(expected, n(u), "normalizing: {u}");
    }
}

#[test]
fn test_normalize_endpoint_uri_order_independent_single_parameter_with_colon_value() {
    assert_eq!(
        "kafka://mytopic?brokers=localhost:19092",
        n("kafka:mytopic?brokers=localhost:19092")
    );
}

#[test]
fn test_normalize_endpoint_uri_order_independent_with_comma_value() {
    let out1 = n("smtp://localhost?subject=Hello,World&username=davsclaus");
    let out2 = n("smtp://localhost?username=davsclaus&subject=Hello,World");
    assert_eq!(out1, out2);
    assert_eq!(
        "smtp://localhost?subject=Hello,World&username=davsclaus",
        out1
    );
}

#[test]
fn test_normalize_endpoint_uri_order_independent_with_slash_value() {
    let out1 = n("foo:bar?host=http://petstore.example.com&produces=application/json");
    let out2 = n("foo:bar?produces=application/json&host=http://petstore.example.com");
    assert_eq!(out1, out2);
    assert_eq!(
        "foo://bar?host=http://petstore.example.com&produces=application/json",
        out1
    );
}

#[test]
fn test_normalize_endpoint_uri_order_independent_is_idempotent() {
    let out1 = n("kafka:mytopic?groupId=mygroup&brokers=localhost:19092");
    assert_eq!(out1, n(&out1));
}

#[test]
fn test_normalize_endpoint_uri_order_independent_with_raw_value() {
    let out1 = n("kafka:mytopic?password=RAW(p@ss:word)&username=scott");
    let out2 = n("kafka:mytopic?username=scott&password=RAW(p@ss:word)");
    assert_eq!(out1, out2);
    assert_eq!(
        "kafka://mytopic?password=RAW(p@ss:word)&username=scott",
        out1
    );
}

#[test]
fn test_normalize_endpoint_uri_order_independent_with_dual_parameters_and_colon_value() {
    let out1 = n("smtp://localhost?to=foo:1&to=bar:2&from=me");
    let out2 = n("smtp://localhost?from=me&to=foo:1&to=bar:2");
    assert_eq!(out1, out2);
    assert_eq!("smtp://localhost?from=me&to=foo:1&to=bar:2", out1);
}

#[test]
fn test_sanitize_access_token() {
    assert_eq!(
        "google-sheets-stream://spreadsheets?accessToken=xxxxxx&clientId=xxxxxx&clientSecret=xxxxxx",
        sanitize_uri("google-sheets-stream://spreadsheets?accessToken=MY_TOKEN&clientId=foo&clientSecret=MY_SECRET")
    );
}

#[test]
fn test_sanitize_authorization_token() {
    assert_eq!(
        "telegram:bots?authorizationToken=xxxxxx",
        sanitize_uri("telegram:bots?authorizationToken=1234567890:XXAuthTokenHereXX")
    );
}

#[test]
fn test_sanitize_api_key_header() {
    assert_eq!(
        "openai:CHAT?baseUrl=https://host&additionalHeader.api-key=xxxxxx",
        sanitize_uri("openai:CHAT?baseUrl=https://host&additionalHeader.api-key=MY_SECRET")
    );
}

#[test]
fn test_sanitize_authorization_header() {
    assert_eq!(
        "openai:CHAT?additionalHeader.Authorization=xxxxxx",
        sanitize_uri("openai:CHAT?additionalHeader.Authorization=Bearer%20MY_SECRET")
    );
}

#[test]
fn test_sanitize_uri_with_user_info() {
    assert_eq!(
        "jt400://GEORGE:xxxxxx@LIVERPOOL/QSYS.LIB/BEATLES.LIB/PENNYLANE.DTAQ",
        sanitize_uri("jt400://GEORGE:HARRISON@LIVERPOOL/QSYS.LIB/BEATLES.LIB/PENNYLANE.DTAQ")
    );
}

#[test]
fn test_sanitize_uri_with_user_info_and_colon_password() {
    assert_eq!(
        "sftp://USERNAME:xxxxxx@sftp.server.test",
        sanitize_uri("sftp://USERNAME:HARRISON:COLON@sftp.server.test")
    );
}

#[test]
fn test_sanitize_path_with_user_info() {
    assert_eq!(
        "GEORGE:xxxxxx@LIVERPOOL/QSYS.LIB/BEATLES.LIB/PENNYLANE.PGM",
        sanitize_path("GEORGE:HARRISON@LIVERPOOL/QSYS.LIB/BEATLES.LIB/PENNYLANE.PGM")
    );
}

#[test]
fn test_sanitize_path_with_user_info_and_colon_password() {
    assert_eq!(
        "USERNAME:xxxxxx@sftp.server.test",
        sanitize_path("USERNAME:HARRISON:COLON@sftp.server.test")
    );
}

#[test]
fn test_sanitize_path_without_sensitive_info_is_unchanged() {
    assert_eq!("myhost:8080/mypath", sanitize_path("myhost:8080/mypath"));
}

#[test]
fn test_sanitize_uri_with_raw_password() {
    let expected = "http://foo?username=xxxxxx&password=xxxxxx&foo=bar";
    assert_eq!(
        expected,
        sanitize_uri("http://foo?username=me&password=RAW(me#@123)&foo=bar")
    );
    assert_eq!(
        expected,
        sanitize_uri("http://foo?username=me&password=RAW{me#@123}&foo=bar")
    );
}

#[test]
fn test_sanitize_uri_raw_unsafe_password() {
    let expected = "sftp://localhost/target?password=xxxxxx&username=xxxxxx";
    assert_eq!(
        expected,
        sanitize_uri("sftp://localhost/target?password=RAW(beforeAmp&afterAmp)&username=jrandom")
    );
    assert_eq!(
        expected,
        sanitize_uri("sftp://localhost/target?password=RAW{beforeAmp&afterAmp}&username=jrandom")
    );
}

#[test]
fn test_sanitize_uri_with_raw_password_and_simple_expression() {
    let expected = "http://foo?username=xxxxxx&password=xxxxxx&foo=bar&port=21&tempFileName=${file:name.noext}.tmp&anotherOption=true";
    assert_eq!(
        expected,
        sanitize_uri("http://foo?username=me&password=RAW(me#@123)&foo=bar&port=21&tempFileName=${file:name.noext}.tmp&anotherOption=true")
    );
    assert_eq!(
        expected,
        sanitize_uri("http://foo?username=me&password=RAW{me#@123}&foo=bar&port=21&tempFileName=${file:name.noext}.tmp&anotherOption=true")
    );
}

#[test]
fn test_sanitize_sasl_jaas_config() {
    assert_eq!(
        "kafka://MY-TOPIC-NAME?saslJaasConfig=xxxxxx",
        sanitize_uri("kafka://MY-TOPIC-NAME?saslJaasConfig=org.apache.kafka.common.security.plain.PlainLoginModule required username=scott password=tiger")
    );
}

#[test]
fn test_normalize_endpoint_uri_with_user_info_special_sign() {
    let expected = "ftp://us%40r:t%25st@localhost:21000/tmp3/camel?foo=us%40r";
    assert_eq!(
        expected,
        n("ftp://us%40r:t%st@localhost:21000/tmp3/camel?foo=us@r")
    );
    assert_eq!(
        expected,
        n("ftp://us%40r:t%25st@localhost:21000/tmp3/camel?foo=us@r")
    );
    assert_eq!(
        expected,
        n("ftp://us@r:t%st@localhost:21000/tmp3/camel?foo=us@r")
    );
    assert_eq!(
        expected,
        n("ftp://us@r:t%25st@localhost:21000/tmp3/camel?foo=us@r")
    );
}

#[test]
fn test_special_uri_from_xmpp_component() {
    assert_eq!(
        "xmpp://camel-user@localhost:123/test-user@localhost?password=secret&serviceName=someCoolChat",
        n("xmpp://camel-user@localhost:123/test-user@localhost?password=secret&serviceName=someCoolChat")
    );
}

#[test]
fn test_raw_parameter() {
    assert_eq!(
        "xmpp://camel-user@localhost:123/test-user@localhost?password=RAW(++?w0rd)&serviceName=some+chat",
        n("xmpp://camel-user@localhost:123/test-user@localhost?password=RAW(++?w0rd)&serviceName=some chat")
    );
    // the RAW parameter can be resolved rightly, the % is replaced with %25
    assert_eq!(
        "xmpp://camel-user@localhost:123/test-user@localhost?password=RAW(foo %25%25 bar)&serviceName=some+chat",
        n("xmpp://camel-user@localhost:123/test-user@localhost?password=RAW(foo %% bar)&serviceName=some chat")
    );
}

#[test]
fn test_raw_parameter_curly() {
    assert_eq!(
        "xmpp://camel-user@localhost:123/test-user@localhost?password=RAW{++?w0rd}&serviceName=some+chat",
        n("xmpp://camel-user@localhost:123/test-user@localhost?password=RAW{++?w0rd}&serviceName=some chat")
    );
    assert_eq!(
        "xmpp://camel-user@localhost:123/test-user@localhost?password=RAW{foo %25%25 bar}&serviceName=some+chat",
        n("xmpp://camel-user@localhost:123/test-user@localhost?password=RAW{foo %% bar}&serviceName=some chat")
    );
}

#[test]
fn test_parse_query_double_ampersand() {
    let map = parse_query("delay=250&&period=500").unwrap();
    assert_eq!(2, map.len());
    assert_eq!("250", map["delay"]);
    assert_eq!("500", map["period"]);

    let map = parse_query("&&delay=250&&&period=500").unwrap();
    assert_eq!(2, map.len());
    assert_eq!("250", map["delay"]);
    assert_eq!("500", map["period"]);

    let map = parse_query("password=RAW(se&&cret)&&serviceName=somechat").unwrap();
    assert_eq!(2, map.len());
    assert_eq!("RAW(se&&cret)", map["password"]);
    assert_eq!("somechat", map["serviceName"]);

    assert_eq!(
        "timer://foo?delay=250&period=500",
        n("timer://foo?delay=250&&period=500")
    );
}

#[test]
fn test_parse_query() {
    let cases = [
        ("password=secret&serviceName=somechat", "secret"),
        ("password=RAW(++?w0rd)&serviceName=somechat", "RAW(++?w0rd)"),
        (
            "password=RAW(++?)w&rd)&serviceName=somechat",
            "RAW(++?)w&rd)",
        ),
        (
            "password=RAW(%2520w&rd)&serviceName=somechat",
            "RAW(%2520w&rd)",
        ),
        ("password=RAW{++?w0rd}&serviceName=somechat", "RAW{++?w0rd}"),
        (
            "password=RAW{++?)w&rd}&serviceName=somechat",
            "RAW{++?)w&rd}",
        ),
        (
            "password=RAW{%2520w&rd}&serviceName=somechat",
            "RAW{%2520w&rd}",
        ),
    ];
    for (q, password) in cases {
        let map = parse_query(q).unwrap();
        assert_eq!(2, map.len(), "{q}");
        assert_eq!(password, map["password"], "{q}");
        assert_eq!("somechat", map["serviceName"], "{q}");
    }
}

#[test]
fn test_parse_query_lenient() {
    assert!(parse_query_full("password=secret&serviceName=somechat&", false, false).is_err());
    let map = parse_query_full("password=secret&serviceName=somechat&", false, true).unwrap();
    assert_eq!(2, map.len());
    assert_eq!("secret", map["password"]);
    assert_eq!("somechat", map["serviceName"]);
}

#[test]
fn test_scan_raw() {
    assert_eq!(
        vec![Pair::new(9, 21)],
        scan_raw("password=RAW(++?5w0rd)&serviceName=somechat")
    );
    assert_eq!(
        vec![Pair::new(9, 21)],
        scan_raw("password=RAW{++?5w0rd}&serviceName=somechat")
    );
    assert_eq!(
        vec![Pair::new(9, 21)],
        scan_raw("password=RAW{++?)&0rd}&serviceName=somechat")
    );
    assert_eq!(
        vec![Pair::new(10, 22), Pair::new(34, 46)],
        scan_raw("password1=RAW(++?}&0rd)&password2=RAW{++?)&0rd}&serviceName=somechat")
    );
}

#[test]
fn test_is_raw() {
    let pairs = [Pair::new(3, 5), Pair::new(8, 10)];
    for i in 0..3 {
        assert!(!is_raw(i, &pairs));
    }
    for i in 3..6 {
        assert!(is_raw(i, &pairs));
    }
    for i in 6..8 {
        assert!(!is_raw(i, &pairs));
    }
    for i in 8..11 {
        assert!(is_raw(i, &pairs));
    }
    for i in 11..15 {
        assert!(!is_raw(i, &pairs));
    }
}

#[test]
fn test_resolve_raw_parameter_values() {
    let cases = [
        ("password=secret&serviceName=somechat", "secret"),
        ("password=RAW(++?w0rd)&serviceName=somechat", "++?w0rd"),
        ("password=RAW(++?)w&rd)&serviceName=somechat", "++?)w&rd"),
        ("password=RAW{++?w0rd}&serviceName=somechat", "++?w0rd"),
        ("password=RAW{++?)w&rd}&serviceName=somechat", "++?)w&rd"),
    ];
    for (q, password) in cases {
        let mut map = parse_query(q).unwrap();
        resolve_raw_parameter_values(&mut map);
        assert_eq!(2, map.len());
        assert_eq!(password, map["password"]);
        assert_eq!("somechat", map["serviceName"]);
    }
    let mut map = parse_query("a=RAW(x)&a=RAW(y%25)").unwrap();
    resolve_raw_parameter_values_with(&mut map, |s| s.to_uppercase());
    assert_eq!(ParamValue::List(vec!["X".into(), "Y%".into()]), map["a"]);
}

#[test]
fn test_append_parameter_to_uri_and_replace_existing_one() {
    let new_parameters = params(&[("foo", "456"), ("bar", "yes")]);
    assert_eq!(
        "stub://foo?foo=456&bar=yes",
        append_parameters_to_uri("stub:foo?foo=123", &new_parameters).unwrap()
    );
}

#[test]
fn test_path_and_query_of() {
    assert_eq!("/", path_and_query_of(&JavaUri::create("http://localhost")));
    assert_eq!(
        "/",
        path_and_query_of(&JavaUri::create("http://localhost:80"))
    );
    assert_eq!(
        "/",
        path_and_query_of(&JavaUri::create("http://localhost:80/"))
    );
    assert_eq!(
        "/path",
        path_and_query_of(&JavaUri::create("http://localhost:80/path"))
    );
    assert_eq!(
        "/path/",
        path_and_query_of(&JavaUri::create("http://localhost:80/path/"))
    );
    assert_eq!(
        "/path?query=value",
        path_and_query_of(&JavaUri::create("http://localhost:80/path?query=value"))
    );
}

#[test]
fn should_strip_prefixes() {
    assert_eq!("", strip_prefix("", ""));
    assert_eq!("a", strip_prefix("a", "b"));
    assert_eq!("", strip_prefix("a", "a"));
    assert_eq!("ab", strip_prefix("ab", "b"));
    assert_eq!("a", strip_prefix("a", "ab"));
}

#[test]
fn should_strip_suffixes() {
    assert_eq!("", strip_suffix("", ""));
    assert_eq!("a", strip_suffix("a", "b"));
    assert_eq!("", strip_suffix("a", "a"));
    assert_eq!("a", strip_suffix("ab", "b"));
    assert_eq!("a", strip_suffix("a", "ab"));
}

#[test]
fn should_join_paths() {
    assert_eq!("", join_paths(&[None, None]));
    assert_eq!("", join_paths(&[Some(""), None]));
    assert_eq!("", join_paths(&[None, Some("")]));
    assert_eq!("", join_paths(&[Some(""), Some("")]));
    assert_eq!("", join_paths(&[]));
    assert_eq!("a", join_paths(&[Some("a"), Some("")]));
    assert_eq!("a/b", join_paths(&[Some("a"), Some("b")]));
    assert_eq!("/a/b", join_paths(&[Some("/a"), Some("b")]));
    assert_eq!("/a/b/", join_paths(&[Some("/a"), Some("b/")]));
    assert_eq!("/a/b/", join_paths(&[Some("/a/"), Some("b/")]));
    assert_eq!("/a/b/", join_paths(&[Some("/a/"), Some("/b/")]));
    assert_eq!("a/b/c", join_paths(&[Some("a"), Some("b"), Some("c")]));
    assert_eq!("a/c", join_paths(&[Some("a"), None, Some("c")]));
    assert_eq!(
        "/a/b/c/d/",
        join_paths(&[Some("/a/"), Some("/b"), Some("c/"), Some("/d/")])
    );
    assert_eq!(
        "/a/b/c/",
        join_paths(&[Some("/a/"), Some("/b"), Some("c/"), None])
    );
    assert_eq!("/a/", join_paths(&[Some("/a/"), None, None, None]));
    assert_eq!("a/b", join_paths(&[Some("a/"), Some("/b"), None, None]));
}

#[test]
fn test_extract_query() {
    assert_eq!(None, extract_query(""));
    assert_eq!(None, extract_query("file:foo"));
    assert_eq!(
        Some("recursive=true".into()),
        extract_query("file:foo?recursive=true")
    );
    assert_eq!(
        Some("recursive=true&delete=true".into()),
        extract_query("file:foo?recursive=true&delete=true")
    );
}

#[test]
fn test_plus_in_query() {
    assert_eq!(
        "param1=%2B447777111222",
        create_query_string(&params(&[("param1", "+447777111222")]))
    );
    // will be double encoded however
    assert_eq!(
        "param1=%252B447777111222",
        create_query_string(&params(&[("param1", "%2B447777111222")]))
    );
}

#[test]
fn test_build_multi_value_query() {
    assert_eq!("", build_multi_value_query("id", Vec::<String>::new()));
    assert_eq!("id=hello", build_multi_value_query("id", ["hello"]));
    assert_eq!("id=1&id=2&id=3", build_multi_value_query("id", [1, 2, 3]));
    let mixed: Vec<Box<dyn fmt::Display>> = vec![
        Box::new("foo"),
        Box::new("bar"),
        Box::new(3),
        Box::new(true),
        Box::new("baz"),
    ];
    assert_eq!(
        "hey=foo&hey=bar&hey=3&hey=true&hey=baz",
        build_multi_value_query("hey", mixed)
    );
}

#[test]
fn test_get_decode_query() {
    let out = n("smtp://localhost?username=davsclaus&password=secret");
    let enc = encode(&out);
    let dec = get_decode_query(&enc).unwrap();
    assert_eq!(out, dec);
    let out = n("smtp://localhost?password=secret&username=davsclaus");
    assert_eq!(out, dec);

    let out = n("http://localhost?username=davsclaus&password=RAW(#@a)");
    let enc = encode(&out);
    assert_ne!(out, enc);
    assert_eq!(out, get_decode_query(&enc).unwrap());

    let out = n("bean://MyBean?method=RAW(addString(%22#@a%23, test))");
    let enc = encode(&out);
    assert_ne!(out, enc);
    assert_eq!(out, get_decode_query(&enc).unwrap());
}

#[test]
fn test_sanitize_custom_keys() {
    let _guard = keywords_guard();
    let uri1 = "http://foo?username=me&password=secret&domain=foobar&database=customers";
    let expected = "http://foo?username=xxxxxx&password=xxxxxx&domain=foobar&database=customers";
    assert_eq!(expected, sanitize_uri(uri1));

    add_sanitize_keywords(Some("domain,system,cheese"));
    assert_ne!(expected, sanitize_uri(uri1));
    assert_eq!(
        "http://foo?username=xxxxxx&password=xxxxxx&domain=xxxxxx&database=customers",
        sanitize_uri(uri1)
    );
}

#[test]
fn test_sanitize_custom_keys_applied_to_all_parameters() {
    let _guard = keywords_guard();
    add_sanitize_keywords(Some("pincode"));
    assert_eq!(
        "my:host?pincode=xxxxxx&pincode=xxxxxx",
        sanitize_uri("my:host?pincode=1111&pincode=2222")
    );

    // keywords added by an earlier call are kept
    add_sanitize_keywords(Some("otp-code"));
    assert_eq!(
        "my:host?otpCode=xxxxxx&pincode=xxxxxx&otp-code=xxxxxx",
        sanitize_uri("my:host?otpCode=123456&pincode=1111&otp-code=654321")
    );

    // only built-in or empty keywords, which must not mask any other parameter
    add_sanitize_keywords(Some("password, "));
    add_sanitize_keywords(None);
    assert_eq!(
        "my:host?foo=bar&timeout=5000",
        sanitize_uri("my:host?foo=bar&timeout=5000")
    );
}

#[test]
fn test_sanitize_uri_raw_value_with_closing_bracket() {
    assert_eq!(
        "ftp://host/dir?password=xxxxxx&binary=true",
        sanitize_uri("ftp://host/dir?password=RAW(se)cret)&binary=true")
    );
    assert_eq!(
        "ftp://host/dir?password=xxxxxx&binary=true",
        sanitize_uri("ftp://host/dir?password=RAW{se}cret}&binary=true")
    );
    assert_eq!(
        "ftp://host/dir?binary=true&password=xxxxxx",
        sanitize_uri("ftp://host/dir?binary=true&password=RAW()s3cr3t)")
    );
    assert_eq!(
        "foo:bar?password=xxxxxx&serviceName=somechat",
        sanitize_uri("foo:bar?password=RAW(++?)w&rd)&serviceName=somechat")
    );
}

#[test]
fn test_sanitize_uri_raw_value_in_text() {
    assert_eq!(
        "from[ftp://host/dir?password=xxxxxx]",
        sanitize_uri("from[ftp://host/dir?password=RAW(se)cret)]")
    );
    assert_eq!(
        "from[ftp://host/dir?password=xxxxxx",
        sanitize_uri("from[ftp://host/dir?password=RAW(secr...]")
    );
}

#[test]
fn test_sanitize_uri_with_user_info_and_other_uri() {
    assert_eq!(
        "ftp://joe:xxxxxx@ftp.example.com/in?callbackUrl=http://cb.example.com:8080/x&notify=ops@example.com",
        sanitize_uri("ftp://joe:s3cr3t@ftp.example.com/in?callbackUrl=http://cb.example.com:8080/x&notify=ops@example.com")
    );
    assert_eq!(
        "ftp://joe:xxxxxx@ftp.example.com/in",
        sanitize_uri("ftp://joe:p@ss@ftp.example.com/in")
    );
    assert_eq!(
        "From[ftp://joe:xxxxxx@host1/in] -> To[sftp://bob:xxxxxx@host2/out]",
        sanitize_uri("From[ftp://joe:secret1@host1/in] -> To[sftp://bob:secret2@host2/out]")
    );
    assert_eq!(
        "smtp://host:25?to=ops@example.com",
        sanitize_uri("smtp://host:25?to=ops@example.com")
    );
    assert_eq!(
        "ftp://joe@host:21/dir?to=ops@example.com",
        sanitize_uri("ftp://joe@host:21/dir?to=ops@example.com")
    );
}

#[test]
fn test_sanitize_uri_dash_case_keys() {
    assert_eq!(
        "my:host?access-key=xxxxxx&private-key=xxxxxx&connection-string=xxxxxx&sasl-jaas-config=xxxxxx&pass-phrase=xxxxxx&foo-bar=keep",
        sanitize_uri("my:host?access-key=A1&private-key=P2&connection-string=C3&sasl-jaas-config=J4&pass-phrase=P5&foo-bar=keep")
    );
}

#[test]
fn test_sanitize_uri_nested_uri() {
    assert_eq!(
        "http://host/cb?url=http://other/path?token=xxxxxx&foo=bar",
        sanitize_uri("http://host/cb?url=http://other/path?token=abc&foo=bar")
    );
}

#[test]
fn test_sanitize_parameters() {
    let mut parameters = Parameters::new();
    parameters.insert("password".into(), "secret".into());
    parameters.insert("private-key".into(), "pk".into());
    parameters.insert("token".into(), ParamValue::Null);
    parameters.insert("foo".into(), "bar".into());
    let sanitized = sanitize_parameters(&parameters);
    let entries: Vec<(&str, ParamValue)> = sanitized
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    assert_eq!(
        vec![
            ("password", ParamValue::from("xxxxxx")),
            ("private-key", ParamValue::from("xxxxxx")),
            ("token", ParamValue::Null),
            ("foo", ParamValue::from("bar")),
        ],
        entries
    );
    // the given parameters are not changed
    assert_eq!("secret", parameters["password"]);
}

#[test]
fn test_sanitize_uri_with_user_info_password_with_slash_or_question_mark() {
    assert_eq!(
        "ftp://joe:xxxxxx@host/dir",
        sanitize_uri("ftp://joe:pa/ss@host/dir")
    );
    assert_eq!(
        "ftp://joe:xxxxxx@host/dir?binary=true",
        sanitize_uri("ftp://joe:pa?ss@host/dir?binary=true")
    );
    assert_eq!(
        "ftp://joe:xxxxxx@host/dir",
        sanitize_uri("ftp://joe:p/a@b@host/dir")
    );
    // a password that contains a query parameter (?key=value) cannot be told apart from
    // host:port?key=value@... (see smtp://host:25?to=ops@example.com), so it is not masked
    assert_eq!(
        "ftp://joe:p?a=b@host/dir",
        sanitize_uri("ftp://joe:p?a=b@host/dir")
    );
}

#[test]
fn test_sanitize_path_with_user_info_and_other_at() {
    assert_eq!(
        "joe:xxxxxx@host/in?to=ops@example.com",
        sanitize_path("joe:secret@host/in?to=ops@example.com")
    );
    assert_eq!("joe:xxxxxx@host/dir", sanitize_path("joe:pa/ss@host/dir"));
}

// --- additional coverage of helpers without a dedicated Java test ---

#[test]
fn test_text_block_and_noise() {
    assert_eq!(
        "sql:select * from t?a=1&b=2",
        text_block_to_single_line("sql:select *\r\n  from t?a=1\n  &b=2\n")
    );
    assert_eq!(
        "foo:bar?a=1&b=2",
        remove_noise_from_uri("foo:bar?a=1&\n   b=2")
    );
    assert_eq!("foo:bar?a=1&b=2", remove_noise_from_uri("foo:bar?a=1&b=2"));
}

#[test]
fn test_extract_remainder_and_properties() {
    let u = uri("file://target/in?noop=true");
    assert_eq!("target/in", extract_remainder_path(&u, false));
    let mut props = params(&[("foo.a", "1"), ("bar", "2"), ("foo.b", "3")]);
    let extracted = extract_properties(&mut props, "foo.");
    assert_eq!(params(&[("a", "1"), ("b", "3")]), extracted);
    assert_eq!(params(&[("bar", "2")]), props);
    assert_eq!("text", strip_query("text?x=1"));
}

// --- URIScannerTest ---

#[test]
fn test_not_raw_uri_scanner() {
    assert_eq!(None, resolve_raw("foo"));
}

#[test]
fn test_uri_scanner_raw_type1() {
    assert_eq!(Some("++?w0rd".to_string()), resolve_raw("RAW(++?w0rd)"));
}

#[test]
fn test_uri_scanner_type2() {
    assert_eq!(Some("++?w0rd".to_string()), resolve_raw("RAW{++?w0rd}"));
}
