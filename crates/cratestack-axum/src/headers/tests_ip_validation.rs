//! #415 Finding 2: the selected hop must parse as a genuine `IpAddr`
//! before it is ever recorded as the audit `client_ip` — an unparseable or
//! malformed string (`666.666.666.666`) must never reach the audit trail,
//! and realistic shapes (a port suffix, bracketed IPv6, RFC 7239's
//! quoted-string `for="..."` syntax) must still resolve correctly rather
//! than being rejected as "malformed" by accident. Split out of
//! `tests_trusted_proxy.rs` to stay under this crate's ~200-line-per-file
//! convention.

#![cfg(test)]

use std::net::IpAddr;

use crate::trusted_proxy::{ForwardedHeader, TrustedProxyConfig};

use super::enrich::enrich_context_from_headers;
use super::forwarded::parse_hop_ip;
use super::tests_trusted_proxy::{addr, ctx, headers_with};

fn trusted_config() -> TrustedProxyConfig {
    TrustedProxyConfig::trusting(["198.51.100.1".parse::<IpAddr>().unwrap().into()]).max_hops(1)
}

// --- parse_hop_ip: direct unit coverage of the realistic wire shapes -------

#[test]
fn bare_ipv4_parses() {
    assert_eq!(
        parse_hop_ip("203.0.113.9"),
        Some("203.0.113.9".parse().unwrap())
    );
}

#[test]
fn ipv4_with_port_suffix_is_normalized_to_the_bare_address() {
    assert_eq!(
        parse_hop_ip("10.0.0.5:5678"),
        Some("10.0.0.5".parse().unwrap())
    );
}

#[test]
fn bare_ipv6_without_brackets_parses() {
    assert_eq!(
        parse_hop_ip("2001:db8::1"),
        Some("2001:db8::1".parse().unwrap())
    );
}

#[test]
fn bracketed_ipv6_without_a_port_parses() {
    assert_eq!(parse_hop_ip("[::1]"), Some("::1".parse().unwrap()));
}

#[test]
fn bracketed_ipv6_with_a_port_is_normalized_to_the_bare_address() {
    assert_eq!(parse_hop_ip("[::1]:8080"), Some("::1".parse().unwrap()));
}

#[test]
fn invalid_address_does_not_parse() {
    assert_eq!(parse_hop_ip("666.666.666.666"), None);
}

#[test]
fn rfc7239_placeholder_tokens_do_not_parse_as_ips() {
    assert_eq!(parse_hop_ip("unknown"), None);
    assert_eq!(parse_hop_ip("_hidden"), None);
}

// --- enrich_context_from_headers: end-to-end validation of the selected hop

/// The headline case: an invalid, spoofed string must never be recorded —
/// falls back to the verified peer address instead.
#[test]
fn invalid_ip_string_falls_back_to_peer_address_not_recorded_verbatim() {
    let headers = headers_with("x-forwarded-for", "666.666.666.666");
    let config = trusted_config();
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("198.51.100.1"));
    assert_ne!(enriched.client_ip(), Some("666.666.666.666"));
}

#[test]
fn port_suffix_on_a_trusted_xff_entry_is_stripped() {
    let headers = headers_with("x-forwarded-for", "10.0.0.5:5678");
    let config = trusted_config();
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("10.0.0.5"));
}

#[test]
fn bracketed_ipv6_with_port_on_a_trusted_xff_entry_is_normalized() {
    let headers = headers_with("x-forwarded-for", "[2001:db8::1]:8080");
    let config = trusted_config();
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("2001:db8::1"));
}

#[test]
fn forwarded_quoted_bracketed_ipv6_with_port_is_normalized() {
    let headers = headers_with("forwarded", "for=\"[2001:db8::1]:8080\"");
    let config = trusted_config().forwarded_header(ForwardedHeader::Forwarded);
    let enriched = enrich_context_from_headers(
        ctx(),
        &headers,
        Some(&config),
        Some(addr("198.51.100.1:9000")),
    );
    assert_eq!(enriched.client_ip(), Some("2001:db8::1"));
}
