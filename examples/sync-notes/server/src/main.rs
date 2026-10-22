//! sync-notes server: the generated CrateStack surface for `schema.cstack`
//! plus `cratestack-sync`, on one axum router.
//!
//! ```bash
//! export DATABASE_URL=postgres://postgres@localhost:55432/sync_notes
//! cargo run -p sync-notes-server          # http://localhost:8787
//! ```
//!
//! See the example README for building the wasm client the page loads.

mod auth;
mod demo;
mod sync;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cratestack::axum::Router;
use cratestack::ratelimit::{InMemoryRateLimitStore, RateLimitConfig};
use cratestack::sqlx::{self, PgPool};
use cratestack_codec_json::JsonCodec;
use cratestack_sync::SyncPolicy;
use cratestack_sync::server::{CaptureSpec, SyncServer, sync_router};

cratestack::include_server_schema!("../schema.cstack", db = Postgres);

const NOTES_DDL: &str = "CREATE TABLE IF NOT EXISTS notes (
    id UUID PRIMARY KEY,
    owner_id TEXT NOT NULL,
    title TEXT NOT NULL,
    done BOOLEAN NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
)";

#[derive(Clone)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "set DATABASE_URL to a Postgres database for the example")?;
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let web_dir = std::env::var("WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web"));

    let pool = PgPool::connect(&url).await?;
    sqlx::raw_sql(NOTES_DDL).execute(&pool).await?;
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();

    // TTLs are shortened from the defaults so the demo can show them; the
    // validated invariant (client TTL > mutation TTL) still holds.
    let policy = SyncPolicy {
        mutation_ttl_secs: 24 * 60 * 60,
        client_ttl_secs: 7 * 24 * 60 * 60,
        tombstone_ttl_secs: 7 * 24 * 60 * 60,
        ..SyncPolicy::default()
    };
    let limiter = Arc::new(InMemoryRateLimitStore::new());
    let sync = SyncServer::builder(pool.clone(), sync::registry(db.clone()), auth::principal)
        .policy(policy)
        .rate_limit(limiter, RateLimitConfig::new(120, 10.0))
        .build()?;
    sync.install().await?;
    sync.capture(CaptureSpec {
        table: cratestack_schema::NOTE_MODEL.table_name,
        model: "Note",
        pk_column: cratestack_schema::NOTE_MODEL.primary_key,
        scope_sql: "'user:' || r.owner_id",
    })
    .await?;
    sync.spawn_background(Duration::from_secs(60));

    let generated = cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        auth::DemoAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let app = Router::new()
        .nest("/api", generated)
        .merge(sync_router(sync, auth::DemoAuth))
        .merge(demo::router(pool, web_dir));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("sync-notes listening on http://localhost:{port}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    cratestack::axum::serve(listener, app).await?;
    Ok(())
}
