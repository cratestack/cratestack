//! #415: `max_hops`/right-to-left chain-walk coverage for `parse_client_ip`
//! plus the trust-boundary coverage for `enrich_context_from_headers`.
//! Split out of `tests_correlation.rs` (which keeps the pre-existing
//! `traceparent`/basic `parse_client_ip` coverage) once this file's own
//! growth would have pushed `tests_correlation.rs` past this crate's
//! ~200-line-per-file convention. Header-selection precedence (Finding 1)
//! and IP-shape validation (Finding 2) coverage lives in
//! `tests_header_precedence.rs`/`tests_ip_validation.rs` respectively, for
//! the same reason.

#![cfg(test)]

use std::net::SocketAddr;

use axum::http::{HeaderMap, HeaderValue};

use crate::trusted_proxy::{ForwardedHeader, TrustedProxyConfig};

use super::enrich::{enrich_context_from_headers, is_missing_connect_info_misconfiguration};
use super::forwarded::parse_client_ip;

pub(super) fn headers_with(name: &'static str, value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(name, HeaderValue::from_str(value).unwrap());
    headers
}

pub(super) fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

pub(super) fn ctx() -> cratestack_core::CratestackContext {
    cratestack_core::CratestackContext::anonymous()
}

// --- max_hops / right-to-left chain walk -----------------------------------

#[test]
fn max_hops_zero_trusts_nothing() {
    let h = headers_with("x-forwarded-for", "192.0.2.43, 10.0.0.1");
    assert_eq!(parse_client_ip(&h, 0, ForwardedHeader::XForwardedFor), None);
}

#[test]
fn single_hop_chain_with_max_hops_one_returns_the_only_entry() {
    let h = headers_with("x-forwarded-for", "203.0.113.5");
    assert_eq!(
        parse_client_ip(&h, 1, ForwardedHeader::XForwardedFor),
        Some("203.0.113.5".to_owned())
    );
}

#[test]
fn max_hops_deeper_than_the_actual_chain_returns_none() {
    let h = headers_with("x-forwarded-for", "203.0.113.5, 10.0.0.5");
    assert_eq!(parse_client_ip(&h, 3, ForwardedHeader::XForwardedFor), None);
}

/// The whole point of this PR (#415): the hop-count walk must be
/// right-to-left. A naive left-to-right implementation (take the
/// `max_hops`-th entry counting from the left) would return the
/// attacker-controlled leftmost entry here instead of the entry the
/// trusted proxy actually appended — re-opening the exact spoofing gap
/// this feature exists to close for any chain longer than one hop.
///
/// This test FAILS under a left-to-right implementation: such an
/// implementation would return `"203.0.113.9"` (the spoofed, attacker-
/// supplied entry at index `max_hops - 1 = 0` from the left) instead of
/// `"10.0.0.5"` (the entry the trusted proxy actually appended, at index
/// `len - max_hops = 1` from the left / 0 from the right).
#[test]
fn hop_count_walks_right_to_left_not_left_to_right() {
    // An attacker-controlled client prepends a spoofed entry; the trusted
    // proxy appends its own observed value on the right.
    let h = headers_with("x-forwarded-for", "203.0.113.9, 10.0.0.5");
    let resolved = parse_client_ip(&h, 1, ForwardedHeader::XForwardedFor);
    assert_eq!(resolved, Some("10.0.0.5".to_owned()));
    assert_ne!(resolved, Some("203.0.113.9".to_owned()));
}

#[test]
fn hop_count_two_selects_second_from_right_entry() {
    let h = headers_with("x-forwarded-for", "203.0.113.9, 10.0.0.1, 10.0.0.2");
    // Rightmost (10.0.0.2) is the immediate trusted peer's own hop;
    // walking 2 in from the right lands on what it saw (10.0.0.1).
    assert_eq!(
        parse_client_ip(&h, 2, ForwardedHeader::XForwardedFor),
        Some("10.0.0.1".to_owned())
    );
}

