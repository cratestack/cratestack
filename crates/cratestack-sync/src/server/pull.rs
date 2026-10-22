//! Pull: changes after a cursor, in commit-safe order.
//!
//! The visibility gate is `txid < pg_snapshot_xmin(pg_current_snapshot())`:
//! only changes written by transactions older than every transaction still
//! in flight are handed out. Without it, a transaction that allocated an
//! earlier position but committed later would land *behind* a cursor a
//! client already holds, and that client would never see it. Transaction ids
//! and snapshots are *cluster-wide*, so a long-running transaction anywhere
//! on the Postgres server — any database, not just this one — delays (never
//! loses) sync until it ends. That is the trade the design doc accepts in
//! §4, with the `idle_in_transaction_session_timeout` mitigation.
//!
//! A page never splits a transaction: it is extended to the end of the
//! last transaction it touches, so a client never applies half of one.

use std::collections::{BTreeMap, HashMap};

use cratestack_core::CratestackContext;
use cratestack_sqlx::sqlx;
use serde_json::Value;

use super::clients::authorize;
use super::{SyncError, SyncServer, internal};
use crate::protocol::{
    ACK_MODEL, Change, ChangeOp, Cursor, PullRequest, PullResponse, SyncErrorCode, client_scope,
};

const PAGE_SQL: &str = r#"
WITH bounded AS (
    SELECT txid FROM cratestack_sync_changes
    WHERE scope = ANY($1)
      AND (txid, seq) > ($2::text::xid8, $3)
      AND txid < pg_snapshot_xmin(pg_current_snapshot())
    ORDER BY txid, seq
    LIMIT $4
)
SELECT model, pk, op, inline_row, txid::text, seq
FROM cratestack_sync_changes
WHERE scope = ANY($1)
  AND (txid, seq) > ($2::text::xid8, $3)
  AND txid <= (SELECT max(txid) FROM bounded)
ORDER BY txid, seq
"#;

type ChangeRow = (String, String, String, Option<Value>, String, i64);

impl SyncServer {
    pub async fn pull(
        &self,
        ctx: &CratestackContext,
        request: PullRequest,
    ) -> Result<PullResponse, SyncError> {
        let principal = self.principal(ctx)?;
        self.admit("pull", &principal.id).await?;
        let limit = request
            .limit
            .unwrap_or(self.policy.max_pull_changes)
            .clamp(1, self.policy.max_pull_changes);

        // REPEATABLE READ: the horizon check, the page, and the has-more
        // probe all read one snapshot.
        let mut tx = self.pool.begin().await.map_err(internal)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        authorize(&mut tx, &principal, request.client_id, false).await?;
        self.check_horizon(&mut tx, request.cursor).await?;

        let mut scopes = principal.scopes;
        scopes.push(client_scope(request.client_id));
        let rows: Vec<ChangeRow> = sqlx::query_as(PAGE_SQL)
            .bind(&scopes)
            .bind(request.cursor.txid.to_string())
            .bind(request.cursor.seq)
            .bind(i64::from(limit))
            .fetch_all(&mut *tx)
            .await
            .map_err(internal)?;
        let cursor = match rows.last() {
            Some((_, _, _, _, txid, seq)) => Cursor::new(txid.parse().map_err(internal)?, *seq),
            None => request.cursor,
        };
        let has_more: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM cratestack_sync_changes WHERE scope = ANY($1) \
             AND (txid, seq) > ($2::text::xid8, $3) \
             AND txid < pg_snapshot_xmin(pg_current_snapshot()))",
        )
        .bind(&scopes)
        .bind(cursor.txid.to_string())
        .bind(cursor.seq)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        let changes = self.materialize(ctx, rows).await?;
        Ok(PullResponse {
            changes,
            cursor,
            has_more,
        })
    }

    async fn check_horizon(
        &self,
        conn: &mut sqlx::PgConnection,
        cursor: Cursor,
    ) -> Result<(), SyncError> {
        if cursor.is_zero() {
            // A fresh client has nothing a purged tombstone could delete.
            return Ok(());
        }
        let (txid, seq): (String, i64) =
            sqlx::query_as("SELECT txid::text, seq FROM cratestack_sync_horizon")
                .fetch_one(&mut *conn)
                .await
                .map_err(internal)?;
        let horizon = Cursor::new(txid.parse().map_err(internal)?, seq);
        if cursor < horizon {
            return Err(SyncError::new(
                SyncErrorCode::CursorExpired,
                "cursor predates the tombstone TTL; resync from zero",
            ));
        }
        Ok(())
    }

    /// Turn change keys into rows the puller may read, preserving order.
    async fn materialize(
        &self,
        ctx: &CratestackContext,
        rows: Vec<ChangeRow>,
    ) -> Result<Vec<Change>, SyncError> {
        let mut wanted: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for (model, pk, op, ..) in &rows {
            if op == "upsert" && model != ACK_MODEL {
                wanted.entry(model.as_str()).or_default().push(pk.clone());
            }
        }
        let mut fetched: HashMap<String, HashMap<String, Value>> = HashMap::new();
        for (model, pks) in wanted {
            // A model with no fetcher is not synced to clients at all.
            let Some(fetcher) = self.registry.fetchers.get(model) else {
                continue;
            };
            let found = fetcher.fetch(ctx, &pks).await.map_err(internal)?;
            fetched.insert(model.to_owned(), found);
        }

        let mut changes = Vec::with_capacity(rows.len());
        for (model, pk, op, inline_row, ..) in rows {
            let op = if model == ACK_MODEL {
                ChangeOp::Upsert {
                    row: inline_row.unwrap_or(Value::Null),
                }
            } else if !self.registry.fetchers.contains_key(&model) {
                continue;
            } else {
                // `get`, not `remove`: one key can appear twice in a page
                // (tombstone in the scope it left, upsert in the one it joined).
                match (op.as_str(), fetched.get(&model).and_then(|m| m.get(&pk))) {
                    ("upsert", Some(row)) => ChangeOp::Upsert { row: row.clone() },
                    _ => ChangeOp::Delete,
                }
            };
            changes.push(Change { model, pk, op });
        }
        Ok(changes)
    }
}
