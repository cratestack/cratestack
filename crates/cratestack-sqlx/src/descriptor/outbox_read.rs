//! Reading one outbox row back as the envelope JSON a model event travels
//! as, for a backplane listener whose notification referred to the row
//! instead of carrying it (an event over Postgres' `NOTIFY` size limit).

use cratestack_core::CratestackError;

use super::event_outbox::EventOutboxRow;
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;

/// One outbox row as the envelope JSON a model event travels as: what a
/// backplane listener reads when a notification referred to the row instead
/// of carrying it (an event over the `NOTIFY` size limit).
pub(crate) async fn outbox_envelope_json(
    pool: &sqlx::PgPool,
    event_id: uuid::Uuid,
) -> Result<String, CratestackError> {
    let row = sqlx::query_as::<_, EventOutboxRow>(
        "SELECT event_id, model, operation, occurred_at, payload, attempts, last_error \
         FROM cratestack_event_outbox WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .map_err(cratestack_error_from_sqlx)?;
    serde_json::to_string(&row.try_into_envelope()?).map_err(|error| {
        CratestackError::Codec(format!("failed to encode event envelope: {error}"))
    })
}