#[test]
fn forwarded_header_hop_count_is_also_right_to_left() {
    let h = headers_with("forwarded", "for=203.0.113.9, for=10.0.0.5");
    assert_eq!(
        parse_client_ip(&h, 1, ForwardedHeader::Forwarded),
        Some("10.0.0.5".to_owned())
    );
}

// --- enrich_context_from_headers: trust boundary ----------------------------

#[test]
fn untrusted_peer_with_spoofed_header_falls_back_to_peer_address() {
    let headers = headers_with("x-forwarded-for", "10.0.0.1");
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()]);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("203.0.113.7:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("203.0.113.7"));
}

#[test]
fn trusted_peer_resolves_header_chain() {
    // The trusted proxy `198.51.100.1` appends the address of whoever
    // connected to *it* (the real client, `203.0.113.9`) — not its own
    // address. The leftmost entry is attacker-controlled noise the client
    // sent before reaching the proxy.
    let headers = headers_with("x-forwarded-for", "6.6.6.6, 203.0.113.9");
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()]);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("203.0.113.9"));
}

#[test]
fn trusted_peer_with_malformed_header_falls_back_to_peer_address_without_panicking() {
    let headers = HeaderMap::new(); // no forwarding headers at all
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()]);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("198.51.100.1"));
}

#[test]
fn no_connect_info_at_all_yields_no_client_ip_regardless_of_headers() {
    let headers = headers_with("x-forwarded-for", "203.0.113.9");
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()]);
    let enriched = enrich_context_from_headers(ctx(), &headers, Some(&config), None);
    assert_eq!(enriched.client_ip(), None);
}

#[test]
fn unconfigured_default_uses_peer_address_never_headers() {
    let headers = headers_with("x-forwarded-for", "10.0.0.1");
    let enriched =
        enrich_context_from_headers(ctx(), &headers, None, Some(addr("203.0.113.7:9000")));
    assert_eq!(enriched.client_ip(), Some("203.0.113.7"));
}

#[test]
fn unconfigured_default_with_no_peer_yields_no_client_ip() {
    let headers = headers_with("x-forwarded-for", "10.0.0.1");
    let enriched = enrich_context_from_headers(ctx(), &headers, None, None);
    assert_eq!(enriched.client_ip(), None);
}

// --- Finding 6: missing-ConnectInfo misconfiguration detection -------------

/// The exact combination the warn-once-per-process log line exists to
/// catch: a `TrustedProxyConfig` is applied, but no peer ever arrived.
#[test]
fn trusted_proxy_without_a_peer_is_flagged_as_a_misconfiguration() {
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()]);
    assert!(is_missing_connect_info_misconfiguration(
        Some(&config),
        None
    ));
}

#[test]
fn trusted_proxy_with_a_peer_is_not_flagged() {
    let config =
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()]);
    assert!(!is_missing_connect_info_misconfiguration(
        Some(&config),
        Some(addr("198.51.100.1:9000"))
    ));
}

#[test]
fn no_trusted_proxy_config_at_all_is_never_flagged_regardless_of_peer() {
    // The unconfigured default (decision 3) is intentional, not a
    // misconfiguration — nothing to warn about.
    assert!(!is_missing_connect_info_misconfiguration(None, None));
    assert!(!is_missing_connect_info_misconfiguration(
        None,
        Some(addr("198.51.100.1:9000"))
    ));
}

#[test]
fn cidr_trusted_proxy_matches_a_range() {
    // Peer `10.4.5.6` falls inside the trusted `10.0.0.0/8` CIDR range
    // (not an exact match); it appends the real client's address on the
    // right.
    let headers = headers_with("x-forwarded-for", "6.6.6.6, 203.0.113.9");
    let config = TrustedProxyConfig::trusting(["10.0.0.0/8".parse().unwrap()]);
    let enriched =
        enrich_context_from_headers(ctx(), &headers, Some(&config), Some(addr("10.4.5.6:9000")));
    assert_eq!(enriched.client_ip(), Some("203.0.113.9"));
}
