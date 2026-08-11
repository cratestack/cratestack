//! #415: proves the trusted-proxy `client_ip` protection reaches the
//! separately-built gRPC `into_router()`, not just `router()`. `service.rs`
//! (`ApiServer::call`) is a raw tonic `Service`, not an axum handler, so it
//! reads `Extension<TrustedProxyConfig>`/`ConnectInfo<SocketAddr>` straight
//! off `http::Request::extensions()` via `ClientIpContext::from_extensions`
//! rather than through axum's extractor machinery — this is the seam that
//! proves that code path actually runs. Mirrors `transport_grpc.rs`'s
//! `connect_lazy` (no live Postgres) + `frame_grpc_message`/
//! `strip_grpc_frame` pattern.
//!
//! Run with `cargo test -p cratestack-pg --features grpc --test
//! trusted_proxy_client_ip_grpc`.

#![cfg(feature = "grpc")]

use std::net::SocketAddr;

use cratestack::axum::extract::ConnectInfo;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CodecSet, TrustedProxyConfig, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_grpc::{frame_grpc_message, strip_grpc_frame};
use prost::Message as _;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/transport_grpc.cstack", db = Postgres);

fn test_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl std::future::Future<Output = Result<cratestack::CoolContext, Self::Error>> + Send
    {
        // Deliberately does NOT set `client_ip` itself — that's exactly
        // what `enrich_context_from_headers` (called after `authenticate`
        // inside every generated dispatch fn, `client_ip_ctx` in hand) is
        // responsible for.
        std::future::ready(Ok(cratestack::CoolContext::authenticated([])))
    }
}

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn echo_widget_name(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &cratestack::CoolContext,
        args: cratestack_schema::procedures::echo_widget_name::Args,
    ) -> impl std::future::Future<
        Output = Result<
            cratestack_schema::procedures::echo_widget_name::Output,
            cratestack::CoolError,
        >,
    > + Send {
        async move { Ok(format!("echo: {}", args.name)) }
    }

    fn widget_name_samples(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &cratestack::CoolContext,
        _args: cratestack_schema::procedures::widget_name_samples::Args,
    ) -> impl std::future::Future<
        Output = Result<
            cratestack_schema::procedures::widget_name_samples::Output,
            cratestack::CoolError,
        >,
    > + Send {
        async move { Ok(vec!["alpha".to_owned(), "beta".to_owned()]) }
    }

    /// Surfaces the resolved `client_ip` back to the caller so the test
    /// can assert on it — the whole point of this fixture procedure.
    fn who_am_i(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &cratestack::CoolContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
    ) -> impl std::future::Future<
        Output = Result<cratestack_schema::procedures::who_am_i::Output, cratestack::CoolError>,
    > + Send {
        let client_ip = ctx.client_ip().unwrap_or("none").to_owned();
        async move { Ok(client_ip) }
    }
}

async fn call_who_am_i(
    router: cratestack::axum::Router,
) -> cratestack_schema::grpc::pb::WhoAmIOutput {
    let input = cratestack_schema::grpc::pb::WhoAmIInput {};
    let framed = frame_grpc_message(&input.encode_to_vec(), false);

    let request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ProcedureWhoAmI")
        .header("content-type", "application/grpc")
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), cratestack::axum::http::StatusCode::OK);

    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body).expect("response must carry one gRPC message frame");
    cratestack_schema::grpc::pb::WhoAmIOutput::decode(unframed)
        .expect("response frame must decode as WhoAmIOutput")
}

fn router() -> cratestack::axum::Router {
    let db = test_db();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    cratestack_schema::grpc::into_router(db, Procedures, codec, AllowAllAuth)
}

/// The headline evidence for decision 3 in `into_router()` specifically:
/// with no `Extension<TrustedProxyConfig>` layered onto the gRPC router at
/// all, `client_ip` is `None` even though a spoofed header is present.
/// This is the exact acceptance criterion that a fix scoped only to
/// `router()` would silently fail for `transport grpc` schemas.
#[tokio::test]
async fn grpc_router_default_ignores_forwarded_headers_and_yields_none() {
    let router = router();

    let input = cratestack_schema::grpc::pb::WhoAmIInput {};
    let framed = frame_grpc_message(&input.encode_to_vec(), false);
    let request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ProcedureWhoAmI")
        .header("content-type", "application/grpc")
        .header("x-forwarded-for", "203.0.113.9")
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body).unwrap();
    let output = cratestack_schema::grpc::pb::WhoAmIOutput::decode(unframed).unwrap();

    assert_eq!(output.result.as_deref(), Some("none"));
}

