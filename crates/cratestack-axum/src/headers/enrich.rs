use axum::http::HeaderMap;

use super::forwarded::parse_client_ip;
use super::traceparent::parse_traceparent;

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
