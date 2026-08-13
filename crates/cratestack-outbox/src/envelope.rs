use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Outbox row as returned to drain callers. `id` is a UUIDv7 minted by
/// [`crate::OutboxClient::persist`]/[`crate::OutboxClient::persist_in_tx`] —
/// lexically sortable, so a snapshotter can store it as an opaque cursor
/// string and pass it back as [`crate::DrainRequest::after_id`] on the next
/// drain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Option<String>,
}

/// Input to [`crate::OutboxClient::persist`] /
/// [`crate::OutboxClient::persist_in_tx`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub correlation_id: Option<String>,
}

impl NewEvent {
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        event_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            event_type: event_type.into(),
            payload,
            correlation_id: None,
        }
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }
}
