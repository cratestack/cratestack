//! Model-event bus: typed `created/updated/deleted` envelopes that
//! procedure handlers can subscribe to.

mod bus;

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::CoolError;

pub use bus::{CoolEventBus, SubscriptionGuard, SubscriptionHandle};

pub type CoolEventFuture = Pin<Box<dyn Future<Output = Result<(), CoolError>> + Send + 'static>>;

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

    pub fn parse(value: &str) -> Result<Self, CoolError> {
        match value {
            "created" => Ok(Self::Created),
            "updated" => Ok(Self::Updated),
            "deleted" => Ok(Self::Deleted),
            other => Err(CoolError::Validation(format!(
                "unsupported model event operation `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolEventEnvelope {
    pub event_id: uuid::Uuid,
    pub model: String,
    pub operation: ModelEventKind,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEvent<T> {
    pub event_id: uuid::Uuid,
    pub model: String,
    pub operation: ModelEventKind,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub data: T,
}

impl<T> TryFrom<CoolEventEnvelope> for ModelEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    type Error = CoolError;

    fn try_from(value: CoolEventEnvelope) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: value.event_id,
            model: value.model,
            operation: value.operation,
            occurred_at: value.occurred_at,
            data: serde_json::from_value(value.data).map_err(|error| {
                CoolError::Codec(format!("failed to decode event payload: {error}"))
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
