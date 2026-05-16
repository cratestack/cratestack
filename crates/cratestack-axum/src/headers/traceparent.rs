use axum::http::HeaderMap;
use cratestack_core::CoolError;

/// Extract a W3C `traceparent` header, returning the trace-id portion when
/// the header is present and well-formed. Returns `Ok(None)` when absent —
/// callers should mint their own request id in that case so every audit row
/// carries something. The trace-id is the second hyphen-delimited segment
/// per [W3C Trace Context]; this implementation does **not** validate the
/// flags/version segments since banks usually rebuild traceparent at the
/// edge anyway.
///
/// [W3C Trace Context]: https://www.w3.org/TR/trace-context/
pub fn parse_traceparent(headers: &HeaderMap) -> Result<Option<String>, CoolError> {
    let Some(value) = headers.get("traceparent") else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CoolError::BadRequest("traceparent must be ASCII".to_owned()))?
        .trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.len() != 4 {
        return Err(CoolError::BadRequest(
            "traceparent must have 4 hyphen-delimited segments".to_owned(),
        ));
    }
    let trace_id = parts[1];
    if trace_id.len() != 32 || !trace_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CoolError::BadRequest(
            "traceparent trace-id must be 32 lowercase hex characters".to_owned(),
        ));
    }
    if trace_id == "00000000000000000000000000000000" {
        return Err(CoolError::BadRequest(
            "traceparent trace-id must not be all zeros".to_owned(),
        ));
    }
    Ok(Some(trace_id.to_owned()))
}
