//! The JSON a `NOTIFY` on [`NOTIFY_CHANNEL`] carries: the backplane channel
//! and either the payload itself or, for a model event too large for
//! Postgres' 8000-byte limit, the outbox row's `event_id` to read it from.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// The one Postgres channel every replica `LISTEN`s on; backplane channels
/// are multiplexed inside the payload.
pub(crate) const NOTIFY_CHANNEL: &str = "cratestack_events";

/// Postgres refuses a notification payload of 8000 bytes or more.
pub(crate) const NOTIFY_LIMIT: usize = 7900;

/// Source of [`Notification::nonce`]. Postgres folds identical notifications
/// of one transaction into one; only one process's transactions need telling
/// apart, so a counter is enough.
static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Notification<'a> {
    /// The backplane channel.
    #[serde(rename = "c", borrow)]
    pub(crate) channel: std::borrow::Cow<'a, str>,
    /// The payload, inline.
    #[serde(rename = "p", default, skip_serializing_if = "Option::is_none", borrow)]
    pub(crate) payload: Option<std::borrow::Cow<'a, str>>,
    /// Or the outbox row holding it (a large model event).
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    pub(crate) event_id: Option<uuid::Uuid>,
    /// Makes every notification distinct, so Postgres does not deliver two
    /// identical ones from one transaction once. Readers ignore it.
    #[serde(rename = "n", default, skip_serializing_if = "Option::is_none")]
    pub(crate) nonce: Option<u64>,
}

/// The notification text for `payload` on `channel`, falling back to a
/// reference to `event_id` when inline would exceed the limit. `None` when
/// it is too large and there is no row to refer to.
pub(crate) fn encode(channel: &str, payload: &str, event_id: Option<uuid::Uuid>) -> Option<String> {
    let nonce = Some(NONCE.fetch_add(1, Ordering::Relaxed));
    let inline = serde_json::to_string(&Notification {
        channel: channel.into(),
        payload: Some(payload.into()),
        event_id: None,
        nonce,
    })
    .ok()?;
    if inline.len() <= NOTIFY_LIMIT {
        return Some(inline);
    }
    let event_id = event_id?;
    serde_json::to_string(&Notification {
        channel: channel.into(),
        payload: None,
        event_id: Some(event_id),
        nonce,
    })
    .ok()
}
