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

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackContext, CratestackError, TrustedProxyConfig, include_server_schema};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/trusted_proxy_client_ip.cstack", db = None);

/// Deliberately does NOT set `client_ip` itself — enrichment happens
/// after `authenticate` returns, inside the generated dispatch fn.
#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
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
        ctx: &CratestackContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
        _authorized: cratestack_schema::procedures::who_am_i::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::who_am_i::Output, CratestackError>,
    > + Send {
        let client_ip = ctx.client_ip().unwrap_or("none").to_owned();
        async move { Ok(client_ip) }
    }
}

fn router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
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

/// **Finding 1, reproduced through the real generated router.** A real
/// trusted proxy sets `X-Forwarded-For` (never `Forwarded` — nginx, an
/// ALB, and HAProxy's defaults all write XFF only). An attacker adds an
/// entirely unvalidated `Forwarded` header on top. With the default header
/// selection, the attacker's `Forwarded` header must be ignored outright
/// and the proxy-appended XFF value must win.
#[tokio::test]
async fn forwarded_header_from_an_attacker_does_not_override_the_trusted_xff_chain() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let mut request = who_am_i_request()
        .header("x-forwarded-for", "6.6.6.6, 203.0.113.9") // real proxy-appended chain
        .header("forwarded", "for=\"666.666.666.666\"") // attacker-authored, proxy never touches this
        .body(empty_args_body())
        .unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "203.0.113.9");
    assert_ne!(result, "666.666.666.666");
}

/// Finding 3, reproduced through the real generated router: a proxy that
/// appends its hop as a *second* `X-Forwarded-For` header line, rather
/// than extending the first, must still have that value honored.
#[tokio::test]
async fn duplicate_x_forwarded_for_lines_are_merged_through_a_real_router() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let mut request = who_am_i_request()
        .header("x-forwarded-for", "203.0.113.9") // attacker, sent first
        .header("x-forwarded-for", "6.6.6.6, 10.0.0.5") // proxy-appended, second line
        .body(empty_args_body())
        .unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "10.0.0.5");
    assert_ne!(result, "203.0.113.9");
}

/// Finding 2, reproduced through the real generated router: a value that
/// isn't a genuine IP address (spoofed, or simply malformed) must never
/// reach the audit trail — falls back to the verified peer address.
#[tokio::test]
async fn invalid_ip_in_a_trusted_xff_chain_falls_back_to_peer_address() {
    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let mut request = who_am_i_request()
        .header("x-forwarded-for", "666.666.666.666")
        .body(empty_args_body())
        .unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let result = who_am_i(app, request).await;
    assert_eq!(result, "198.51.100.1");
    assert_ne!(result, "666.666.666.666");
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

/// **Finding 4.** Every other test in this file inserts `ConnectInfo`
/// manually into `request.extensions_mut()`, which exercises the same
/// extractor/dispatch code `ClientIpContext` reads from but does NOT prove
/// that `into_make_service_with_connect_info` — the wiring the README/
/// CHANGELOG migration note actually tells consumers to apply — genuinely
/// produces that `ConnectInfo` extension in the first place. This test
/// binds a real `TcpListener`, serves through
/// `into_make_service_with_connect_info::<SocketAddr>()`, and sends a real
/// HTTP/1.1 request over an actual TCP socket via `reqwest` (a real HTTP
/// client, not a hand-rolled raw-socket request), so the peer address
/// `ConnectInfo` carries is whatever the OS's TCP handshake produced — not
/// a value this test chose. REST/RPC is the transport exercised here (this
/// crate has no gRPC dependency); the same wiring is proven for gRPC
/// separately by `cratestack-pg`'s `trusted_proxy_client_ip_grpc.rs` tests
/// reaching `ClientIpContext::from_extensions` off a real
/// `http::Request::extensions()`.
///
/// An earlier version of this test hand-rolled the HTTP/1.1 request over a
/// raw `tokio::net::TcpStream`, writing the request then calling
/// `AsyncWriteExt::shutdown()` before reading the response. That raced the
/// server's accept loop on CI runners with fewer cores available to a
/// `multi_thread` runtime: the early half-close could reach the server
/// before its H1 codec had fully processed the request, occasionally
/// producing a clean zero-byte EOF instead of a response (reproduced twice
/// — once locally under the default `current_thread` test runtime, once on
/// a real CI runner even under `flavor = "multi_thread"`). `reqwest`
/// doesn't shut down the write half at all for a request with an explicit
/// `Content-Length`, sidestepping that race entirely — the correctness
/// property under test (`ConnectInfo` reaching dispatch through a real
/// accept loop) doesn't depend on which HTTP client sends the request.
#[tokio::test]
async fn connect_info_from_a_real_tcp_listener_reaches_the_dispatch_path() {
    // `reqwest`'s `rustls-no-provider` feature requires a crypto provider
    // installed before building any `Client`, even for a plain `http://`
    // request that never negotiates TLS — `let _ =` because installing
    // twice (e.g. if another test in this binary already did) errors,
    // which is fine to ignore here.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let app = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["127.0.0.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    let make_service = app.into_make_service_with_connect_info::<SocketAddr>();
    let server = tokio::spawn(async move {
        cratestack::axum::serve(listener, make_service)
            .await
            .unwrap();
    });

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{local_addr}/$procs/whoAmI"))
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .header("x-forwarded-for", "203.0.113.9")
        .body(empty_args_body_bytes())
        .send()
        .await
        .expect("real HTTP request to the bound listener should succeed");
    server.abort();

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.bytes().await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // Trusted only because the real peer address the TCP handshake
    // produced is loopback (`127.0.0.1`), matching the allowlist above —
    // proving `ConnectInfo` actually carried a real, non-fabricated peer
    // address through the real connect-info-serving path.
    assert_eq!(reply.as_str().unwrap(), "203.0.113.9");
}

fn empty_args_body_bytes() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "args": {} })).unwrap()
}
