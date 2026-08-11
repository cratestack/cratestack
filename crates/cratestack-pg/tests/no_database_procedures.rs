//! cratestack#328: under `db = None`, `Cratestack::builder()` and the
//! generated router state carry **zero** `PgPool`/connection-string/`sqlx`
//! shape anywhere — not an unused parameter, not an `Option<PgPool>` that
//! happens to always be `None`. This test's own setup code proves it: no
//! `cratestack::sqlx` import, no connection string, no pool of any kind.
//!
//! Compare with cratestack#327's original version of this file (see git
//! history), which still built a `sqlx::PgPool` via `connect_lazy` to
//! satisfy `Cratestack::builder(pool)` — that workaround is exactly what
//! this story removes. `Cratestack::builder()` now takes no arguments at
//! all under `db = None`.
//!
//! The negative half of the datasource/macro-argument cross-check (a
//! mismatch failing to compile) is still demonstrated manually per the
//! PR description, following the same precedent as `reject_grpc.rs`'s
//! composite-PK guard: a `proc_macro::TokenStream` compile-error path
//! can't be exercised from a plain `cargo test` run.

use cratestack::CoolCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CoolContext, CoolError};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

/// The fixture's `ping` procedure declares `@allow(auth() != null)`, so
/// this test's auth provider always returns an authenticated context —
/// what it authenticates has nothing to do with a database (there isn't
/// one under `db = None`), it's purely a `CoolContext` predicate.
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

/// `Cratestack::builder()` — no `PgPool` parameter, no connection string,
/// no `sqlx` type in sight. This is the whole point of cratestack#328.
fn build_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

#[test]
fn no_database_schema_declares_zero_models_and_one_procedure() {
    assert_eq!(cratestack_schema::MODEL_COUNT, 0);
    assert_eq!(cratestack_schema::PROCEDURE_COUNT, 1);
    assert_eq!(cratestack_schema::TRANSPORT_STYLE, "rest");
}

#[tokio::test]
async fn no_database_schema_procedure_handler_still_dispatches() {
    let db = cratestack_schema::Cratestack::builder().build();
    let procedures = Procedures;
    let output = cratestack_schema::procedures::ProcedureRegistry::ping(
        &procedures,
        &db,
        &CoolContext::anonymous(),
        cratestack_schema::procedures::ping::Args {
            args: cratestack_schema::PingArgs {
                message: "hello".to_owned(),
            },
        },
    )
    .await
    .expect("ping handler should succeed");

    assert_eq!(output.echo, "hello");
}

/// The story's headline evidence: the *generated router* — built from a
/// `db = None` `Cratestack` with no pool anywhere — round-trips a real
/// HTTP procedure call end to end.
#[tokio::test]
async fn no_database_router_round_trips_ping_procedure_over_http() {
    let app = build_router();

    let body = serde_json::json!({ "args": { "message": "hello" } });
    let response = app
        .oneshot(
            Request::post("/$procs/ping")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["echo"], "hello");
}
