//! The wire form of a revocation and how a received one is applied.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{RevocationKey, StreamRevocations};
use crate::error::StreamEndReason;

/// How far ahead a received `until` that does not fit a `SystemTime` is
/// clamped: the revoke is applied, never skipped, and the tombstone is held
/// for the longest a signed stream may live (`MAX_SIGNED_STREAM_LIFE`, 24 h).
const OVERFLOW_UNTIL: Duration = Duration::from_secs(24 * 60 * 60);

/// The wire form. Every field but `kind` and `key` is read leniently: a
/// payload from a newer replica (an unknown `reason`) or a hostile one (an
/// absurd `until`) must still revoke, because dropping a revoke fails open.
#[derive(Serialize, Deserialize)]
pub(super) struct Message {
    pub(super) kind: String,
    pub(super) key: String,
    /// The snake_case name of a [`StreamEndReason`].
    pub(super) reason: String,
    /// Seconds since the epoch.
    pub(super) until: Option<serde_json::Value>,
}

impl StreamRevocations {
    pub(super) fn apply(&self, payload: &str) {
        let message = match serde_json::from_str::<Message>(payload) {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(%error, "skipping a malformed revocation payload");
                return;
            }
        };
        let Some(key) = RevocationKey::from_parts(&message.kind, &message.key) else {
            tracing::warn!(kind = %message.kind, "skipping a revocation with an unknown key");
            return;
        };
        let reason = StreamEndReason::parse(&message.reason).unwrap_or_else(|| {
            tracing::warn!(reason = %message.reason, "unknown revocation reason, revoking as session_ended");
            StreamEndReason::SessionEnded
        });
        let until = message.until.as_ref().map(|until| self.until_of(until));
        self.revoke(key, reason, until);
    }

    /// The instant a received `until` names. Anything that is not a count of
    /// seconds the clock can hold is clamped, never dropped.
    fn until_of(&self, until: &serde_json::Value) -> SystemTime {
        let now = (self.lock().now)();
        until
            .as_u64()
            .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
            .unwrap_or_else(|| {
                tracing::warn!(%until, "revocation `until` out of range, clamping to 24 hours");
                now.checked_add(OVERFLOW_UNTIL).unwrap_or(now)
            })
    }
}
