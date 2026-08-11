//! #415: real end-to-end proof that the trusted-proxy `client_ip`
//! resolution reaches a real generated `router()`, through the actual
//! `Option<Extension<TrustedProxyConfig>>`-equivalent (`ClientIpContext`)
//! extractor at the dispatch call sites — not just the unit-level
//! `enrich_context_from_headers` coverage in `cratestack-axum`. Mirrors
//! `no_database_procedures.rs`'s `include_server_schema!(..., db = None)`
//! pattern: no Postgres dependency, `cratestack-sqlx` is not even in this
//! crate's dependency graph.
//!
//! `cratestack-axum` cannot itself invoke `include_server_schema!` (no
//! dependency on `cratestack-macros`), so this macro-integration coverage
//! has to live here rather than in `cratestack-axum`'s own test suite —
//! see `docs/design/trusted-proxy-client-ip.md`.

use std::net::SocketAddr;

use cratestack::CoolCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CoolContext, CoolError, TrustedProxyConfig, include_server_schema};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/trusted_proxy_client_ip.cstack", db = None);

/// Deliberately does NOT set `client_ip` itself — enrichment happens
/// after `authenticate` returns, inside the generated dispatch fn.
#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CoolError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

/// Surfaces the resolved `client_ip` back to the caller so the test can
/// assert on it directly.
#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn who_am_i(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CoolContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::who_am_i::Output, CoolError>,
    > + Send {
        let client_ip = ctx.client_ip().unwrap_or("none").to_owned();
        async move { Ok(client_ip) }
    }
}

fn router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(db, Procedures, JsonCodec, AllowAllAuth)
}

async fn who_am_i(app: cratestack::axum::Router, request: Request<Body>) -> String {
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    reply.as_str().unwrap().to_owned()
}

fn who_am_i_request() -> cratestack::axum::http::request::Builder {
    Request::post("/$procs/whoAmI")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
}

fn empty_args_body() -> Body {
    Body::from(serde_json::to_vec(&serde_json::json!({ "args": {} })).unwrap())
}

/// Safe default (decision 3): no `Extension<TrustedProxyConfig>` applied
/// at all, no `ConnectInfo<SocketAddr>` either — `client_ip` must be
/// absent, never guessed and never taken from the header.
#[tokio::test]
async fn unconfigured_default_yields_no_client_ip() {
    let request = who_am_i_request()
        .header("x-forwarded-for", "203.0.113.9")
        .body(empty_args_body())
        .unwrap();

    let result = who_am_i(router(), request).await;
    assert_eq!(result, "none");
}

/// Safe default, other half: no trusted-proxy config, but a verified
/// socket peer IS available (`ConnectInfo`) — the peer address is used,
/// never the (still entirely unverified) header.
#[tokio::test]
async fn unconfigured_default_with_peer_uses_peer_address_not_header() {
    let mut request = who_am_i_request()
        .header("x-forwarded-for", "203.0.113.9")
        .body(empty_args_body())
        .unwrap();
    let peer: SocketAddr = "10.9.9.9:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));

    let result = who_am_i(router(), request).await;
    assert_eq!(result, "10.9.9.9");
}

/// Acceptance criterion: forwarded headers ignored from an untrusted
/// peer, falling back to the socket peer address. The peer here is a
/// real `ConnectInfo` value that simply isn't in the allowlist.
#[tokio::test]
async fn spoofed_xff_from_untrusted_peer_is_ignored() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let mut request = who_am_i_request()
        .header("x-forwarded-for", "6.6.6.6")
        .body(empty_args_body())
        .unwrap();
    let untrusted_peer: SocketAddr = "10.0.0.9:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(untrusted_peer));

    let result = who_am_i(app, request).await;
    // The real (untrusted) peer address, never the spoofed header value.
    assert_eq!(result, "10.0.0.9");
    assert_ne!(result, "6.6.6.6");
}

/// Acceptance criterion: a trusted proxy's chain resolves correctly.
/// The trusted proxy (`198.51.100.1`, matching `ConnectInfo`) appends
/// the address of whoever connected to *it* — the real client — on the
/// right; the left entry is attacker-controlled noise from before the
/// proxy.
#[tokio::test]
async fn trusted_proxy_chain_resolves_correctly() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let mut request = who_am_i_request()
        .header("x-forwarded-for", "6.6.6.6, 203.0.113.9")
        .body(empty_args_body())
        .unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "203.0.113.9");
}

/// The whole point of this PR (#415): the hop-count walk is right-to-left.
/// Two trusted hops (CDN + load balancer) each append the peer address
/// they observed; `max_hops(2)` must land on the CDN's observation (what
/// it saw connecting to it — the real client), not the load balancer's
/// own address, and never the attacker-controlled leftmost entry. This
/// test FAILS under a left-to-right implementation for the same reason
/// `cratestack-axum`'s unit test does: a left-to-right walk would return
/// the leftmost (attacker-supplied) entry instead.
#[tokio::test]
async fn hop_count_walks_right_to_left_through_a_real_router() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(2),
    ));

    // Chain: [attacker-spoofed, real client (seen by the CDN), CDN's own
    // address (seen by the load balancer)]. `max_hops(2)` walks 2 in from
    // the right -> index 1 -> the real client.
    let mut request = who_am_i_request()
        .header("x-forwarded-for", "6.6.6.6, 203.0.113.9, 192.0.2.55")
        .body(empty_args_body())
        .unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "203.0.113.9");
    assert_ne!(
        result, "6.6.6.6",
        "must not pick the attacker-controlled leftmost entry"
    );
    assert_ne!(
        result, "192.0.2.55",
        "must not pick the wrong (adjacent) hop either"
    );
}

/// Acceptance criterion / decision 2: CIDR ranges, not just exact
/// addresses, are honored in the allowlist.
#[tokio::test]
async fn cidr_allowlist_matches_a_range() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["10.0.0.0/8".parse().unwrap()]).max_hops(1),
    ));

    let mut request = who_am_i_request()
        .header("x-forwarded-for", "6.6.6.6, 203.0.113.9")
        .body(empty_args_body())
        .unwrap();
    // Not an exact match against any configured entry — inside the /8.
    let trusted_peer: SocketAddr = "10.4.5.6:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "203.0.113.9");
}

/// A trusted peer whose header is malformed/missing falls back to the
/// peer address rather than panicking or silently omitting `client_ip`.
#[tokio::test]
async fn trusted_peer_without_a_header_falls_back_to_peer_address() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let mut request = who_am_i_request().body(empty_args_body()).unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "198.51.100.1");
}
