//! RPC-transport counterpart to `auth_provider_extensions.rs`: proves
//! `AuthProvider::authenticate` can read the request's `http::Extensions`
//! on `transport rpc` too, not just REST. Reuses
//! `rpc_batch_no_database.cstack` (no new fixture needed — "cheap"
//! per this ticket's coverage ask) and drives the real generated
//! `rpc_router` over axum via `oneshot`.
//!
//! The flow is identical to the REST test: a tower/axum `Extension`
//! layer inserts a marker into the request's `http::Extensions`; the
//! `AuthProvider` reads it off `RequestContext::extensions` and reflects
//! it into the `CratestackContext`; the procedure surfaces it in its reply
//! (repurposing `PingReply.nonce` as the observed-marker carrier, since
//! the fixture has no dedicated field for it) so the test can assert on
//! it through a real `POST /rpc/procedure.ping` response.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackContext, CratestackError, Value, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Debug, PartialEq, Eq)]
struct UpstreamTenant(String);

#[derive(Clone)]
struct MarkerReadingAuthProvider;

impl cratestack::AuthProvider for MarkerReadingAuthProvider {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let tenant = request
            .extensions
            .get::<UpstreamTenant>()
            .map(|marker| marker.0.clone())
            .unwrap_or_else(|| "NO-EXTENSION-SEEN".to_owned());
        core::future::ready(Ok(CratestackContext::authenticated([
            ("id".to_owned(), Value::Int(1)),
            ("tenant_marker".to_owned(), Value::String(tenant)),
        ])))
    }
}

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        _args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        let tenant = ctx
            .auth_field("tenant_marker")
            .and_then(|value| match value {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "NO-CLAIM".to_owned());
        async move { Ok(cratestack_schema::PingReply { nonce: tenant }) }
    }
}

fn router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        (),
        CborCodec,
        MarkerReadingAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn ping(app: cratestack::axum::Router) -> String {
    let frame = CborCodec
        .encode(&cratestack_schema::procedures::ping::Args {
            args: cratestack_schema::PingArgs {
                nonce: "unused".to_owned(),
            },
        })
        .expect("encode ping frame");
    let response = app
        .oneshot(
            Request::post("/rpc/procedure.ping")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("rpc dispatch should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: cratestack_schema::PingReply =
        CborCodec.decode(&bytes).expect("reply should decode");
    reply.nonce
}

/// No layer inserted an `UpstreamTenant` — the RPC path must see its
/// absence too, same as REST.
#[tokio::test]
async fn rpc_no_layer_means_auth_provider_sees_no_extension() {
    let result = ping(router()).await;
    assert_eq!(result, "NO-EXTENSION-SEEN");
}

/// The decisive assertion for `transport rpc`: a layer-inserted
/// extension reaches `AuthProvider::authenticate` through the real
/// generated `rpc_router`, exactly as it does through `router()` on
/// REST.
#[tokio::test]
async fn rpc_extension_inserted_by_a_layer_reaches_the_auth_provider() {
    let app = router().layer(cratestack::axum::Extension(UpstreamTenant(
        "acme-corp".to_owned(),
    )));
    let result = ping(app).await;
    assert_eq!(
        result, "acme-corp",
        "AuthProvider::authenticate must observe the UpstreamTenant a preceding \
         layer inserted into the request's http::Extensions, on transport rpc too"
    );
}
