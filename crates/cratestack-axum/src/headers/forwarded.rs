use axum::http::HeaderMap;

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
