//! `transport grpc` server codegen smoke test (ticket #171). Gated on the
//! `grpc` Cargo feature — without it, `include_server_schema!` against a
//! `transport grpc` schema is a `compile_error!` by design (see
//! `crates/cratestack-macros/src/include/reject_grpc.rs`), so this whole
//! file is skipped rather than failing a default `cargo test -p
//! cratestack-pg` run. Run with `cargo test -p cratestack-pg --features
//! grpc --test transport_grpc`.
//!
//! Uses `connect_lazy` (no live Postgres needed) — this test's job is
//! proving the *generated code compiles and mounts*, i.e. that the tonic
//! service, the pb mirror structs, and `into_router` all typecheck against
//! a real schema + committed `.pb.lock`. Dispatch-level (DB-backed)
//! coverage lives in `just test-pg`'s `banking_*`/`generated_client_rust`
//! style tests, which this fixture doesn't yet have a gRPC-client
//! counterpart for — see this ticket's final report for what's covered
//! vs. not.

#![cfg(feature = "grpc")]

use cratestack::CodecSet;
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_grpc::{frame_grpc_message, strip_grpc_frame};
use prost::Message as _;

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
        std::future::ready(Ok(cratestack::CoolContext::authenticated([])))
    }
}

/// Ticket #208: neither procedure touches the database — same
/// minimalism as `examples/rpc-procedures`' own `Procedures` registry;
/// this fixture's job is proving gRPC dispatch, not business logic.
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

    /// #415: exercised by `trusted_proxy_client_ip_grpc.rs`, not this
    /// file — kept minimal here since this file's job is proving the
    /// generated code compiles/mounts, not the trusted-proxy behavior.
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

/// Proves `cratestack_schema::grpc::pb` exists with the expected mirror
/// types and `From`/`TryFrom` conversions actually typecheck against the
/// domain structs.
#[test]
fn pb_mirror_round_trips_widget() {
    let domain = cratestack_schema::Widget {
        id: 1,
        name: "Alpha".to_owned(),
    };
    let mirror = cratestack_schema::grpc::pb::Widget::from(&domain);
    assert_eq!(mirror.id, Some(1));
    assert_eq!(mirror.name, Some("Alpha".to_owned()));

    let back = cratestack_schema::Widget::try_from(mirror).expect("round trip should succeed");
    assert_eq!(back, domain);
}

/// Proves the tonic service actually mounts into an `axum::Router` —
/// `docs/design/protobuf.md` §7.2's axum/tonic alignment claim, exercised
/// for real rather than just asserted from `cargo tree` output.
#[tokio::test]
async fn grpc_service_mounts_into_axum_router() {
    let db = test_db();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let _router: cratestack::axum::Router =
        cratestack_schema::grpc::into_router(db, Procedures, codec, AllowAllAuth);
}

