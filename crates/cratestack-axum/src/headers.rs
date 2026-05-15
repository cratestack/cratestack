//! Header helpers used by axum-bound handlers: optimistic-locking ETag
//! parsing/emission, W3C `traceparent` extraction, RFC 7239 `Forwarded`
//! client-IP extraction, and context enrichment that bundles those.

use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use cratestack_core::CoolError;

/// Parse an `If-Match` header carrying a strong ETag of the form `"<int>"`.
/// Returns `None` if the header is absent. Returns an error if the header
/// is present but malformed (weak validators, non-integer payloads, etc.).
pub fn parse_if_match_version(headers: &HeaderMap) -> Result<Option<i64>, CoolError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CoolError::BadRequest("If-Match header must be ASCII".to_owned()))?
        .trim();
    if raw == "*" {
        return Err(CoolError::BadRequest(
            "If-Match: * is not supported on versioned models".to_owned(),
        ));
    }
    let stripped = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            CoolError::BadRequest(
                "If-Match must be a strong ETag of the form \"<integer>\"".to_owned(),
            )
        })?;
    stripped
        .parse::<i64>()
        .map(Some)
        .map_err(|_| CoolError::BadRequest("If-Match ETag must be an integer".to_owned()))
}

/// Insert an `ETag` header onto a response, formatted as a strong validator
/// over the integer optimistic-locking version.
pub fn set_version_etag(response: &mut Response, version: i64) {
    if let Ok(value) = HeaderValue::from_str(&format!("\"{version}\"")) {
        response.headers_mut().insert(header::ETAG, value);
    }
}

/// Enrich a `CoolContext` with the request id (from `traceparent`) and the
/// client IP (from `Forwarded`/`X-Forwarded-For`). Malformed `traceparent`
/// headers are silently ignored here — the auth/header-validation layer is
/// the right place to reject them, not the enrichment seam.
pub fn enrich_context_from_headers(
    ctx: cratestack_core::CoolContext,
    headers: &HeaderMap,
) -> cratestack_core::CoolContext {
    let mut ctx = ctx;
    if let Ok(Some(trace_id)) = parse_traceparent(headers) {
        ctx = ctx.with_request_id(trace_id);
    }
    if let Some(ip) = parse_client_ip(headers) {
        ctx = ctx.with_client_ip(ip);
    }
    ctx
}

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

/// Extract the most-specific client IP available from the request headers,
/// falling back to none. Prefers `Forwarded` (RFC 7239) over the legacy
/// `X-Forwarded-For`. Banks running behind a single trusted L7 take the
/// leftmost entry; deeper proxy chains must verify and rewrite at the edge.
pub fn parse_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(forwarded) = headers.get("forwarded").and_then(|v| v.to_str().ok()) {
        for segment in forwarded.split(',').map(str::trim) {
            for kv in segment.split(';').map(str::trim) {
                if let Some(rest) = kv.strip_prefix("for=") {
                    let cleaned = rest.trim_matches('"');
                    let cleaned = cleaned
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .unwrap_or(cleaned);
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_owned());
                    }
                }
            }
        }
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod if_match_tests {
    use super::*;

    fn header_map_with_if_match(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn returns_none_when_header_absent() {
        let headers = HeaderMap::new();
        assert_eq!(parse_if_match_version(&headers).unwrap(), None);
    }

    #[test]
    fn parses_strong_quoted_integer() {
        let headers = header_map_with_if_match("\"42\"");
        assert_eq!(parse_if_match_version(&headers).unwrap(), Some(42));
    }

    #[test]
    fn rejects_unquoted_payload() {
        let headers = header_map_with_if_match("42");
        let error = parse_if_match_version(&headers).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_wildcard() {
        let headers = header_map_with_if_match("*");
        let error = parse_if_match_version(&headers).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_non_integer_payload() {
        let headers = header_map_with_if_match("\"v42\"");
        let error = parse_if_match_version(&headers).unwrap_err();
        assert_eq!(error.code(), "BAD_REQUEST");
    }
}

#[cfg(test)]
mod correlation_tests {
    use super::*;

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn traceparent_absent_returns_none() {
        assert!(parse_traceparent(&HeaderMap::new()).unwrap().is_none());
    }

    #[test]
    fn parses_canonical_traceparent_into_trace_id() {
        let h = headers_with(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        );
        let trace_id = parse_traceparent(&h).unwrap().unwrap();
        assert_eq!(trace_id, "0af7651916cd43dd8448eb211c80319c");
    }

    #[test]
    fn rejects_traceparent_with_wrong_segment_count() {
        let h = headers_with("traceparent", "00-deadbeef");
        let err = parse_traceparent(&h).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_traceparent_with_short_trace_id() {
        let h = headers_with("traceparent", "00-deadbeef-b7ad6b7169203331-01");
        let err = parse_traceparent(&h).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rejects_all_zero_trace_id() {
        let h = headers_with(
            "traceparent",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
        );
        let err = parse_traceparent(&h).unwrap_err();
        assert_eq!(err.code(), "BAD_REQUEST");
    }

    #[test]
    fn rfc7239_forwarded_takes_priority_over_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            HeaderValue::from_static("for=192.0.2.43;proto=https"),
        );
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
        assert_eq!(parse_client_ip(&headers), Some("192.0.2.43".to_owned()));
    }

    #[test]
    fn x_forwarded_for_takes_leftmost_address() {
        let h = headers_with("x-forwarded-for", "192.0.2.43, 10.0.0.1");
        assert_eq!(parse_client_ip(&h), Some("192.0.2.43".to_owned()));
    }

    #[test]
    fn client_ip_strips_brackets_around_ipv6() {
        let h = headers_with("forwarded", "for=\"[2001:db8::1]\"");
        assert_eq!(parse_client_ip(&h), Some("2001:db8::1".to_owned()));
    }
}
