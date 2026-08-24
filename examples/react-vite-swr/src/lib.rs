//! Server half of the `--swr`-flag end-to-end example (issue #306, the
//! last story of epic #298). Two models (`Board` has many `Task`s) plus
//! one stateless procedure, exposed over `transport rest` — see
//! `schema.cstack` for why this shape and why REST.
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
//! test in this workspace.

use cratestack::axum::Router;
use cratestack::sqlx::PgPool;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower_http::cors::CorsLayer;

cratestack::include_server_schema!("schema.cstack", db = Postgres);

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
    fn estimate_focus_minutes(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::estimate_focus_minutes::Args,
        _authorized: cratestack_schema::procedures::estimate_focus_minutes::Authorized,
    ) -> impl core::future::Future<
        Output = Result<
            cratestack_schema::procedures::estimate_focus_minutes::Output,
            CratestackError,
        >,
    > + Send {
        async move {
            let total_minutes = args.args.taskCount * args.args.minutesPerTask;
            Ok(cratestack_schema::FocusEstimateResult {
                totalMinutes: total_minutes,
            })
        }
    }
}

/// Header-based auth, same convention as every other example in this
/// repo (`x-auth-id` -> `auth().id`) — a real deployment would verify a
/// session cookie or bearer token here instead.
#[derive(Clone)]
pub struct HeaderAuthProvider;

impl AuthProvider for HeaderAuthProvider {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let ctx = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|raw| raw.parse::<i64>().ok())
            .map(|id| CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]))
            .unwrap_or_else(CratestackContext::anonymous);
        core::future::ready(Ok(ctx))
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
