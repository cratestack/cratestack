//! Drain request/response shapes. The query itself lives on
//! [`crate::OutboxClient::drain`]; the cursor is a single opaque string
//! (the last-returned row's UUIDv7 `id`), so a snapshotter persists it as
//! plain text with no cratestack-specific decoding.

use serde::{Deserialize, Serialize};

use crate::envelope::EventEnvelope;

const DEFAULT_MAX: i64 = 500;
pub(crate) const HARD_MAX: i64 = 5_000;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DrainRequest {
    /// Return events whose `id` is strictly greater than this cursor.
    /// `None` starts from the beginning.
    pub after_id: Option<String>,
    /// Upper bound on rows returned. Clamped to an internal hard ceiling
    /// (5,000 rows) regardless of what the caller asks for.
    pub max: i64,
}

impl Default for DrainRequest {
    fn default() -> Self {
        Self {
            after_id: None,
            max: DEFAULT_MAX,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrainResponse {
    pub events: Vec<EventEnvelope>,
    /// The highest `id` returned, or `None` when the batch was empty.
    /// Callers should pass this as `after_id` on their next drain.
    pub next_cursor: Option<String>,
}
