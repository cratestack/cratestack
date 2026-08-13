use axum::http::{HeaderMap, header};
use cratestack_core::{CoolError, RouteTransportCapabilities};

use super::http_transport::HttpTransport;

pub(crate) fn validate_transport_accept_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(());
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    if supported
        .iter()
        .any(|content_type| accepts_content_type(accept, content_type))
    {
        Ok(())
    } else {
        Err(CoolError::NotAcceptable(format!(
            "router only serves {} responses",
            supported.join(", "),
        )))
    }
}

pub(crate) fn validate_transport_content_type_header(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<(), CoolError> {
    request_content_type(headers, supported).map(|_| ())
}

pub(crate) fn request_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
) -> Result<&'static str, CoolError> {
    let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
        return Err(CoolError::UnsupportedMediaType(format!(
            "expected Content-Type one of {}",
            supported.join(", "),
        )));
    };
    let content_type = content_type
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Content-Type header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|expected| media_type_matches(content_type, expected))
        .ok_or_else(|| {
            CoolError::UnsupportedMediaType(format!(
                "expected Content-Type one of {}, got {}",
                supported.join(", "),
                content_type,
            ))
        })
}

/// Negotiates the response `Content-Type` for one route against the
/// concrete `transport`'s real encoders, not just `capabilities`'s
/// compile-time `response_types` list (cratestack#489): a router built
/// with a single `JsonCodec` still emits a `response_types` list naming
/// both `application/cbor` and `application/json`, so picking straight
/// from that list can select a type the router has no encoder for at
/// all, producing a 406 the caller never sees coming (`main`'s bug).
///
/// `capabilities.response_types` empty is a sentinel meaning "no
/// capabilities declared" — only the capability-free
/// `encode_transport_result`/`encode_transport_sequence_result` (and
/// `_with_status`) convenience wrappers pass that, and nothing in this
/// workspace calls them — so `default_response_type` is trusted as-is
/// there, same as before this fix, rather than inventing a policy for an
/// unconfigured caller.
pub(crate) fn select_transport_response_content_type<T>(
    transport: &T,
    headers: &HeaderMap,
    capabilities: &RouteTransportCapabilities,
) -> Result<&'static str, CoolError>
where
    T: HttpTransport,
{
    if capabilities.response_types.is_empty() {
        return Ok(capabilities.default_response_type);
    }
    let encodable: Vec<&'static str> = capabilities
        .response_types
        .iter()
        .copied()
        .filter(|content_type| transport.can_encode(content_type))
        .collect();
    select_response_content_type(headers, &encodable, capabilities.default_response_type)
}

/// Picks the response `Content-Type`, from `encodable` — the subset of a
/// route's advertised `response_types` the router's concrete transport can
/// actually encode (see [`select_transport_response_content_type`]) —
/// that best matches the client's `Accept` header. `encodable` is
/// pre-filtered by the caller, so the `NotAcceptable` this returns always
/// names something the router can genuinely produce.
pub(crate) fn select_response_content_type(
    headers: &HeaderMap,
    encodable: &[&'static str],
    default: &'static str,
) -> Result<&'static str, CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        if encodable.contains(&default) {
            return Ok(default);
        }
        return encodable
            .first()
            .copied()
            .ok_or_else(|| not_acceptable_response(encodable));
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    encodable
        .iter()
        .copied()
        .find(|content_type| accepts_content_type(accept, content_type))
        .ok_or_else(|| not_acceptable_response(encodable))
}

fn not_acceptable_response(encodable: &[&'static str]) -> CoolError {
    if encodable.is_empty() {
        CoolError::NotAcceptable("router has no response encoder configured".to_owned())
    } else {
        CoolError::NotAcceptable(format!(
            "router only serves {} responses",
            encodable.join(", "),
        ))
    }
}

pub(crate) fn accepts_content_type(accept: &str, expected: &str) -> bool {
    accept.split(',').map(str::trim).any(|value| {
        if value == "*/*" {
            return true;
        }
        let media_type = strip_media_type_params(value);
        media_type == expected
            || media_type == wildcard_media_type(expected)
            || media_type == "application/*"
    })
}

pub(crate) fn media_type_matches(candidate: &str, expected: &str) -> bool {
    strip_media_type_params(candidate) == expected
}

pub(crate) fn strip_media_type_params(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

pub(crate) fn wildcard_media_type(content_type: &str) -> &str {
    content_type
        .split_once('/')
        .map(|(prefix, _)| {
            if prefix == "application" {
                "application/*"
            } else {
                "*/*"
            }
        })
        .unwrap_or("*/*")
}

#[cfg(test)]
mod tests;
