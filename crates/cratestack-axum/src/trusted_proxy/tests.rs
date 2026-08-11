#![cfg(test)]

use std::net::IpAddr;

use super::TrustedProxyConfig;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn none_trusts_nothing() {
    let config = TrustedProxyConfig::none();
    assert!(!config.is_trusted(ip("10.0.0.1")));
    assert!(!config.is_trusted(ip("127.0.0.1")));
    assert_eq!(config.hop_count(), 0);
}

#[test]
fn default_is_equivalent_to_none() {
    assert_eq!(TrustedProxyConfig::default().hop_count(), 0);
    assert!(!TrustedProxyConfig::default().is_trusted(ip("10.0.0.1")));
}

#[test]
fn trusting_exact_host_address_matches_only_that_address() {
    let config = TrustedProxyConfig::trusting([ip("10.0.0.5").into()]);
    assert!(config.is_trusted(ip("10.0.0.5")));
    assert!(!config.is_trusted(ip("10.0.0.6")));
    assert!(!config.is_trusted(ip("10.0.5.5")));
}

#[test]
fn trusting_defaults_max_hops_to_one() {
    let config = TrustedProxyConfig::trusting([ip("10.0.0.5").into()]);
    assert_eq!(config.hop_count(), 1);
}

#[test]
fn max_hops_overrides_the_default() {
    let config = TrustedProxyConfig::trusting([ip("10.0.0.5").into()]).max_hops(3);
    assert_eq!(config.hop_count(), 3);
}

#[test]
fn cidr_range_matches_every_address_inside_it() {
    let config = TrustedProxyConfig::trusting(["10.0.0.0/8".parse().unwrap()]);
    assert!(config.is_trusted(ip("10.0.0.1")));
    assert!(config.is_trusted(ip("10.255.255.254")));
    assert!(!config.is_trusted(ip("11.0.0.1")));
    assert!(!config.is_trusted(ip("192.168.1.1")));
}

#[test]
fn cidr_range_supports_ipv6() {
    let config = TrustedProxyConfig::trusting(["2001:db8::/32".parse().unwrap()]);
    assert!(config.is_trusted(ip("2001:db8::1")));
    assert!(!config.is_trusted(ip("2001:db9::1")));
}

#[test]
fn multiple_allowlist_entries_are_all_checked() {
    let config =
        TrustedProxyConfig::trusting(["10.0.0.0/8".parse().unwrap(), ip("203.0.113.9").into()]);
    assert!(config.is_trusted(ip("10.1.2.3")));
    assert!(config.is_trusted(ip("203.0.113.9")));
    assert!(!config.is_trusted(ip("203.0.113.10")));
}
