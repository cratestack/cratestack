//! `drain_event_outbox`: deliver undelivered outbox rows to the in-process
//! `on_<model>_<op>` handlers, at least once.
//!
//! Rows are claimed with `FOR UPDATE SKIP LOCKED` inside one transaction, so
//! two replicas draining at once never both deliver a row (design §4.3,
//! slice S2). A handler that writes, and so drains in turn, skips the rows
//! this drain holds.
//!
//! Subscribers do not ride this path when the backplane is notified by the
//! writing transaction (`PgNotifyBackplane`). Any other backplane (in-process,
//! later Redis) gets each event published here, after commit.

use cratestack_core::{CratestackError, model_event_channel};

use super::SqlxRuntime;
use super::event_outbox::{EventOutboxRow, ensure_event_outbox_table};
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;

impl SqlxRuntime {
    #[doc(hidden)]
    pub async fn drain_event_outbox(&self) -> Result<usize, CratestackError> {
        // Inside an `@isolation` procedure the outbox rows are still
        // uncommitted: remember to drain once the attempt commits.
        if let Some(bound) = self.bound() {
            bound.request_drain();
            return Ok(0);
        }
        ensure_event_outbox_table(&self.pool).await?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(cratestack_error_from_sqlx)?;
        let rows = sqlx::query_as::<_, EventOutboxRow>(
            "SELECT event_id, model, operation, occurred_at, payload, attempts, last_error \
             FROM cratestack_event_outbox \
             WHERE delivered_at IS NULL \
             ORDER BY occurred_at ASC, event_id ASC \
             FOR UPDATE SKIP LOCKED",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(cratestack_error_from_sqlx)?;

        let publish = !self.backplane.publishes_model_events_in_tx();
        let mut delivered = 0usize;
        let mut to_publish = Vec::new();
        for row in rows {
            let event_id = row.event_id;
            let first_attempt = row.attempts == 0;
            let envelope = row.try_into_envelope()?;
            if publish
                && first_attempt
                && let Ok(json) = serde_json::to_string(&envelope)
            {
                to_publish.push((model_event_channel(&envelope.model), json));
            }
            match self.events.emit(envelope).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE cratestack_event_outbox \
                         SET delivered_at = NOW(), last_error = NULL, attempts = attempts + 1 \
                         WHERE event_id = $1",
                    )
                    .bind(event_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(cratestack_error_from_sqlx)?;
                    delivered += 1;
                }
                Err(error) => {
                    sqlx::query(
                        "UPDATE cratestack_event_outbox \
                         SET attempts = attempts + 1, last_error = $2 \
                         WHERE event_id = $1",
                    )
                    .bind(event_id)
                    .bind(error.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(cratestack_error_from_sqlx)?;
                }
            }
        }
        tx.commit().await.map_err(cratestack_error_from_sqlx)?;

        // Subscribers: at most once, so a row a handler failed on is not
        // re-announced when its retry comes round.
        for (channel, json) in to_publish {
            if let Err(error) = self.backplane.publish(&channel, &json).await {
                tracing::warn!(target: "cratestack", %error, "model event not published to the backplane");
            }
        }
        Ok(delivered)
    }
}
