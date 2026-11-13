//! Port of the camel-jsoup `SimpleJSoupTest`.

use camel_core::api::context::CamelContext;
use camel_core::prelude::*;
use camel_jsoup::HtmlDocument;
use std::sync::Arc;

fn evaluate(c: &Arc<DefaultCamelContext>, body: &str, expression: &str) -> Value {
    let shared: Arc<dyn CamelContext> = c.clone();
    let mut e = Exchange::new(shared);
    e.message_mut().set_body(body);
    let exp = c
        .resolve_language("simple")
        .unwrap()
        .create_expression(expression)
        .unwrap();
    exp.init(&(c.clone() as Arc<dyn CamelContext>)).unwrap();
    exp.evaluate(&e).unwrap()
}

#[test]
fn test_html_functions() {
    let c = camel_core::default_context();
    camel_jsoup::register(&c);
    let link = "<p><a href='https://example.com/' onclick='stealCookies()'>Link</a></p>";
    for exp in ["${htmlClean()}", "${htmlClean(${body})}"] {
        assert_eq!(
            Value::from("<p><a href=\"https://example.com/\" rel=\"nofollow\">Link</a></p>"),
            evaluate(&c, link, exp)
        );
    }
    let html = "<html><title>My Camel</title><body><p>Some blah blah</p></body></html>";
    for exp in ["${htmlParse()}", "${htmlParse(${body})}"] {
        let Value::Object(o) = evaluate(&c, html, exp) else {
            panic!("a document")
        };
        let d = o.downcast_ref::<HtmlDocument>().unwrap();
        assert_eq!("My Camel", d.title());
        assert_eq!("Some blah blah", d.body().text());
    }
    for exp in ["${htmlDecode()}", "${htmlDecode(${body})}"] {
        assert_eq!(
            Value::from("My Camel Some blah blah"),
            evaluate(&c, html, exp)
        );
    }
}
