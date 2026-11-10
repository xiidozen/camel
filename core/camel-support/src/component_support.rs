//! The generic part of creating endpoints from uris (Java
//! `org.apache.camel.support.DefaultComponent`): parse the uri, extract the remaining path and
//! the query parameters, let the component create the endpoint, and reject the parameters the
//! component did not use.

use camel_api::context::CamelContext;
use camel_api::convert::FromValue;
use camel_api::endpoint::{Component, SharedEndpoint};
use camel_api::error::{box_error, CamelError, CamelResult, ErrorKind};
use camel_util::net::JavaUri;
use camel_util::uri_support::{
    extract_remainder_path, parse_parameters, sanitize_parameters, ParamValue, Parameters,
};
use camel_util::value::Value;
use std::sync::Arc;

/// Creates the endpoint for the uri with the component (Java `DefaultComponent.createEndpoint`).
///
/// # Errors
///
/// Returns an error if the uri is invalid, the component fails to create the endpoint, or
/// (unless the endpoint is lenient) some uri parameters are not known by the component.
pub fn create_endpoint(
    component: &dyn Component,
    context: &Arc<dyn CamelContext>,
    uri: &str,
) -> CamelResult<SharedEndpoint> {
    let encoded = camel_util::unsafe_uri_characters_encoder::encode(uri);
    let u = JavaUri::parse(&encoded)
        .map_err(|e| CamelError::resolve_endpoint_failed(uri, box_error(e)))?;
    let path = if u.scheme().is_some() {
        extract_remainder_path(&u, component.use_raw_uri())
    } else {
        String::new()
    };
    let mut parameters = if component.use_raw_uri() {
        let query = camel_util::string_helper::after(uri, "?")
            .or_else(|| u.raw_query().map(str::to_string));
        match query {
            Some(q) => parse_query_raw(&q)
                .map_err(|e| CamelError::resolve_endpoint_failed(uri, box_error(e)))?,
            None => Parameters::new(),
        }
    } else {
        parse_parameters(&u).map_err(|e| CamelError::resolve_endpoint_failed(uri, box_error(e)))?
    };
    // the hash option is internal only
    parameters.shift_remove("hash");
    let uri = if component.use_raw_uri() {
        uri.to_string()
    } else {
        encoded
    };
    log::debug!(
        "Creating endpoint uri=[{}], path=[{}]",
        camel_util::uri_support::sanitize_uri(&uri),
        path
    );
    let endpoint = component.create_endpoint(context, &uri, &path, &mut parameters)?;
    if !endpoint.is_lenient_properties() {
        validate_parameters(&uri, &parameters)?;
    }
    Ok(endpoint)
}

fn parse_query_raw(query: &str) -> Result<Parameters, camel_util::net::UriSyntaxError> {
    camel_util::uri_support::parse_query_full(query, true, false)
}

/// Formats the parameters like Java's `Map.toString()`: `{a=1, b=2}`.
pub fn parameters_to_string(parameters: &Parameters) -> String {
    let body: Vec<String> = parameters.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{{{}}}", body.join(", "))
}

/// Fails if there are parameters left over (that the component did not use).
///
/// # Errors
///
/// Returns a `ResolveEndpointFailedException` listing the unknown parameters (with sensitive
/// values masked).
pub fn validate_parameters(uri: &str, parameters: &Parameters) -> CamelResult<()> {
    if parameters.is_empty() {
        return Ok(());
    }
    Err(CamelError::resolve_endpoint_failed_message(
        uri,
        &format!(
            "There are {} parameters that couldn't be set on the endpoint. Check the uri if the parameters are spelt correctly and that they are properties of the endpoint. Unknown parameters=[{}]",
            parameters.len(),
            parameters_to_string(&sanitize_parameters(parameters))
        ),
    ))
}

