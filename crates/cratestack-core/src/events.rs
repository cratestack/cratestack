//! Model-event bus: typed `created/updated/deleted` envelopes that
//! procedure handlers can subscribe to.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::error::CoolError;

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

type EventHandler = Arc<dyn Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync>;

#[derive(Clone, Default)]
pub struct CoolEventBus {
    handlers: Arc<RwLock<BTreeMap<String, Vec<EventHandler>>>>,
}

impl std::fmt::Debug for CoolEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let handler_count = self
            .handlers
            .read()
            .map(|handlers| handlers.values().map(Vec::len).sum::<usize>())
            .unwrap_or_default();
        f.debug_struct("CoolEventBus")
            .field("handler_count", &handler_count)
            .finish()
    }
}

impl CoolEventBus {
    pub fn subscribe<F>(&self, model: &'static str, operation: ModelEventKind, handler: F)
    where
        F: Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync + 'static,
    {
        let mut handlers = self
            .handlers
            .write()
            .expect("event bus handler registry should not be poisoned");
        handlers
            .entry(event_topic(model, operation))
            .or_default()
            .push(Arc::new(handler));
    }

    pub async fn emit(&self, envelope: CoolEventEnvelope) -> Result<(), CoolError> {
        let handlers = self
            .handlers
            .read()
            .expect("event bus handler registry should not be poisoned")
            .get(&event_topic(&envelope.model, envelope.operation))
            .cloned()
            .unwrap_or_default();

        for handler in handlers {
            handler(envelope.clone()).await?;
        }

        Ok(())
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
