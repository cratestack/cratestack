//! Regression guard for `docs/design/boot-surface.md` §4.1: the generated
//! `ProcedureRegistry` trait and `AuthProvider` are implementable with
//! **plain `async fn` methods** — no hand-written
//! `-> impl Future<Output = …> + Send { async move { … } }`, no attribute,
//! no boxing. Rust ≥ 1.75 lets an impl satisfy an `impl Future + Send`
//! trait method with an `async fn` and checks the `Send` bound on the
//! concrete future; every example in this repo used to spell the long form
//! anyway, and `justfile`'s `-A clippy::manual_async_fn` was silencing the
//! lint that says so.
//!
//! If a future change to the generated trait — a boxed future, a lifetime
//! the `async fn` desugaring cannot express — ever broke this property,
//! this file would stop compiling, which is the point. Drives the REAL
//! generated `router()` over axum via `oneshot`; `db = None`, so no
//! Postgres. Same fixture as `no_database_procedures.rs`.
//!
//! The procedure body `.await`s before touching its borrowed `ctx`, so the
//! future genuinely captures the reference arguments across a suspension
//! point rather than only in a trivially-ready body.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> Result<cratestack_schema::procedures::ping::Output, CratestackError> {
        tokio::task::yield_now().await;
        Ok(cratestack_schema::PingReply {
            echo: format!("{}:{}", args.args.message, ctx.is_authenticated()),
        })
    }
}

/// Authenticated iff `x-auth-id` is present — the fixture's `ping` declares
/// `@allow(auth() != null)`, so the anonymous branch is what proves policy
/// still runs in front of an `async fn` registry.
#[derive(Clone)]
struct HeaderAuth;

impl cratestack::AuthProvider for HeaderAuth {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        Ok(match request.headers.get("x-auth-id") {
            Some(_) => {
                CratestackContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))])
            }
            None => CratestackContext::anonymous(),
        })
    }
}

fn build_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        HeaderAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn ping_request() -> cratestack::axum::http::request::Builder {
    Request::post("/$procs/ping")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
}

async fn body_json(response: cratestack::axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("json body")
}

#[tokio::test]
async fn plain_async_fn_registry_and_auth_provider_dispatch_through_the_real_router() {
    let response = build_router()
        .oneshot(
            ping_request()
                .header("x-auth-id", "1")
                .body(Body::from(r#"{"args":{"message":"hello"}}"#))
                .expect("request"),
        )
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["echo"], "hello:true", "got: {json}");
}

#[tokio::test]
async fn policy_still_runs_in_front_of_an_async_fn_registry() {
    let response = build_router()
        .oneshot(
            ping_request()
                .body(Body::from(r#"{"args":{"message":"hello"}}"#))
                .expect("request"),
        )
        .await
        .expect("router responds");

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "anonymous caller must be denied by @allow(auth() != null)"
    );
}
