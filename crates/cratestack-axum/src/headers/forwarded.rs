use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;

use crate::trusted_proxy::ForwardedHeader;

/// Extract the client IP `max_hops` entries in from the *right* end of
/// whichever single header `header` selects — never both. Falls back to
/// `None` if that header is absent, empty, or the walk runs off the end of
/// the chain.
///
/// **Only one header is ever consulted (#415 remediation).** RFC 7239
/// `Forwarded` and the legacy `X-Forwarded-For` are alternatives, not
/// complements — a real proxy emits one or the other, never both
/// meaningfully. Consulting `Forwarded` whenever it happens to be present,
/// ahead of `X-Forwarded-For`, let an attacker who knows a deployment
/// trusts `X-Forwarded-For` bypass every hop-count/allowlist check just by
/// sending an entirely unvalidated `Forwarded` header instead. `header`
/// (from [`crate::trusted_proxy::TrustedProxyConfig::forwarded_header`])
/// names the one header this deployment's proxy actually writes; the other
/// is never even inspected.
///
/// Callers must only invoke this with a `max_hops`/`header` they have
/// independently established is trustworthy (i.e. after confirming the
/// request's socket peer is a configured trusted proxy) — this function
/// has no notion of trust itself, it only walks the chain. See
/// [`crate::trusted_proxy::TrustedProxyConfig`] and
/// [`crate::headers::enrich_context_from_headers`] for the trust check,
/// and [`crate::headers::enrich_context_from_headers`] for the IP-shape
/// validation applied to whatever this function selects.
///
/// **Right-to-left, not left-to-right.** The left end of the chain is
/// exactly the part an untrusted client controls (it can prepend arbitrary
/// entries), so walking in from the left re-opens the identical spoofing
/// gap for any chain longer than one hop. `max_hops` counts inward from
/// the right: `max_hops = 1` takes the rightmost entry (the immediate
/// trusted proxy's own contribution); `max_hops = 2` takes the
/// second-from-right entry (what the *next* hop in reported seeing),
/// and so on. `max_hops = 0` trusts nothing and always returns `None`.
/// See decision 5 in `docs/design/trusted-proxy-client-ip.md`.
///
/// **Duplicate header occurrences are merged, not dropped (#415
/// remediation).** RFC 7230 §3.2.2: repeated list-type header fields are
/// semantically equivalent to a single comma-joined value. A proxy that
/// appends its hop as a *second* `X-Forwarded-For` line (rather than
/// extending the first) must not have that value silently lost to
/// whichever line an attacker sent first — every occurrence is
/// concatenated, in wire order, before the chain is walked.
pub fn parse_client_ip(
    headers: &HeaderMap,
    max_hops: usize,
    header: ForwardedHeader,
) -> Option<String> {
    let entries = match header {
        ForwardedHeader::XForwardedFor => list_header_entries(headers, "x-forwarded-for"),
        ForwardedHeader::Forwarded => forwarded_for_entries(headers),
    };
    select_hop(&entries, max_hops)
}

/// Every comma-separated value across all occurrences of a list-type
/// header, concatenated in wire order (RFC 7230 §3.2.2 — see this module's
/// doc for why that matters here).
fn list_header_entries(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The ordered `for=` values across every occurrence of the RFC 7239
/// `Forwarded` header's comma-separated segments, left-to-right as they
/// appear on the wire (occurrences merged per RFC 7230 §3.2.2, same as
/// [`list_header_entries`]).
fn forwarded_for_entries(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("forwarded")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|raw| raw.split(','))
        .filter_map(|segment| {
            segment.split(';').map(str::trim).find_map(|kv| {
                let rest = kv.strip_prefix("for=")?;
                // Strip the RFC 7239 quoted-string form (`for="..."`); the
                // bracket/port shape inside is normalized later by
                // `parse_hop_ip`, once a hop is actually selected.
                let cleaned = rest.trim_matches('"');
                (!cleaned.is_empty()).then(|| cleaned.to_owned())
            })
        })
        .collect()
}

/// Select the entry `max_hops` positions in from the right end of an
/// ordered (left-to-right, as on the wire) chain. `max_hops = 0` yields
/// `None` (trust nothing); a `max_hops` deeper than the chain's actual
/// length also yields `None` rather than guessing which shorter-than-
/// expected entry might still be trustworthy — a chain shorter than the
/// configured hop count is treated as unexpected, not as license to fall
/// back to the leftmost (client-controlled) entry.
fn select_hop(entries: &[String], max_hops: usize) -> Option<String> {
    if max_hops == 0 {
        return None;
    }
    let index = entries.len().checked_sub(max_hops)?;
    entries.get(index).cloned()
}

/// Parse a single selected hop entry into a validated [`IpAddr`] — the
/// realistic forms a proxy or client actually sends: a bare address, an
/// IPv4 address with a port suffix (`1.2.3.4:5678`), and a bracketed IPv6
/// address with or without a port (`[::1]`, `[::1]:8080`). Returns `None`
/// for anything else, including malformed/spoofed strings like
/// `666.666.666.666` or RFC 7239 placeholders (`unknown`, `_hidden`) —
/// callers must never record an unparseable string as the audit
/// `client_ip` (#415 remediation, Finding 2).
pub(super) fn parse_hop_ip(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('[') {
        // Bracketed IPv6, with or without a trailing `:port` — take
        // everything up to the closing bracket.
        let inner = rest.split(']').next()?;
        return inner.parse().ok();
    }
    if let Ok(ip) = raw.parse::<IpAddr>() {
        return Some(ip);
    }
    // IPv4 with a port suffix (`1.2.3.4:5678`); bare IPv6 without brackets
    // never reaches here successfully since it isn't a valid `SocketAddr`
    // without brackets, which is correct — unbracketed IPv6 is ambiguous
    // with a trailing port and is already handled by the bare-`IpAddr`
    // branch above.
    raw.parse::<SocketAddr>().ok().map(|addr| addr.ip())
}
