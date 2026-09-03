//! cratestack#871 review, blocker 1: IPv6 `/64` aggregation on the paths
//! that are NOT the budgeted `auth:` scope — the `Ignore` policy's peer
//! key. The scope/fallback aggregation lives next door in
//! `tests_scope.rs`; this file exists because the two together exceed the
//! workspace's 200-line ceiling.

#![cfg(test)]

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use http::Request as HttpRequest;

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::key_fn::default_key_fn;
use super::scope::{KeyDerivation, UnverifiedAuthPolicy, bucket_address};
use cratestack_core::CratestackError;

fn with_connect_info(mut req: Request, addr: &str) -> Request {
    let socket_addr: SocketAddr = addr.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(socket_addr));
    req
}

fn derive(req: &Request) -> Result<KeyDerivation, CratestackError> {
    default_key_fn(
        req,
        RateLimitBucketBudget::default(),
        UnverifiedAuthPolicy::default(),
        &BudgetWarnings::default(),
    )
}

fn derive_with(req: &Request, policy: UnverifiedAuthPolicy) -> KeyDerivation {
    default_key_fn(
        req,
        RateLimitBucketBudget::default(),
        policy,
        &BudgetWarnings::default(),
    )
    .expect("derivation must succeed")
}

fn bearer(token: &str) -> Request {
    Request::from(
        HttpRequest::builder()
            .header("authorization", token)
            .body(axum::body::Body::empty())
            .unwrap(),
    )
}

/// `Ignore` drops the header and keys on the peer — which for IPv6 must
/// also be the /64, or the policy advertised as "strictly stronger" is
/// weaker than the default it replaces.
#[test]
fn ipv6_ignore_policy_keys_on_the_aggregated_peer() {
    let one = derive_with(
        &with_connect_info(bearer("Bearer a"), "[2001:db8:5:5::1]:1"),
        UnverifiedAuthPolicy::Ignore,
    );
    let two = derive_with(
        &with_connect_info(bearer("Bearer b"), "[2001:db8:5:5:ffff::2]:1"),
        UnverifiedAuthPolicy::Ignore,
    );

    assert_eq!(one.key, "ip:2001:db8:5:5::/64");
    assert_eq!(one.key, two.key);
}

/// IPv4 is deliberately not aggregated: a /24 under CGNAT is thousands of
/// unrelated subscribers, and IPv4 gives an attacker no free-address supply
/// comparable to a /64.
#[test]
fn ipv4_scopes_are_not_aggregated() {
    let one = derive(&with_connect_info(bearer("Bearer a"), "192.0.2.1:1")).expect("derives");
    let two = derive(&with_connect_info(bearer("Bearer b"), "192.0.2.2:1")).expect("derives");

    let scope = |d: &KeyDerivation| d.budget.as_ref().expect("budgeted").scope_key.clone();
    assert_eq!(scope(&one), "peer:192.0.2.1");
    assert_ne!(scope(&one), scope(&two));
}

// --- cratestack#871 round-2 blocker: IPv4-mapped IPv6 ---

/// A dual-stack listener delivers every IPv4 client as `::ffff:a.b.c.d`.
/// Those have all-zero top groups, so taking the /64 blindly mapped EVERY
/// IPv4 client onto `ip:::/64` — measured: 200 distinct IPv4 clients, 1
/// bucket, 5 allowed. A cratestack#416 collision of unlimited width and a
/// one-client DoS of all IPv4 traffic.
#[test]
fn ipv4_mapped_addresses_are_unwrapped_not_aggregated() {
    use std::net::IpAddr;

    let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
    assert_eq!(bucket_address(mapped), "192.0.2.1");

    // ...and two of them must not collide.
    let other: IpAddr = "::ffff:192.0.2.2".parse().unwrap();
    assert_ne!(bucket_address(mapped), bucket_address(other));

    // Identical to how the same client arriving over a v4 socket is keyed,
    // so a dual-stack and a v4-only listener agree on the bucket.
    let plain: IpAddr = "192.0.2.1".parse().unwrap();
    assert_eq!(bucket_address(mapped), bucket_address(plain));
}

/// The rest of the all-zero `::/64` region must not be aggregated either:
/// merging the unspecified address, the IPv6 loopback and the deprecated
/// IPv4-compatible form into one bucket is the same collision in miniature.
/// (This is why `to_ipv4_mapped` is used rather than `to_ipv4`, which would
/// turn `::1` into `0.0.0.1`.)
#[test]
fn the_all_zero_prefix_is_never_aggregated() {
    use std::net::IpAddr;

    let loopback: IpAddr = "::1".parse().unwrap();
    let unspecified: IpAddr = "::".parse().unwrap();
    let compat: IpAddr = "::192.0.2.3".parse().unwrap();

    assert_eq!(bucket_address(loopback), "::1");
    assert_ne!(bucket_address(loopback), bucket_address(unspecified));
    assert_ne!(bucket_address(loopback), bucket_address(compat));
    // And none of them may masquerade as a real IPv4 address.
    assert_ne!(bucket_address(compat), "192.0.2.3");
}

/// Routable IPv6 still aggregates — the blocker fix must not undo the
/// evasion fix it sits next to.
#[test]
fn routable_ipv6_still_aggregates_to_a_64() {
    use std::net::IpAddr;

    let one: IpAddr = "2001:db8:1:2::1".parse().unwrap();
    let two: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
    assert_eq!(bucket_address(one), "2001:db8:1:2::/64");
    assert_eq!(bucket_address(one), bucket_address(two));
}

/// The reviewer's probe, against a REAL dual-stack socket rather than a
/// hand-written address: whatever the kernel actually hands `accept()` on
/// `[::]:0` for an IPv4 client must key per-address.
#[tokio::test]
async fn a_real_dual_stack_socket_keys_ipv4_clients_per_address() {
    use tokio::net::{TcpListener, TcpStream};

    // `[::]:0` is the ordinary Linux dual-stack bind. If the platform
    // refuses it (v6-only sysctl, no IPv6), there is nothing to assert.
    let Ok(listener) = TcpListener::bind("[::]:0").await else {
        return;
    };
    let port = listener.local_addr().expect("local_addr").port();
    let Ok(_client) = TcpStream::connect(("127.0.0.1", port)).await else {
        return;
    };
    let (_socket, peer) = listener.accept().await.expect("accept");

    let key = bucket_address(peer.ip());
    assert_ne!(
        key, "::/64",
        "the kernel handed us {peer:?}; keying it as ::/64 collapses every IPv4 client in the \
         world onto one bucket",
    );
    assert!(
        key.starts_with("127.0.0.1"),
        "expected the v4 loopback address, got {key} from peer {peer:?}",
    );
}
