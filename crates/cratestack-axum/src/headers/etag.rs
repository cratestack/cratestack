use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use cratestack_core::CratestackError;

/// Parse an `If-Match` header carrying a strong ETag of the form `"<int>"`.
/// Returns `None` if the header is absent. Returns an error if the header
/// is present but malformed (weak validators, non-integer payloads, etc.).
pub fn parse_if_match_version(headers: &HeaderMap) -> Result<Option<i64>, CratestackError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CratestackError::BadRequest("If-Match header must be ASCII".to_owned()))?
        .trim();
    if raw == "*" {
        return Err(CratestackError::BadRequest(
            "If-Match: * is not supported on versioned models".to_owned(),
        ));
    }
    let stripped = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            CratestackError::BadRequest(
                "If-Match must be a strong ETag of the form \"<integer>\"".to_owned(),
            )
        })?;
    stripped
        .parse::<i64>()
        .map(Some)
        .map_err(|_| CratestackError::BadRequest("If-Match ETag must be an integer".to_owned()))
}

/// Insert an `ETag` header onto a response, formatted as a strong validator
/// over the integer optimistic-locking version.
pub fn set_version_etag(response: &mut Response, version: i64) {
    if let Ok(value) = HeaderValue::from_str(&format!("\"{version}\"")) {
        response.headers_mut().insert(header::ETAG, value);
    }
}
