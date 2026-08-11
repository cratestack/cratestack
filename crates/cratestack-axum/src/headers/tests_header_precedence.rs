//! #415 Finding 1 (CRITICAL) + Finding 3: which single forwarding header is
//! honored, and duplicate-occurrence merging of whichever one is selected.
//! Split out of `tests_trusted_proxy.rs` to stay under this crate's
//! ~200-line-per-file convention and to keep the decisive regression test
//! for the confirmed bypass in its own clearly-named file.

#![cfg(test)]

use axum::http::HeaderMap;

use crate::trusted_proxy::{ForwardedHeader, TrustedProxyConfig};

use super::enrich::enrich_context_from_headers;
use super::tests_trusted_proxy::{addr, ctx, headers_with};

fn headers_with_both(xff: &str, forwarded: &str) -> HeaderMap {
    let mut headers = headers_with("x-forwarded-for", xff);
    headers.insert(
        "forwarded",
        axum::http::HeaderValue::from_str(forwarded).unwrap(),
    );
    headers
}

/// **The decisive regression test for Finding 1.** Reproduces the exact
/// bypass scenario confirmed against the real generated router: a trusted
/// proxy appends a legitimate `X-Forwarded-For` chain, but the request
/// also carries an entirely attacker-authored `Forwarded` header (real
/// proxies — nginx, an ALB, HAProxy defaults — set XFF and never touch
/// `Forwarded`, so in practice a `Forwarded` header on the wire is
/// attacker-controlled, unvalidated, and never appended to by the trusted
/// proxy). With the default `ForwardedHeader::XForwardedFor`, the
/// attacker's `Forwarded` header must be ignored entirely and the
/// XFF-derived value must win.
///
/// This test FAILS under the pre-fix precedence (`Forwarded` inspected
/// first and returned unconditionally whenever present) — see the PR
/// verification log for the reproduction of that failure.
#[test]
fn xff_wins_by_default_and_the_attackers_forwarded_header_is_ignored() {
    let headers = headers_with_both(
        "6.6.6.6, 203.0.113.9",    // what the real trusted proxy appended
        "for=\"666.666.666.666\"", // attacker-authored, never touched by the proxy
    );
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("203.0.113.9"));
    assert_ne!(enriched.client_ip(), Some("666.666.666.666"));
}

/// The other half: a deployment whose proxy actually emits RFC 7239
/// `Forwarded` opts in via `forwarded_header`, and an attacker-authored
/// `X-Forwarded-For` on the same request is then the one ignored.
#[test]
fn explicit_forwarded_opt_in_uses_forwarded_and_ignores_xff() {
    let headers = headers_with_both(
        "666.666.666.666", // attacker-authored, this deployment's proxy never writes XFF
        "for=6.6.6.6, for=203.0.113.9", // what the real trusted proxy appended
    );
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1)
            .forwarded_header(ForwardedHeader::Forwarded);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("203.0.113.9"));
    assert_ne!(enriched.client_ip(), Some("666.666.666.666"));
}

/// Finding 3: `HeaderMap::get` only returns the first occurrence of a
/// repeated header. RFC 7230 §3.2.2 makes repeated list-type header lines
/// semantically equivalent to one comma-joined value — a proxy that
/// appends its hop as a *second* `X-Forwarded-For` header line (rather
/// than extending the first) must still have that value honored, not
/// silently dropped in favor of whichever line an attacker sent first.
#[test]
fn duplicate_x_forwarded_for_lines_are_merged_proxy_appended_value_wins() {
    let mut headers = HeaderMap::new();
    headers.append(
        "x-forwarded-for",
        axum::http::HeaderValue::from_static("203.0.113.9"), // attacker, sent first
    );
    headers.append(
        "x-forwarded-for",
        axum::http::HeaderValue::from_static("6.6.6.6, 10.0.0.5"), // proxy-appended, second line
    );
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    // The merged chain is `203.0.113.9, 6.6.6.6, 10.0.0.5`; the rightmost
    // (proxy-appended) entry is `10.0.0.5`, never the attacker's
    // first-line value.
    assert_eq!(enriched.client_ip(), Some("10.0.0.5"));
    assert_ne!(enriched.client_ip(), Some("203.0.113.9"));
}
