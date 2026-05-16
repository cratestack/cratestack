use axum::http::{HeaderMap, header};
use cratestack_core::CoolError;

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

pub(crate) fn select_response_content_type(
    headers: &HeaderMap,
    supported: &[&'static str],
    default: &'static str,
) -> Result<&'static str, CoolError> {
    let Some(accept) = headers.get(header::ACCEPT) else {
        return Ok(default);
    };
    let accept = accept
        .to_str()
        .map_err(|error| CoolError::BadRequest(format!("invalid Accept header: {error}")))?;

    supported
        .iter()
        .copied()
        .find(|content_type| accepts_content_type(accept, content_type))
        .ok_or_else(|| {
            CoolError::NotAcceptable(format!(
                "router only serves {} responses",
                supported.join(", "),
            ))
        })
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