/// Same untrusted-peer proof as the REST/RPC integration test, but through
/// `into_router()`: a `ConnectInfo` peer that is NOT in the allowlist must
/// not have the spoofed header trusted — the real (though unauthorized)
/// peer address is recorded instead.
#[tokio::test]
async fn grpc_router_untrusted_peer_ignores_spoofed_header() {
    let router = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let input = cratestack_schema::grpc::pb::WhoAmIInput {};
    let framed = frame_grpc_message(&input.encode_to_vec(), false);
    let mut request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ProcedureWhoAmI")
        .header("content-type", "application/grpc")
        .header("x-forwarded-for", "203.0.113.9")
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();
    let untrusted_peer: SocketAddr = "10.0.0.9:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(untrusted_peer));

    let response = router.oneshot(request).await.unwrap();
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body).unwrap();
    let output = cratestack_schema::grpc::pb::WhoAmIOutput::decode(unframed).unwrap();

    // The real (untrusted) peer address, never the spoofed header value.
    assert_eq!(output.result.as_deref(), Some("10.0.0.9"));
}

/// The other half: a `ConnectInfo` peer that IS in the allowlist gets the
/// `Forwarded`/`X-Forwarded-For` chain honored — proving the trust path,
/// not just the distrust path, reaches `into_router()`.
#[tokio::test]
async fn grpc_router_trusted_peer_resolves_forwarded_header() {
    let router = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let output = {
        let input = cratestack_schema::grpc::pb::WhoAmIInput {};
        let framed = frame_grpc_message(&input.encode_to_vec(), false);
        let mut request = cratestack::axum::http::Request::builder()
            .method("POST")
            .uri("/widgets_api.Api/ProcedureWhoAmI")
            .header("content-type", "application/grpc")
            .header("x-forwarded-for", "203.0.113.9")
            .version(cratestack::axum::http::Version::HTTP_2)
            .body(cratestack::axum::body::Body::from(framed))
            .unwrap();
        let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(trusted_peer));

        let response = router.oneshot(request).await.unwrap();
        let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let unframed = strip_grpc_frame(&body).unwrap();
        cratestack_schema::grpc::pb::WhoAmIOutput::decode(unframed).unwrap()
    };

    assert_eq!(output.result.as_deref(), Some("203.0.113.9"));
}

/// Sanity check that the fixture's `whoAmI` procedure still round-trips a
/// well-formed value end-to-end without any trusted-proxy wiring at all —
/// keeps `call_who_am_i` exercised so it doesn't rot as dead code.
#[tokio::test]
async fn grpc_who_am_i_round_trips_without_any_client_ip_signal() {
    let output = call_who_am_i(router()).await;
    assert_eq!(output.result.as_deref(), Some("none"));
}

/// Finding 1, reproduced through `into_router()`: an attacker-authored
/// `Forwarded` header must not override the proxy-appended
/// `X-Forwarded-For` chain under the default header selection.
#[tokio::test]
async fn grpc_router_forwarded_header_from_attacker_does_not_override_trusted_xff() {
    let router = router().layer(cratestack::axum::Extension(
        TrustedProxyConfig::trusting(["198.51.100.1".parse::<std::net::IpAddr>().unwrap().into()])
            .max_hops(1),
    ));

    let input = cratestack_schema::grpc::pb::WhoAmIInput {};
    let framed = frame_grpc_message(&input.encode_to_vec(), false);
    let mut request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ProcedureWhoAmI")
        .header("content-type", "application/grpc")
        .header("x-forwarded-for", "6.6.6.6, 203.0.113.9")
        .header("forwarded", "for=\"666.666.666.666\"")
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();
    let trusted_peer: SocketAddr = "198.51.100.1:9000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(trusted_peer));

    let response = router.oneshot(request).await.unwrap();
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body).unwrap();
    let output = cratestack_schema::grpc::pb::WhoAmIOutput::decode(unframed).unwrap();

    assert_eq!(output.result.as_deref(), Some("203.0.113.9"));
    assert_ne!(output.result.as_deref(), Some("666.666.666.666"));
}
