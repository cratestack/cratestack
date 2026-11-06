//! Server half of the `--swr`-flag end-to-end example (issue #306, the
//! last story of epic #298). Two models (`Board` has many `Task`s), one
//! stateless procedure, and `clearDoneTasks`, which needs a step-up
//! (`@authn`, cratestack#1245; see `auth.rs` for where the facts it reads
//! come from), exposed over `transport rest` — see `schema.cstack` for why
//! this shape and why REST.
//!
//! Routes mount under `/api` (`Router::new().nest("/api", ...)` in
//! `build_router` below) to match the generated TypeScript client's
//! default `--base-path /api` — `cratestack_schema::axum::router(...)`
//! itself mounts unprefixed at the router root (`/boards`, `/tasks`,
//! ...), so server and client only agree because both sides chose `/api`
//! on purpose; nothing in the framework enforces that for you.
//!
//! `tests/smoke.rs` is the real documentation: an offline router-builds
//! test (no DB needed) plus a real-Postgres CRUD + procedure round trip,
//! gated on `CRATESTACK_TEST_DATABASE_URL` like every other PG-backed
//! test in this workspace. `tests/step_up.rs` covers the step-up.

use cratestack::axum::Router;
use cratestack::sqlx::PgPool;
use cratestack::{CratestackContext, CratestackError};
use cratestack_codec_json::JsonCodec;
use tower_http::cors::CorsLayer;

cratestack::include_server_schema!("schema.cstack", db = Postgres);

mod auth;

pub use auth::HeaderAuthProvider;
pub use cratestack_schema as schema;

/// `estimateFocusMinutes` is deliberately stateless (no DB access) —
/// same shape as the `rpc-procedures` example's `greet`/`increment`.
/// Real services would size this off historical task duration data;
/// the example keeps the arithmetic trivial so the point (a procedure
/// is a real, callable operation with its own hook, not just model
/// CRUD) isn't buried under domain logic.
#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn estimate_focus_minutes(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::estimate_focus_minutes::Args,
        _authorized: cratestack_schema::procedures::estimate_focus_minutes::Authorized,
    ) -> Result<cratestack_schema::procedures::estimate_focus_minutes::Output, CratestackError>
    {
        let total_minutes = args.args.taskCount * args.args.minutesPerTask;
        Ok(cratestack_schema::FocusEstimateResult {
            totalMinutes: total_minutes,
        })
    }

    /// Deletes the board's done tasks. It only runs for a caller who meets
    /// the schema's `@authn(maxAge: 300, amr: ["hwk"])`: the generated
    /// dispatch refuses anyone else with the step-up 401 first, so nothing
    /// here checks how the caller authenticated.
    async fn clear_done_tasks(
        &self,
        db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::clear_done_tasks::Args,
        _authorized: cratestack_schema::procedures::clear_done_tasks::Authorized,
    ) -> Result<cratestack_schema::procedures::clear_done_tasks::Output, CratestackError> {
        let removed = cratestack::sqlx::query("DELETE FROM tasks WHERE board_id = $1 AND done")
            .bind(args.args.boardId)
            .execute(db.pool())
            .await
            .map_err(|error| CratestackError::Internal(error.to_string()))?
            .rows_affected();
        Ok(cratestack_schema::ClearDoneTasksResult {
            removed: i64::try_from(removed).unwrap_or(i64::MAX),
        })
    }
}

/// Creates the two tables if they don't already exist — the same
/// `CREATE TABLE IF NOT EXISTS` pattern every PG-backed test in this
/// workspace uses (no `cratestack-migrate` step wired into any example
/// yet). Safe to call on every startup.
pub async fn ensure_schema(pool: &PgPool) -> Result<(), cratestack::sqlx::Error> {
    cratestack::sqlx::query(
        "CREATE TABLE IF NOT EXISTS boards (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
    )
    .execute(pool)
    .await?;
    cratestack::sqlx::query(
        "CREATE TABLE IF NOT EXISTS tasks (\
           id BIGINT PRIMARY KEY, \
           title TEXT NOT NULL, \
           done BOOLEAN NOT NULL, \
           board_id BIGINT NOT NULL REFERENCES boards(id)\
         )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Builds the full app router: generated model/procedure routes nested
/// under `/api`, permissive CORS (dev-only convenience — a real
/// deployment would scope `allow_origin` to its actual front-end
/// origin(s)) so the Vite dev server on a different port can call it.
pub fn build_router(db: cratestack_schema::Cratestack) -> Router {
    let inner = cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        HeaderAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    Router::new().nest("/api", inner).layer(
        CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any),
    )
}
