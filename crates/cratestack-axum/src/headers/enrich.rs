use std::net::SocketAddr;
use std::sync::Once;

use axum::http::HeaderMap;

use crate::trusted_proxy::TrustedProxyConfig;

use super::forwarded::{parse_client_ip, parse_hop_ip};
use super::traceparent::parse_traceparent;

/// Enrich a `CratestackContext` with the request id (from `traceparent`) and the
/// client IP recorded on audit events. Malformed `traceparent` headers are
/// silently ignored here — the auth/header-validation layer is the right
/// place to reject them, not the enrichment seam.
///
/// `client_ip` resolution (#415 — see `docs/design/trusted-proxy-client-ip.md`
/// for the decided design):
///
/// - `trusted_proxy` is `Some` and the request's socket `peer` is in its
///   allowlist: honor whichever single header
///   [`TrustedProxyConfig::forwarded_header`] selects, walking `max_hops`
///   entries in from the right end of the chain (right-to-left — see
///   [`TrustedProxyConfig::max_hops`]). The selected hop is then parsed as
///   an [`std::net::IpAddr`] (Finding 2 remediation) — a value that isn't a
///   real IP address (a spoofed string, a placeholder like `unknown`, a
///   malformed entry) is never recorded; if the header is absent, doesn't
///   parse at that hop depth, or doesn't parse as an IP, this falls back
///   to the socket peer address rather than recording nothing.
/// - Otherwise (no `trusted_proxy` configured, or the peer isn't in its
///   allowlist): headers are never consulted. `client_ip` is the socket
///   peer address if one is available, or omitted entirely if it isn't.
///
/// The unconfigured default — no `Extension<TrustedProxyConfig>` applied
/// and/or no `ConnectInfo<SocketAddr>` available — is the safe one: headers
/// are never trusted, and nothing is guessed. `client_ip` is simply absent
/// from the audit record.
pub fn enrich_context_from_headers(
    ctx: cratestack_core::CratestackContext,
    headers: &HeaderMap,
    trusted_proxy: Option<&TrustedProxyConfig>,
    peer: Option<SocketAddr>,
) -> cratestack_core::CratestackContext {
    let mut ctx = ctx;
    if let Ok(Some(trace_id)) = parse_traceparent(headers) {
        ctx = ctx.with_request_id(trace_id);
    }
    if let Some(ip) = resolve_client_ip(headers, trusted_proxy, peer) {
        ctx = ctx.with_client_ip(ip);
    }
    ctx
}

/// Logged once per process (not per request — see [`resolve_client_ip`]'s
/// call site) when a `TrustedProxyConfig` is applied but no `ConnectInfo`
/// peer ever arrived. That combination is always a misconfiguration: the
/// consumer applied the `Extension` but never wired
/// `into_make_service_with_connect_info::<SocketAddr>()` (or applied it to
/// a *different* router than the one actually serving traffic — the gRPC
/// `into_router()` and REST/RPC `router()` are separate router instances,
/// see decision 6 in `docs/design/trusted-proxy-client-ip.md`), so
/// `Forwarded`/`X-Forwarded-For` can never be honored no matter how the
/// allowlist is configured — `client_ip` silently degrades to `None` on
/// every single request. `Once`, not per-request: this is a boot-time
/// wiring defect, not a per-request condition worth re-reporting under
/// load (a busy misconfigured deployment could otherwise emit this warning
/// thousands of times a second, itself becoming an operational problem).
static MISSING_CONNECT_INFO_WARNING: Once = Once::new();

/// Whether this request's `(trusted_proxy, peer)` combination is the
/// always-a-misconfiguration case the warning above exists to catch.
/// Split out as a pure, `Once`-independent predicate so it can be unit
/// tested directly — the `Once` firing itself is inherently order-
/// dependent process-wide state (see [`resolve_client_ip`]'s doc), not
/// something a test can assert on in isolation without coupling to
/// whichever other test in the same binary happens to run first.
pub(super) fn is_missing_connect_info_misconfiguration(
    trusted_proxy: Option<&TrustedProxyConfig>,
    peer: Option<SocketAddr>,
) -> bool {
    trusted_proxy.is_some() && peer.is_none()
}

fn resolve_client_ip(
    headers: &HeaderMap,
    trusted_proxy: Option<&TrustedProxyConfig>,
    peer: Option<SocketAddr>,
) -> Option<String> {
    if is_missing_connect_info_misconfiguration(trusted_proxy, peer) {
        MISSING_CONNECT_INFO_WARNING.call_once(|| {
            tracing::warn!(
                target: "cratestack",
                "a TrustedProxyConfig is applied to this router but no ConnectInfo<SocketAddr> \
                 peer was available on this request — Forwarded/X-Forwarded-For can never be \
                 honored until the router is served via \
                 into_make_service_with_connect_info::<SocketAddr>() (every router the app \
                 serves, including a separate gRPC into_router() for `transport grpc` \
                 schemas). client_ip is silently None on every request until this is fixed. \
                 Logged once per process."
            );
        });
    }

    let peer_ip = peer.map(|addr| addr.ip().to_string());

    if let (Some(config), Some(addr)) = (trusted_proxy, peer)
        && config.is_trusted(addr.ip())
    {
        // Trusted peer: honor the selected header if it parses at the
        // configured hop depth AND resolves to a genuine IP address
        // (Finding 2 — never record an unparseable/spoofed string),
        // otherwise fall back to the peer address rather than recording
        // nothing.
        let header_ip = parse_client_ip(headers, config.hop_count(), config.header())
            .as_deref()
            .and_then(parse_hop_ip)
            .map(|ip| ip.to_string());
        return header_ip.or(peer_ip);
    }

    // No trusted-proxy config, or an untrusted peer: never consult
    // headers. Peer address if available, `None` otherwise.
    peer_ip
}
