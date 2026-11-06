//! Model-event bus: typed `created/updated/deleted` envelopes that
//! procedure handlers can subscribe to.

mod bus;

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::CratestackError;

pub use bus::{CratestackEventBus, SubscriptionGuard, SubscriptionHandle};

pub type CratestackEventFuture =
    Pin<Box<dyn Future<Output = Result<(), CratestackError>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelEventKind {
    Created,
    Updated,
    Deleted,
}

impl ModelEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, CratestackError> {
        match value {
            "created" => Ok(Self::Created),
            "updated" => Ok(Self::Updated),
            "deleted" => Ok(Self::Deleted),
            other => Err(CratestackError::Validation(format!(
                "unsupported model event operation `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CratestackEventEnvelope {
    pub event_id: uuid::Uuid,
    pub model: String,
    pub operation: ModelEventKind,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
    /// An `Updated` event's row as it was before the write, set only for an
    /// `@@subscribe` model, so a subscriber can be told the row left its
    /// filter. Server-side handlers see it. It never reaches a [`ModelEvent`]
    /// as such: a client gets it only as the `data` of a filter-exit event,
    /// once the subscriber's own read check has passed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEvent<T> {
    pub event_id: uuid::Uuid,
    pub model: String,
    pub operation: ModelEventKind,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// `false` on a model feed's filter-exit event: the update moved the row
    /// out of the subscriber's filter or read policy, and `data` is the row as
    /// the subscriber last saw it, never the new values
    /// (`docs/design/subscriptions-websocket.md` §5.3).
    /// Left out of the wire form while `true`, so an ordinary event is
    /// byte-identical to one from before filter-exit events.
    #[serde(default = "matches_default", skip_serializing_if = "is_true")]
    pub matches: bool,
    pub data: T,
}

fn matches_default() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

impl<T> TryFrom<CratestackEventEnvelope> for ModelEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    type Error = CratestackError;

    fn try_from(value: CratestackEventEnvelope) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: value.event_id,
            model: value.model,
            operation: value.operation,
            occurred_at: value.occurred_at,
            matches: true,
            data: serde_json::from_value(value.data).map_err(|error| {
                CratestackError::Codec(format!("failed to decode event payload: {error}"))
            })?,
        })
    }
}

pub fn event_topic(model: &str, operation: ModelEventKind) -> String {
    format!("{}.{}", model, operation.as_str())
}

pub fn parse_emit_attribute(raw: &str) -> Result<Vec<ModelEventKind>, String> {
    let Some(inner) = raw
        .strip_prefix("@@emit(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(format!("unsupported event attribute `{raw}`"));
    };

    let mut operations = Vec::new();
    for part in inner
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let operation = match part {
            "created" => ModelEventKind::Created,
            "updated" => ModelEventKind::Updated,
            "deleted" => ModelEventKind::Deleted,
            other => {
                return Err(format!(
                    "unsupported event operation `{other}` in `{raw}`; expected created, updated, or deleted"
                ));
            }
        };
        if !operations.contains(&operation) {
            operations.push(operation);
        }
    }

    if operations.is_empty() {
        return Err(format!(
            "event attribute `{raw}` must declare at least one operation"
        ));
    }

    Ok(operations)
}