/// Ticket #172, Part A: the macro-generated `into_router` must come back
/// wrapped in the gRPC-Web + CORS layering (`::cratestack::grpc::
/// apply_grpc_web`), not just a bare tonic `Routes` translation.
/// `cratestack-grpc::web`'s own unit tests already prove the layering
/// primitive in isolation; this test proves the macro's call site actually
/// applies it — a real request through the real generated router exposes
/// `grpc-status`/`grpc-message`/`grpc-status-details-bin`, which is
/// `docs/design/protobuf.md` §7.4's single highest-severity failure mode
/// if silently missing (request "succeeds", browser sees no status).
#[tokio::test]
async fn generated_router_exposes_grpc_status_headers_for_browsers() {
    use tower::ServiceExt;

    let db = test_db();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let router: cratestack::axum::Router =
        cratestack_schema::grpc::into_router(db, Procedures, codec, AllowAllAuth);

    let request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ModelWidgetList")
        .header("origin", "http://example.com")
        .header("content-type", "application/grpc-web+proto")
        .body(cratestack::axum::body::Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    let exposed = response
        .headers()
        .get("access-control-expose-headers")
        .expect("Access-Control-Expose-Headers must be present on the generated router")
        .to_str()
        .unwrap();
    for header_name in ["grpc-status", "grpc-message", "grpc-status-details-bin"] {
        assert!(
            exposed.contains(header_name),
            "expected '{header_name}' in Access-Control-Expose-Headers, got '{exposed}'"
        );
    }
}

/// Ticket #208, AC 1: a unary procedure dispatches through the router —
/// decoded pb request -> `procedures::echo_widget_name::Args` -> the
/// exact same `handle_echo_widget_name_dispatch` fn REST/RPC would call
/// -> bridged back into `pb::EchoWidgetNameOutput`. A real gRPC frame in,
/// a real gRPC frame out — `strip_grpc_frame`/`prost::Message::decode`
/// on the response body proves the dispatch actually ran and returned
/// the expected value, not just that the route matched.
#[tokio::test]
async fn grpc_unary_procedure_dispatches_through_router() {
    use tower::ServiceExt;

    let db = test_db();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let router: cratestack::axum::Router =
        cratestack_schema::grpc::into_router(db, Procedures, codec, AllowAllAuth);

    let input = cratestack_schema::grpc::pb::EchoWidgetNameInput {
        name: Some("gizmo".to_owned()),
    };
    let framed = frame_grpc_message(&input.encode_to_vec(), false);

    let request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ProcedureEchoWidgetName")
        .header("content-type", "application/grpc")
        // `tonic_web::GrpcWebLayer` (applied by `apply_grpc_web`, mounted
        // by every macro-generated `into_router`) only passes a
        // non-grpc-web `Content-Type` straight through to the inner
        // tonic service on real HTTP/2 — see `tonic-web-0.13.1`'s
        // `GrpcWebService::call`, `RequestKind::Other(Version::HTTP_2)`
        // vs. a bare 400 for every other HTTP version. This in-process
        // `tower::ServiceExt::oneshot` call never negotiates ALPN, so the
        // version has to be set explicitly rather than assumed.
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), cratestack::axum::http::StatusCode::OK);

    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body).expect("response must carry one gRPC message frame");
    let output = cratestack_schema::grpc::pb::EchoWidgetNameOutput::decode(unframed)
        .expect("response frame must decode as EchoWidgetNameOutput");
    assert_eq!(output.result.as_deref(), Some("echo: gizmo"));
}

/// Ticket #208, AC 2: a `List`-arity procedure dispatches through the
/// router using tonic's `ServerStreamingService` — see `service.rs`'s
/// module doc for exactly what "streams" means here (the whole
/// `repeated result` list travels in the one streamed message this
/// fixture's `widgetNameSamples` produces). The load-bearing part of
/// this test is that `grpc.server_streaming(...)` — not
/// `grpc.unary(...)` — is what answered the call at all.
#[tokio::test]
async fn grpc_streaming_procedure_dispatches_through_router() {
    use tower::ServiceExt;

    let db = test_db();
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let router: cratestack::axum::Router =
        cratestack_schema::grpc::into_router(db, Procedures, codec, AllowAllAuth);

    let input = cratestack_schema::grpc::pb::WidgetNameSamplesInput {};
    let framed = frame_grpc_message(&input.encode_to_vec(), false);

    let request = cratestack::axum::http::Request::builder()
        .method("POST")
        .uri("/widgets_api.Api/ProcedureWidgetNameSamples")
        .header("content-type", "application/grpc")
        // `tonic_web::GrpcWebLayer` (applied by `apply_grpc_web`, mounted
        // by every macro-generated `into_router`) only passes a
        // non-grpc-web `Content-Type` straight through to the inner
        // tonic service on real HTTP/2 — see `tonic-web-0.13.1`'s
        // `GrpcWebService::call`, `RequestKind::Other(Version::HTTP_2)`
        // vs. a bare 400 for every other HTTP version. This in-process
        // `tower::ServiceExt::oneshot` call never negotiates ALPN, so the
        // version has to be set explicitly rather than assumed.
        .version(cratestack::axum::http::Version::HTTP_2)
        .body(cratestack::axum::body::Body::from(framed))
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), cratestack::axum::http::StatusCode::OK);

    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let unframed = strip_grpc_frame(&body).expect("response must carry one gRPC message frame");
    let output = cratestack_schema::grpc::pb::WidgetNameSamplesOutput::decode(unframed)
        .expect("response frame must decode as WidgetNameSamplesOutput");
    assert_eq!(output.result, vec!["alpha".to_owned(), "beta".to_owned()]);
}
