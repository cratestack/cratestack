//! CrateStack's MCP operator end to end (ADR 0002, cratestack#1041).
//!
//! One Postgres-backed schema (`schema.cstack`) exposes two procedures as
//! MCP tools and one model as a read-only MCP resource. `src/main.rs` serves
//! them over stdio or Streamable HTTP; `src/token.rs` is the example
//! audience-checking `AuthProvider` both transports verify callers with.
//!
//! Nothing here enforces a policy by hand. A tool call reaches
//! [`Procedures`] only through the procedure's generated `invoke_with_db`,
//! which runs `@allow` first, and every ORM call below carries the model's
//! `@@allow` in its SQL. MCP adds no bypass (ADR 0002 § Decision).

pub mod http;
pub mod token;

use cratestack::sqlx::PgPool;
use cratestack::{CratestackContext, CratestackError};

cratestack::include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema as schema;

use cratestack_schema::procedures::{publish_post, recent_posts};

/// The largest page `recent_posts` returns, whatever `limit` asks for.
pub const MAX_RECENT: i64 = 20;

/// The procedure implementations the generated tool table calls.
#[derive(Clone, Default)]
pub struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn recent_posts(
        &self,
        db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: recent_posts::Args,
        _authorized: recent_posts::Authorized,
    ) -> Result<recent_posts::Output, CratestackError> {
        // No visibility filter here: rows the caller may not read never
        // leave Postgres, because `@@allow("read", ...)` is in the SQL.
        db.post()
            .find_many()
            .order_by(cratestack_schema::post::id().desc())
            .limit(args.limit.clamp(1, MAX_RECENT))
            .run(ctx)
            .await
    }

    async fn publish_post(
        &self,
        db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: publish_post::Args,
        _authorized: publish_post::Authorized,
    ) -> Result<publish_post::Output, CratestackError> {
        // `@allow` already admitted the caller as an editor; the update
        // also carries the model's `@@allow("update", ...)` in its SQL.
        db.post()
            .update(args.id)
            .set(cratestack_schema::UpdatePostInput {
                authorId: None,
                title: None,
                published: Some(true),
            })
            .run(ctx)
            .await
    }
}

/// The schema's MCP table: its tools and its resources. No `@computed`
/// field, so no resolver (`()`).
pub fn mcp_table(
    db: cratestack_schema::Cratestack,
) -> cratestack_schema::mcp::McpTools<Procedures, ()> {
    cratestack_schema::mcp::tools(db, Procedures, ())
}

/// Creates the `posts` table and seeds four rows if they are missing. The
/// same `CREATE TABLE IF NOT EXISTS` pattern the repository's other Postgres
/// examples use; safe on every start.
///
/// | id | author | published | who may read it |
/// |----|--------|-----------|-----------------|
/// | 1  | `u-1`  | yes       | anyone signed in |
/// | 2  | `u-1`  | no        | `u-1` only |
/// | 3  | `u-2`  | no        | `u-2` only |
/// | 4  | `u-2`  | yes       | anyone signed in |
pub async fn ensure_schema(pool: &PgPool) -> Result<(), cratestack::sqlx::Error> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS posts (id BIGINT PRIMARY KEY, author_id TEXT NOT NULL, \
         title TEXT NOT NULL, published BOOLEAN NOT NULL)",
        "INSERT INTO posts (id, author_id, title, published) VALUES \
         (1, 'u-1', 'Hello from MCP', TRUE), \
         (2, 'u-1', 'A draft by u-1', FALSE), \
         (3, 'u-2', 'A draft by u-2', FALSE), \
         (4, 'u-2', 'Published by u-2', TRUE) \
         ON CONFLICT (id) DO NOTHING",
    ] {
        cratestack::sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}
