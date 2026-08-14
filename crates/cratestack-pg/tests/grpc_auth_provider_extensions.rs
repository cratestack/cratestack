//! gRPC-transport counterpart to `cratestack-api`'s
//! `auth_provider_extensions.rs`/`rpc_auth_provider_extensions.rs`:
//! proves `AuthProvider::authenticate` can read the request's
//! `http::Extensions` through `into_router()` too, not just `router()`/
//! `rpc_router()`. `service.rs` (`ApiServer::call`) is a raw tonic
//! `Service`, not an axum handler — it reads extensions straight off
//! `http::Request::extensions()` via `ClientIpContext::from_extensions`
//! (see `crates/cratestack-macros/src/include/server/grpc/api_server.rs`)
//! rather than through axum's extractor machinery, so this is the seam
//! that proves that code path independently carries the new field.
//! Mirrors `trusted_proxy_client_ip_grpc.rs`'s `connect_lazy` (no live
//! Postgres) pattern and reuses its `transport_grpc.cstack` fixture.
//!
//! Run with `cargo test -p cratestack-pg --features grpc --test
//! grpc_auth_provider_extensions`.

#![cfg(feature = "grpc")]

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CodecSet, CratestackContext, CratestackError, Value, include_server_schema};
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct UpstreamTenant(String);

/// Reads `UpstreamTenant` off `RequestContext::extensions` and reflects
/// it into the returned `CratestackContext` as an auth claim — same shape as
/// the REST/RPC tests, proving the capability is transport-agnostic.
#[derive(Clone)]
struct MarkerReadingAuthProvider;

impl cratestack::AuthProvider for MarkerReadingAuthProvider {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> impl std::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let tenant = request
            .extensions
            .get::<UpstreamTenant>()
            .map(|marker| marker.0.clone())
            .unwrap_or_else(|| "NO-EXTENSION-SEEN".to_owned());
        std::future::ready(Ok(CratestackContext::authenticated([
            ("id".to_owned(), Value::Int(1)),
            ("tenant_marker".to_owned(), Value::String(tenant)),
        ])))
    }
}

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn echo_widget_name(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::echo_widget_name::Args,
        _authorized: cratestack_schema::procedures::echo_widget_name::Authorized,
    ) -> impl std::future::Future<
        Output = Result<cratestack_schema::procedures::echo_widget_name::Output, CratestackError>,
    > + Send {
        async move { Ok(format!("echo: {}", args.name)) }
    }

    fn widget_name_samples(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: cratestack_schema::procedures::widget_name_samples::Args,
        _authorized: cratestack_schema::procedures::widget_name_samples::Authorized,
    ) -> impl std::future::Future<
        Output = Result<
            cratestack_schema::procedures::widget_name_samples::Output,
            CratestackError,
        >,
    > + Send {
        async move { Ok(vec!["alpha".to_owned(), "beta".to_owned()]) }
    }

    /// Repurposed (this file only) to surface the `tenant_marker` claim
    /// `MarkerReadingAuthProvider` derived from the request extensions,
    /// rather than `client_ip` as `trusted_proxy_client_ip_grpc.rs` uses
    /// it for.
    fn who_am_i(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
        _authorized: cratestack_schema::procedures::who_am_i::Authorized,
    ) -> impl std::future::Future<
        Output = Result<cratestack_schema::procedures::who_am_i::Output, CratestackError>,
    > + Send {
        let tenant = ctx
            .auth_field("tenant_marker")
            .and_then(|value| match value {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "NO-CLAIM".to_owned());
        async move { Ok(tenant) }
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
    cratestack_schema::grpc::into_router(db, Procedures, codec, MarkerReadingAuthProvider)
}

/// No layer inserted an `UpstreamTenant` — the gRPC path must see its
/// absence too, same as REST/RPC.
#[tokio::test]
async fn grpc_no_layer_means_auth_provider_sees_no_extension() {
    let output = call_who_am_i(router()).await;
    assert_eq!(output.result.as_deref(), Some("NO-EXTENSION-SEEN"));
}

/// The decisive assertion for gRPC: a layer-inserted extension reaches
/// `AuthProvider::authenticate` through the real generated
/// `into_router()`, exactly as it does through `router()`/`rpc_router()`
/// on REST/RPC.
#[tokio::test]
async fn grpc_extension_inserted_by_a_layer_reaches_the_auth_provider() {
    let app = router().layer(cratestack::axum::Extension(UpstreamTenant(
        "acme-corp".to_owned(),
    )));
    let output = call_who_am_i(app).await;
    assert_eq!(
        output.result.as_deref(),
        Some("acme-corp"),
        "AuthProvider::authenticate must observe the UpstreamTenant a preceding \
         layer inserted into the request's http::Extensions, on transport grpc too",
    );
}
