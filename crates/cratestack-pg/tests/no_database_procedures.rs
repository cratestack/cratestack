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
use cratestack::{CoolContext, CoolError, SystemContext};
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
        _authorized: cratestack_schema::procedures::ping::Authorized,
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

/// cratestack#512: this test used to call
/// `ProcedureRegistry::ping(&procedures, &db, &CoolContext::anonymous(),
/// args)` directly — the exact silent-bypass shape that ticket describes.
/// It "passed" with an *anonymous* context despite `ping` declaring
/// `@allow(auth() != null)`, because that direct call never ran policy at
/// all; this file itself was live evidence of the bug, not just the ticket
/// text. `ProcedureRegistry::ping` now takes an `Authorized` witness only
/// `invoke_with_db`/`authorize_with_db` can construct, so the old call no
/// longer compiles — this test now goes through `invoke_with_db`, the same
/// entry point the generated axum handler uses, with an authenticated
/// context (anonymous would now correctly be `Forbidden`).
#[tokio::test]
async fn no_database_schema_procedure_handler_still_dispatches() {
    let db = cratestack_schema::Cratestack::builder().build();
    let procedures = Procedures;
    let ctx = CoolContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))]);
    let args = cratestack_schema::procedures::ping::Args {
        args: cratestack_schema::PingArgs {
            message: "hello".to_owned(),
        },
    };

    let call_args = args.clone();
    let call_ctx = ctx.clone();
    let output = cratestack_schema::procedures::ping::invoke_with_db(
        &db,
        &args,
        &ctx,
        |authorized| async move {
            cratestack_schema::procedures::ProcedureRegistry::ping(
                &procedures,
                &db,
                &call_ctx,
                call_args,
                authorized,
            )
            .await
        },
    )
    .await
    .expect("ping handler should succeed");

    assert_eq!(output.echo, "hello");
}

/// The other half of the cratestack#512 regression: the exact context the
/// old direct call used (`CoolContext::anonymous()`) must now be denied —
/// `ping` declares `@allow(auth() != null)`, so this was always a policy
/// violation the old call shape silently let through.
#[tokio::test]
async fn no_database_schema_procedure_denies_anonymous_caller() {
    let db = cratestack_schema::Cratestack::builder().build();
    let args = cratestack_schema::procedures::ping::Args {
        args: cratestack_schema::PingArgs {
            message: "hello".to_owned(),
        },
    };

    let call_args = args.clone();
    let error = cratestack_schema::procedures::ping::invoke_with_db(
        &db,
        &args,
        &CoolContext::anonymous(),
        |authorized| async move {
            let procedures = Procedures;
            cratestack_schema::procedures::ProcedureRegistry::ping(
                &procedures,
                &db,
                &CoolContext::anonymous(),
                call_args,
                authorized,
            )
            .await
        },
    )
    .await
    .expect_err("anonymous caller must be denied by @allow(auth() != null)");
    assert!(matches!(error, CoolError::Forbidden(_)));
}

/// cratestack#512's other required coverage: a legitimate internal caller
/// (a cron job, background worker, or admin tool) using `auth().isSystem()`
/// (cratestack#486)'s sanctioned identity — [`SystemContext`] — must still
/// be able to call a procedure through the enforced path. `ping` only
/// declares `@allow(auth() != null)`, not a system-specific clause, but
/// `SystemContext` is always authenticated (`is_authenticated() == true`,
/// see `cratestack_core::context::system`'s own
/// `system_context_is_system_and_authenticated` test), so it satisfies
/// that predicate exactly the way any other authenticated caller would —
/// proving the fix didn't turn "internal caller" into "caller who can
/// never pass policy".
#[tokio::test]
async fn no_database_schema_procedure_admits_a_system_caller() {
    let db = cratestack_schema::Cratestack::builder().build();
    let procedures = Procedures;
    let ctx = SystemContext::for_service("nightly-ping-reconciler").into_context();
    assert!(
        ctx.is_system(),
        "fixture ctx should be a real system context"
    );
    let args = cratestack_schema::procedures::ping::Args {
        args: cratestack_schema::PingArgs {
            message: "reconcile".to_owned(),
        },
    };

    let call_args = args.clone();
    let call_ctx = ctx.clone();
    let output = cratestack_schema::procedures::ping::invoke_with_db(
        &db,
        &args,
        &ctx,
        |authorized| async move {
            cratestack_schema::procedures::ProcedureRegistry::ping(
                &procedures,
                &db,
                &call_ctx,
                call_args,
                authorized,
            )
            .await
        },
    )
    .await
    .expect("a system-principal caller should pass @allow(auth() != null)");

    assert_eq!(output.echo, "reconcile");
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
