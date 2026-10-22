//! A small synced model (`todos`) with raw-SQL handlers, standing in for
//! what generated delegates do in a real app (see `examples/sync-notes`).
//! The fetcher's `owner = $principal` filter plays the model read policy.

use std::collections::HashMap;

use async_trait::async_trait;
use cratestack_core::{CratestackContext, CratestackError, Value as AuthValue};
use cratestack_sqlx::sqlx::{self, PgPool, PgTransaction};
use cratestack_sync::SyncPolicy;
use cratestack_sync::protocol::RejectCode;
use cratestack_sync::server::{
    CaptureSpec, MutationContext, MutationError, MutationHandler, RowFetcher, SyncPrincipal,
    SyncRegistry, SyncServer,
};
use serde_json::{Value, json};

pub const TODOS_DDL: &str = "CREATE TABLE todos (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    title TEXT NOT NULL,
    deleted BOOLEAN NOT NULL DEFAULT FALSE
)";

pub fn ctx(user: &str) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), AuthValue::String(user.to_owned()))])
}

pub fn resolver(ctx: &CratestackContext) -> Option<SyncPrincipal> {
    match ctx.auth_field("id") {
        Some(AuthValue::String(id)) => Some(SyncPrincipal {
            id: id.clone(),
            scopes: vec![format!("user:{id}")],
        }),
        _ => None,
    }
}

struct Todos(PgPool);

#[async_trait]
impl RowFetcher for Todos {
    async fn fetch(
        &self,
        ctx: &CratestackContext,
        pks: &[String],
    ) -> Result<HashMap<String, Value>, CratestackError> {
        let Some(principal) = resolver(ctx) else {
            return Ok(HashMap::new());
        };
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, title FROM todos WHERE id = ANY($1) AND owner = $2 AND NOT deleted",
        )
        .bind(pks)
        .bind(principal.id)
        .fetch_all(&self.0)
        .await
        .map_err(|e| CratestackError::Internal(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(id, title)| (id.clone(), json!({ "id": id, "title": title })))
            .collect())
    }
}

/// `{id, title}` → insert owned by the pusher. Title `boom` inserts and then
/// rejects, to prove the savepoint discards the insert.
struct Create;

#[async_trait]
impl MutationHandler for Create {
    async fn apply(
        &self,
        tx: &mut PgTransaction<'_>,
        mc: &MutationContext<'_>,
        args: Value,
    ) -> Result<(), MutationError> {
        let id = args["id"].as_str().unwrap_or_default();
        let title = args["title"].as_str().unwrap_or_default();
        sqlx::query("INSERT INTO todos (id, owner, title) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(mc.principal)
            .bind(title)
            .execute(&mut **tx)
            .await
            .map_err(|e| MutationError::Fatal(CratestackError::Internal(e.to_string())))?;
        if title == "boom" {
            return Err(MutationError::Reject(RejectCode::Invalid, "boom".into()));
        }
        Ok(())
    }
}

/// `{id, title}` → rename, only if the pusher owns it ("update policy").
struct Rename;

#[async_trait]
impl MutationHandler for Rename {
    async fn apply(
        &self,
        tx: &mut PgTransaction<'_>,
        mc: &MutationContext<'_>,
        args: Value,
    ) -> Result<(), MutationError> {
        let updated = sqlx::query("UPDATE todos SET title = $3 WHERE id = $1 AND owner = $2")
            .bind(args["id"].as_str())
            .bind(mc.principal)
            .bind(args["title"].as_str())
            .execute(&mut **tx)
            .await
            .map_err(|e| MutationError::Fatal(CratestackError::Internal(e.to_string())))?;
        if updated.rows_affected() == 0 {
            return Err(MutationError::Reject(
                RejectCode::Forbidden,
                "not yours".into(),
            ));
        }
        Ok(())
    }
}

pub async fn server(pool: &PgPool, policy: SyncPolicy) -> SyncServer {
    sqlx::raw_sql(TODOS_DDL).execute(pool).await.unwrap();
    let registry = SyncRegistry::new()
        .model("Todo", Todos(pool.clone()))
        .mutation("todo.create", Create)
        .mutation("todo.rename", Rename);
    let server = SyncServer::builder(pool.clone(), registry, resolver)
        .policy(policy)
        .build()
        .unwrap();
    server.install().await.unwrap();
    server
        .capture(CaptureSpec {
            table: "todos",
            model: "Todo",
            pk_column: "id",
            scope_sql: "'user:' || r.owner",
        })
        .await
        .unwrap();
    server
}