/// Removes the parameter and converts it to the type (Java `getAndRemoveParameter`).
///
/// # Returns
///
/// The value, or `None` if the parameter is absent.
///
/// # Errors
///
/// Returns a `PropertyBindingException` if the value cannot be converted.
///
/// # Examples
///
/// ```
/// use camel_support::component_support::get_and_remove_parameter;
/// use camel_util::uri_support::{ParamValue, Parameters};
///
/// let mut p = Parameters::new();
/// p.insert("timeout".into(), ParamValue::from("5000"));
/// assert_eq!(Some(5000i64), get_and_remove_parameter::<i64>(&mut p, "timeout").unwrap());
/// assert!(p.is_empty());
/// ```
pub fn get_and_remove_parameter<T: FromValue>(
    parameters: &mut Parameters,
    name: &str,
) -> CamelResult<Option<T>> {
    let Some(raw) = parameters.shift_remove(name) else {
        return Ok(None);
    };
    let value = match &raw {
        ParamValue::Null => Value::Null,
        ParamValue::Text(s) => Value::from(s.as_str()),
        ParamValue::List(l) => Value::List(l.iter().map(|s| Value::from(s.as_str())).collect()),
    };
    T::from_value(&value).map_err(|e| {
        CamelError::with_cause(
            ErrorKind::PropertyBinding,
            format!("Error binding property ({name}={raw}) with name: {name}"),
            e,
        )
    })
}

/// Removes the parameter and converts it to the type, or returns the default value.
///
/// # Errors
///
/// Returns a `PropertyBindingException` if the value cannot be converted.
pub fn get_and_remove_parameter_or<T: FromValue>(
    parameters: &mut Parameters,
    name: &str,
    default_value: T,
) -> CamelResult<T> {
    Ok(get_and_remove_parameter(parameters, name)?.unwrap_or(default_value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use camel_api::context::StandaloneContext;
    use camel_api::endpoint::{Endpoint, SharedConsumer, SharedProducer};
    use camel_api::processor::SharedProcessor;
    use camel_api::service::Service;
    use std::any::Any;

    struct TestEndpoint {
        uri: String,
        path: String,
        size: i64,
        lenient: bool,
    }
    impl Service for TestEndpoint {}
    impl Endpoint for TestEndpoint {
        fn endpoint_uri(&self) -> &str {
            &self.uri
        }
        fn camel_context(&self) -> Arc<dyn CamelContext> {
            StandaloneContext::shared()
        }
        fn create_producer(self: Arc<Self>) -> CamelResult<SharedProducer> {
            Err(CamelError::runtime("n/a"))
        }
        fn create_consumer(self: Arc<Self>, _p: SharedProcessor) -> CamelResult<SharedConsumer> {
            Err(CamelError::runtime("n/a"))
        }
        fn is_lenient_properties(&self) -> bool {
            self.lenient
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct TestComponent;
    impl Service for TestComponent {}
    impl Component for TestComponent {
        fn create_endpoint(
            &self,
            _context: &Arc<dyn CamelContext>,
            uri: &str,
            remaining: &str,
            parameters: &mut Parameters,
        ) -> CamelResult<SharedEndpoint> {
            let size = get_and_remove_parameter_or(parameters, "size", 10i64)?;
            let lenient = get_and_remove_parameter_or(parameters, "lenient", false)?;
            Ok(Arc::new(TestEndpoint {
                uri: uri.to_string(),
                path: remaining.to_string(),
                size,
                lenient,
            }))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn create(uri: &str) -> CamelResult<SharedEndpoint> {
        create_endpoint(&TestComponent, &StandaloneContext::shared(), uri)
    }

    #[test]
    fn creates_with_path_and_parameters() {
        let e = create("test://foo/bar?size=5&hash=x").unwrap();
        let t = e.as_any().downcast_ref::<TestEndpoint>().unwrap();
        assert_eq!("foo/bar", t.path);
        assert_eq!(5, t.size);
        assert_eq!("test://foo/bar?size=5&hash=x", t.uri);
        let d = create("test:baz").unwrap();
        assert_eq!(10, d.as_any().downcast_ref::<TestEndpoint>().unwrap().size);
    }

    #[test]
    fn unknown_parameters() {
        let err = create("test://foo?size=5&foo=bar&password=secret")
            .err()
            .unwrap();
        assert!(err.is(ErrorKind::ResolveEndpointFailed));
        assert_eq!(
            Some("Failed to resolve endpoint: test://foo?size=5&foo=bar&password=xxxxxx due to: There are 2 parameters that couldn't be set on the endpoint. Check the uri if the parameters are spelt correctly and that they are properties of the endpoint. Unknown parameters=[{foo=bar, password=xxxxxx}]"),
            err.message()
        );
        assert!(create("test://foo?lenient=true&foo=bar").is_ok());
    }

    #[test]
    fn bad_conversion() {
        let err = create("test://foo?size=abc").err().unwrap();
        assert!(err.is(ErrorKind::PropertyBinding));
        assert!(err
            .message()
            .unwrap()
            .starts_with("Error binding property (size=abc) with name: size"));
        assert_eq!("{}", parameters_to_string(&Parameters::new()));
    }
}
