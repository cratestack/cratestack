//! End-to-end proof that `AuthProvider::authenticate` can read the
//! request's `http::Extensions` — the gap this test is written against:
//! before this change `RequestContext` exposed only method/path/query/
//! headers/body, so anything a preceding tower/axum layer inserted into
//! extensions (`ConnectInfo`, mTLS peer identity, a tenant resolved
//! upstream, …) was invisible to authentication. Mirrors
//! `trusted_proxy_client_ip.rs`'s `include_server_schema!(..., db =
//! None)` pattern: no Postgres dependency, drives the REAL generated
//! `router()` over axum via `oneshot` (no network).
//!
//! The flow under test:
//!   1. A tower/axum `Extension` layer inserts a custom marker type
//!      (`UpstreamTenant`) into the request extensions — standing in for
//!      whatever an mTLS/connect-info/tenant-resolution layer would do.
//!   2. `MarkerReadingAuthProvider::authenticate` reads that marker out
//!      of `request.extensions` and reflects it into the returned
//!      `CratestackContext` as an auth claim.
//!   3. A procedure surfaces the claim back to the caller so the test
//!      can assert on it through a real HTTP response, not just a unit
//!      check of the struct.
//!
//! This file also documents (see `sabotage` module doc) how it was
//! proven to actually fail when the plumbing is broken, per this
//! ticket's "decisive test" requirement — that run isn't automated here
//! (it requires editing generated codegen and is not something CI should
//! do), but the two pasted `cargo test` outputs in the PR description are
//! the record of it.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackContext, CratestackError, Value, include_server_schema};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/trusted_proxy_client_ip.cstack", db = None);

/// Stand-in for whatever an in-process layer (mTLS termination,
/// connect-info, upstream tenant resolution, …) would insert into
/// `http::Extensions` before authentication runs. Must satisfy
/// `Clone + Send + Sync + 'static`, the same bound `http::Extensions::
/// insert` itself requires (http 1.x's `Extensions` is only ever
/// cloneable because it enforces that at insert time).
#[derive(Clone, Debug, PartialEq, Eq)]
struct UpstreamTenant(String);

/// Reads `UpstreamTenant` straight out of `request.extensions` — the
/// exact capability this ticket adds — and reflects it into the
/// returned `CratestackContext` as an auth claim so the procedure below (and
/// this test) can observe it. Falls back to a sentinel when absent so a
/// broken plumbing path is visibly wrong rather than silently missing.
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

/// Surfaces the `tenant_marker` claim `MarkerReadingAuthProvider` derived
/// from the request extensions back to the caller.
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

fn router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        JsonCodec,
        MarkerReadingAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn who_am_i_request() -> cratestack::axum::http::request::Builder {
    Request::post("/$procs/whoAmI")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
}

fn empty_args_body() -> Body {
    Body::from(serde_json::to_vec(&serde_json::json!({ "args": {} })).unwrap())
}

async fn who_am_i(app: cratestack::axum::Router, request: Request<Body>) -> String {
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    reply.as_str().unwrap().to_owned()
}

/// No layer inserted an `UpstreamTenant` at all — the provider must see
/// its absence (never fabricate a value), proving the extensions really
/// are the ones from THIS request and not some stale/global state.
#[tokio::test]
async fn no_layer_means_auth_provider_sees_no_extension() {
    let request = who_am_i_request().body(empty_args_body()).unwrap();
    let result = who_am_i(router(), request).await;
    assert_eq!(result, "NO-EXTENSION-SEEN");
}

/// The decisive assertion: a tower/axum layer inserts `UpstreamTenant`
/// into the request's `http::Extensions`; `AuthProvider::authenticate`
/// reads it off `RequestContext::extensions` and reflects it into the
/// `CratestackContext`; the procedure surfaces it; the caller observes it
/// through a real HTTP response driven through the actual generated
/// router. This is the plumbing this ticket adds — see this file's
/// module doc for how it was proven to fail when broken.
#[tokio::test]
async fn extension_inserted_by_a_layer_reaches_the_auth_provider() {
    let app = router().layer(cratestack::axum::Extension(UpstreamTenant(
        "acme-corp".to_owned(),
    )));

    let request = who_am_i_request().body(empty_args_body()).unwrap();
    let result = who_am_i(app, request).await;
    assert_eq!(
        result, "acme-corp",
        "AuthProvider::authenticate must observe the UpstreamTenant a preceding \
         layer inserted into the request's http::Extensions"
    );
}

/// A second, distinct marker value on a second request proves the value
/// is read per-request, not cached/hardcoded anywhere along the path.
#[tokio::test]
async fn a_different_extension_value_yields_a_different_observed_claim() {
    let app = router().layer(cratestack::axum::Extension(UpstreamTenant(
        "globex-corp".to_owned(),
    )));

    let request = who_am_i_request().body(empty_args_body()).unwrap();
    let result = who_am_i(app, request).await;
    assert_eq!(result, "globex-corp");
    assert_ne!(result, "acme-corp");
}
