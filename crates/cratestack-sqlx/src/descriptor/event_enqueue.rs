//! The enqueue helpers each write path calls in-transaction: one
//! `cratestack_event_outbox` row, plus the subscription backplane's
//! notification. Split from [`super::event_outbox`] (the row shape and the
//! table) to keep both under the ~200-line-per-file convention.

use cratestack_core::{
    CratestackError, CratestackEventEnvelope, ModelEventKind, model_event_channel,
};

use super::OutboxColumn;
use crate::backplane::notify_payload;
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;

/// Inserts one `cratestack_event_outbox` row. `pub` for the same reason
/// as [`ensure_event_outbox_table`] — see its doc comment.
///
/// `model` takes `&str` rather than the `&'static str` every in-crate
/// caller happens to pass (generated code's model names are always
/// `&'static str` literals): `cratestack-studio` parses `.cstack`
/// schemas at runtime, so its model names are owned `String`s with no
/// `'static` lifetime available. The function only ever borrows `model`
/// long enough to bind it as a query parameter, so relaxing the bound
/// costs nothing for the existing callers and is required for the new
/// one.
pub async fn enqueue_event_outbox<'e, E, T>(
    executor: E,
    model: &str,
    operation: ModelEventKind,
    data: &T,
) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    T: serde::Serialize,
{
    // A caller outside the generated write paths (Studio) cannot know whether
    // the model is `@@subscribe`, so it always notifies.
    enqueue(executor, model, true, operation, data, None).await
}

/// The generated write paths' enqueue: notifies subscribers only for an
/// `@@subscribe` model (`target.notify_subscribers`).
pub(crate) async fn enqueue_model_event<'e, E, T>(
    executor: E,
    target: cratestack_sql::EventTarget,
    operation: ModelEventKind,
    data: &T,
) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    T: serde::Serialize,
{
    enqueue(
        executor,
        target.model,
        target.notify_subscribers,
        operation,
        data,
        None,
    )
    .await
}

/// [`enqueue_model_event`] for an update, carrying the row as it was before
/// the write when `column` says the outbox can store it
/// ([`crate::SqlxRuntime::keeps_before`]): a subscriber's filter-exit event
/// needs it, `docs/design/subscriptions-websocket.md` §5.3.
pub(crate) async fn enqueue_model_update<'e, E, T>(
    executor: E,
    target: cratestack_sql::EventTarget,
    data: &T,
    before: Option<&T>,
    column: Option<&OutboxColumn>,
) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    T: serde::Serialize,
{
    let before = before.filter(|_| column.is_some());
    let result = enqueue(
        executor,
        target.model,
        target.notify_subscribers,
        ModelEventKind::Updated,
        data,
        before,
    )
    .await;
    // An undefined column: what said the table had it was stale.
    if let (Some(column), Err(error)) = (column, &result)
        && before.is_some()
        && error.db_sqlstate() == Some("42703")
    {
        column.forget();
    }
    result
}

/// The row, and the backplane notification, in one statement: Postgres
/// delivers the notification only if the transaction commits.
const INSERT: &str = "WITH inserted AS (\
        INSERT INTO cratestack_event_outbox (event_id, model, operation, occurred_at, payload) \
        VALUES ($1, $2, $3, $4, $5) RETURNING event_id\
     ) SELECT pg_notify($6, $7) FROM inserted WHERE $7 <> ''";
const INSERT_WITH_BEFORE: &str = "WITH inserted AS (\
        INSERT INTO cratestack_event_outbox \
            (event_id, model, operation, occurred_at, payload, payload_before) \
        VALUES ($1, $2, $3, $4, $5, $8) RETURNING event_id\
     ) SELECT pg_notify($6, $7) FROM inserted WHERE $7 <> ''";

async fn enqueue<'e, E, T>(
    executor: E,
    model: &str,
    notify: bool,
    operation: ModelEventKind,
    data: &T,
    before: Option<&T>,
) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    T: serde::Serialize,
{
    let encode = |value: &T| {
        serde_json::to_value(value).map_err(|error| {
            CratestackError::Codec(format!("failed to encode event payload: {error}"))
        })
    };
    let payload = encode(data)?;
    let envelope = CratestackEventEnvelope {
        event_id: uuid::Uuid::new_v4(),
        model: model.to_owned(),
        operation,
        occurred_at: chrono::Utc::now(),
        data: payload,
        before: before.map(encode).transpose()?,
    };
    // The subscription backplane's notification, sent by this same statement
    // so Postgres delivers it only if the transaction commits
    // (docs/design/subscriptions-websocket.md §4.3). Nobody listening costs
    // nothing; a `PgNotifyBackplane` listener on any replica receives it.
    let envelope_json = serde_json::to_string(&envelope).map_err(|error| {
        CratestackError::Codec(format!("failed to encode event envelope: {error}"))
    })?;
    let notification = if notify {
        notify_payload::encode(
            &model_event_channel(model),
            &envelope_json,
            Some(envelope.event_id),
        )
        .unwrap_or_default()
    } else {
        String::new()
    };
    // Without a pre-image the statement leaves `payload_before` out, so an
    // outbox table not yet upgraded still takes every other event.
    let query = sqlx::query(if envelope.before.is_some() {
        INSERT_WITH_BEFORE
    } else {
        INSERT
    })
    .bind(envelope.event_id)
    .bind(model)
    .bind(operation.as_str())
    .bind(envelope.occurred_at)
    .bind(envelope.data)
    .bind(notify_payload::NOTIFY_CHANNEL)
    .bind(notification);
    let query = match envelope.before {
        Some(before) => query.bind(before),
        None => query,
    };
    query
        .execute(executor)
        .await
        .map_err(cratestack_error_from_sqlx)?;

    Ok(())
}
