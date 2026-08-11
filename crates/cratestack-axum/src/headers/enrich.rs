use std::net::SocketAddr;

use axum::http::HeaderMap;

use crate::trusted_proxy::TrustedProxyConfig;

use super::forwarded::parse_client_ip;
use super::traceparent::parse_traceparent;

/// Enrich a `CoolContext` with the request id (from `traceparent`) and the
/// client IP recorded on audit events. Malformed `traceparent` headers are
/// silently ignored here — the auth/header-validation layer is the right
/// place to reject them, not the enrichment seam.
///
/// `client_ip` resolution (#415 — see `docs/design/trusted-proxy-client-ip.md`
/// for the decided design):
///
/// - `trusted_proxy` is `Some` and the request's socket `peer` is in its
///   allowlist: honor `Forwarded`/`X-Forwarded-For`, walking `max_hops`
///   entries in from the right end of the chain (right-to-left — see
///   [`TrustedProxyConfig::max_hops`]). If the header is absent or doesn't
///   parse at that hop depth, fall back to the socket peer address rather
///   than recording nothing.
/// - Otherwise (no `trusted_proxy` configured, or the peer isn't in its
///   allowlist): headers are never consulted. `client_ip` is the socket
///   peer address if one is available, or omitted entirely if it isn't.
///
/// The unconfigured default — no `Extension<TrustedProxyConfig>` applied
/// and/or no `ConnectInfo<SocketAddr>` available — is the safe one: headers
/// are never trusted, and nothing is guessed. `client_ip` is simply absent
/// from the audit record.
pub fn enrich_context_from_headers(
    ctx: cratestack_core::CoolContext,
    headers: &HeaderMap,
    trusted_proxy: Option<&TrustedProxyConfig>,
    peer: Option<SocketAddr>,
) -> cratestack_core::CoolContext {
    let mut ctx = ctx;
    if let Ok(Some(trace_id)) = parse_traceparent(headers) {
        ctx = ctx.with_request_id(trace_id);
    }
    if let Some(ip) = resolve_client_ip(headers, trusted_proxy, peer) {
        ctx = ctx.with_client_ip(ip);
    }
    ctx
}

fn resolve_client_ip(
    headers: &HeaderMap,
    trusted_proxy: Option<&TrustedProxyConfig>,
    peer: Option<SocketAddr>,
) -> Option<String> {
    let peer_ip = peer.map(|addr| addr.ip().to_string());

    if let (Some(config), Some(addr)) = (trusted_proxy, peer)
        && config.is_trusted(addr.ip())
    {
        // Trusted peer: honor the header if it parses at the configured
        // hop depth, otherwise fall back to the peer address rather than
        // recording nothing.
        return parse_client_ip(headers, config.hop_count()).or(peer_ip);
    }

    // No trusted-proxy config, or an untrusted peer: never consult
    // headers. Peer address if available, `None` otherwise.
    peer_ip
}
