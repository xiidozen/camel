//! Helpers for endpoint uris (Java `EndpointHelper`): resolving their property placeholders.

use camel_api::context::CamelContext;
use camel_api::error::{CamelError, CamelResult};
use camel_api::spi::properties::{OPTIONAL_TOKEN, PREFIX_TOKEN, SUFFIX_TOKEN};
use camel_util::uri_support::{create_query_string, parse_query, ParamValue, Parameters};

/// The prefix of an optional placeholder, `{{?`.
fn optional_prefix() -> String {
    format!("{PREFIX_TOKEN}{OPTIONAL_TOKEN}")
}

/// The text with its placeholders resolved, keeping the optional placeholders of missing
/// properties (Java `resolvePropertyPlaceholders(text, true)`).
fn resolve_keep(context: &dyn CamelContext, text: &str) -> CamelResult<String> {
    if !text.contains(PREFIX_TOKEN) {
        return Ok(text.to_string());
    }
    match context.properties_component() {
        Some(pc) => Ok(pc
            .parse_uri_keep_unresolved(text, true)?
            .unwrap_or_default()),
        None => Ok(text.to_string()),
    }
}

/// Resolves the property placeholders of the endpoint uri (Java
/// `EndpointHelper.resolveEndpointUriPropertyPlaceholders`): the context path and the query
/// are resolved separately, and the parameters whose value is an optional placeholder of a
/// missing property are removed (`mock:a?retainFirst={{?max}}` is `mock:a` without `max`).
///
/// # Errors
///
/// A `ResolveEndpointFailedException` for a missing property.
pub fn resolve_endpoint_uri_property_placeholders(
    context: &dyn CamelContext,
    uri: &str,
) -> CamelResult<String> {
    if uri.is_empty() || !uri.contains(PREFIX_TOKEN) {
        return Ok(uri.to_string());
    }
    let failed =
        |e: CamelError| CamelError::resolve_endpoint_failed(uri, camel_api::error::box_error(e));
    let prefix = optional_prefix();
    let pos = uri.find(PREFIX_TOKEN);
    let pos1 = uri.find('?');
    let pos2 = uri.find(&prefix);
    let mut resolved = match (pos, pos1) {
        // split only when there are query parameters, and the optional tokens are later
        (Some(_), Some(p1)) if p1 != uri.len() - 1 && pos2.map_or(true, |p2| p2 + 2 > p1) => {
            let u1 = resolve_keep(context, &uri[..p1]).map_err(failed)?;
            let u2 = resolve_keep(context, &uri[p1 + 1..]).map_err(failed)?;
            if u1.contains('?') {
                format!("{u1}&{u2}")
            } else {
                format!("{u1}?{u2}")
            }
        }
        _ => resolve_keep(context, uri).map_err(failed)?,
    };
    if resolved.contains(&prefix) {
        resolved = remove_unresolved_optional(&resolved).map_err(failed)?;
    }
    Ok(resolved)
}

/// Removes the optional placeholders that could not be resolved (Java
/// `doResolveEndpointUriOptionalPropertyPlaceholders`).
fn remove_unresolved_optional(uri: &str) -> CamelResult<String> {
    let prefix = optional_prefix();
    // the query starts at the first ? that is not part of an optional token
    let bytes = uri.as_bytes();
    let mut pos = 0;
    for (i, &ch) in bytes.iter().enumerate() {
        if ch == b'?' {
            if i > 2 {
                if bytes[i - 1] != b'{' && bytes[i - 2] != b'{' {
                    pos = i;
                    break;
                }
            } else {
                pos = i;
                break;
            }
        }
    }
    let (mut base, query) = if pos > 0 {
        (uri[..pos].to_string(), Some(uri[pos + 1..].to_string()))
    } else {
        (uri.to_string(), None)
    };
    // the context path removes all the unresolved placeholders (Java regex \{\{?.*}})
    if let Some(start) = base.find('{') {
        if let Some(end) = base.rfind(SUFFIX_TOKEN).filter(|&e| e > start) {
            base.replace_range(start..end + SUFFIX_TOKEN.len(), "");
        }
    }
    let query = match query {
        Some(q) if q.contains(&prefix) => {
            let params =
                parse_query(&q).map_err(|e| CamelError::illegal_argument(e.to_string()))?;
            let keep: Parameters = params
                .into_iter()
                .filter(|(k, v)| !k.starts_with(&prefix) && !is_unresolved(v, &prefix))
                .collect();
            Some(create_query_string(&keep))
        }
        other => other,
    };
    Ok(match query {
        Some(q) if !q.is_empty() => format!("{base}?{q}"),
        _ => base,
    })
}

