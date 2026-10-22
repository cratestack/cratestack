//! TTL enforcement: purge old tombstones, evict idle clients.
//!
//! Live rows are never purged — the change log is compacted, so it holds
//! one row per synced key and a fresh client pulling from zero needs every
//! one of them. Only tombstones age out, and the newest purged position
//! becomes the *horizon*: a cursor behind it may have missed a delete, so
//! pull answers it with `cursor_expired` and the client resyncs from zero.

use cratestack_sqlx::sqlx;

use super::SyncServer;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcReport {
    pub tombstones_purged: u64,
    pub clients_evicted: u64,
}

const PURGE_TOMBSTONES: &str = r#"
WITH purged AS (
    DELETE FROM cratestack_sync_changes
    WHERE op = 'delete' AND changed_at < now() - make_interval(secs => $1)
    RETURNING txid, seq
), newest AS (
    SELECT txid, seq FROM purged ORDER BY txid DESC, seq DESC LIMIT 1
), moved AS (
    UPDATE cratestack_sync_horizon h SET txid = newest.txid, seq = newest.seq
    FROM newest WHERE (newest.txid, newest.seq) > (h.txid, h.seq)
)
SELECT count(*) FROM purged
"#;

const EVICT_CLIENTS: &str = r#"
WITH evicted AS (
    DELETE FROM cratestack_sync_clients
    WHERE last_seen_at < now() - make_interval(secs => $1)
    RETURNING client_id
), acks AS (
    DELETE FROM cratestack_sync_changes c
    USING evicted e WHERE c.scope = 'client:' || e.client_id::text
)
SELECT count(*) FROM evicted
"#;

impl SyncServer {
    /// One GC pass. Safe to run concurrently with traffic and with itself.
    pub async fn gc(&self) -> Result<GcReport, sqlx::Error> {
        let tombstone_ttl = self.policy.tombstone_ttl_secs as f64;
        let client_ttl = self.policy.client_ttl_secs as f64;
        let mut tx = self.pool.begin().await?;
        let tombstones: i64 = sqlx::query_scalar(PURGE_TOMBSTONES)
            .bind(tombstone_ttl)
            .fetch_one(&mut *tx)
            .await?;
        let clients: i64 = sqlx::query_scalar(EVICT_CLIENTS)
            .bind(client_ttl)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(GcReport {
            tombstones_purged: u64::try_from(tombstones).unwrap_or(0),
            clients_evicted: u64::try_from(clients).unwrap_or(0),
        })
    }

    /// Spawn the `LISTEN` task feeding [`super::PokeHub`] and a GC loop
    /// running every `gc_every`. Both log and retry on failure.
    pub fn spawn_background(&self, gc_every: std::time::Duration) {
        let listener = self.clone();
        tokio::spawn(async move { listener.pokes.listen(&listener.pool).await });
        let gc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(gc_every);
            loop {
                ticker.tick().await;
                match gc.gc().await {
                    Ok(report) if report != GcReport::default() => {
                        tracing::info!(?report, "sync gc");
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "sync gc failed"),
                }
            }
        });
    }
}
