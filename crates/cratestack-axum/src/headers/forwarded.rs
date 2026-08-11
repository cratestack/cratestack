use axum::http::HeaderMap;

/// Extract the client IP `max_hops` entries in from the *right* end of the
/// `Forwarded`/`X-Forwarded-For` chain — the end nearest the trusted proxy
/// — falling back to none. Prefers `Forwarded` (RFC 7239) over the legacy
/// `X-Forwarded-For`.
///
/// Callers must only invoke this with a `max_hops` they have independently
/// established is trustworthy (i.e. after confirming the request's socket
/// peer is a configured trusted proxy) — this function has no notion of
/// trust itself, it only walks the chain. See
/// [`crate::trusted_proxy::TrustedProxyConfig`] and
/// [`crate::headers::enrich_context_from_headers`] for the trust check.
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
pub fn parse_client_ip(headers: &HeaderMap, max_hops: usize) -> Option<String> {
    if let Some(forwarded) = headers.get("forwarded").and_then(|v| v.to_str().ok()) {
        let entries = forwarded_for_entries(forwarded);
        if !entries.is_empty() {
            return select_hop(&entries, max_hops);
        }
    }
    let entries: Vec<String> = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    select_hop(&entries, max_hops)
}

/// The ordered `for=` values across an RFC 7239 `Forwarded` header's
/// comma-separated segments, left-to-right as they appear on the wire.
fn forwarded_for_entries(forwarded: &str) -> Vec<String> {
    forwarded
        .split(',')
        .filter_map(|segment| {
            segment.split(';').map(str::trim).find_map(|kv| {
                let rest = kv.strip_prefix("for=")?;
                let cleaned = rest.trim_matches('"');
                let cleaned = cleaned
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .unwrap_or(cleaned);
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