/// Whether the parameter value is an unresolved optional placeholder (also in `RAW(..)` or
/// after a resource scheme such as `file:`).
fn is_unresolved(value: &ParamValue, prefix: &str) -> bool {
    let Some(s) = value.as_str() else {
        return false;
    };
    if s.starts_with(prefix) {
        return true;
    }
    if let Some(inner) = s.strip_prefix("RAW(").and_then(|r| r.strip_suffix(')')) {
        if inner.starts_with(prefix) {
            return true;
        }
    }
    if let Some(dot) = s.find(':') {
        if dot > 0 && dot < s.len() - 1 && s[dot + 1..].starts_with(prefix) {
            return true;
        }
    }
    false
}

/// The uri with the scheme separator toggled between `scheme:path` and `scheme://path`.
fn toggle_uri_scheme_separators(uri: &str) -> String {
    match uri.split_once("://") {
        Some((scheme, path)) => format!("{scheme}:{path}"),
        None => match uri.split_once(':') {
            Some((scheme, path)) => format!("{scheme}://{path}"),
            None => uri.to_string(),
        },
    }
}

fn do_match_endpoint(uri: &str, pattern: &str, regexp: bool) -> bool {
    let m = |u: &str| {
        if regexp {
            crate::pattern_helper::match_regex(u, pattern)
        } else {
            crate::pattern_helper::match_pattern(u, pattern)
        }
    };
    let toggled = toggle_uri_scheme_separators(uri);
    if m(uri) || m(&toggled) {
        return true;
    }
    if !regexp && pattern.contains('?') {
        // the pattern normalized, so the parameters are in the same order as in the uri
        if let Ok(p) = camel_util::uri_support::normalize_uri(pattern) {
            return uri.eq_ignore_ascii_case(&p) || toggled.eq_ignore_ascii_case(&p);
        }
    }
    false
}

/// Whether the endpoint uri matches the pattern (Java `EndpointHelper.matchEndpoint`): the
/// uri (its property placeholders resolved, normalized) is the pattern, matches it as a
/// wildcard (`*`) or regex pattern, with or without the `//` of the scheme.
///
/// # Errors
///
/// A missing property of the uri.
///
/// # Examples
///
/// ```
/// use camel_api::context::StandaloneContext;
/// use camel_support::endpoint_helper::match_endpoint;
///
/// let context = StandaloneContext::shared();
/// assert!(match_endpoint(context.as_ref(), "mock:foo", "mock:foo").unwrap());
/// assert!(match_endpoint(context.as_ref(), "mock://foo", "mock:*").unwrap());
/// assert!(match_endpoint(context.as_ref(), "mock:foo?b=2&a=1", "mock:foo?a=1&b=2").unwrap());
/// assert!(!match_endpoint(context.as_ref(), "mock:bar", "mock:foo").unwrap());
/// ```
pub fn match_endpoint(context: &dyn CamelContext, uri: &str, pattern: &str) -> CamelResult<bool> {
    let resolved = resolve_endpoint_uri_property_placeholders(context, uri)?;
    let normalized = camel_util::uri_support::normalize_uri(&resolved)
        .map_err(|e| CamelError::resolve_endpoint_failed(uri, camel_api::error::box_error(e)))?;
    Ok(do_match_endpoint(&normalized, pattern, false)
        || do_match_endpoint(&normalized, pattern, true))
}

#[cfg(test)]
mod tests {
    use super::remove_unresolved_optional;

    #[test]
    fn removes_unresolved_optional_placeholders() {
        assert_eq!(
            "mock:result?resultWaitTime=5000",
            remove_unresolved_optional("mock:result?retainFirst={{?maxKeep}}&resultWaitTime=5000")
                .unwrap()
        );
        assert_eq!("foo:", remove_unresolved_optional("foo:{{?bar}}").unwrap());
        assert_eq!(
            "foo:bar",
            remove_unresolved_optional("foo:bar?a={{?x}}").unwrap()
        );
    }
}
