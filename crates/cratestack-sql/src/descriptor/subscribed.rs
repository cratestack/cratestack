//! `@@subscribe` on a model descriptor: whether the model's events are
//! notified to the subscription backplane
//! (`docs/design/subscriptions-websocket.md` §4.3).

use super::ModelDescriptor;

/// Where a write path enqueues a model event: the model's name, and whether
/// the event must also be notified to subscribers (`@@subscribe`). A model
/// that only `@@emit`s skips the notification, and with it Postgres' commit
/// lock on `NOTIFY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventTarget {
    pub model: &'static str,
    pub notify_subscribers: bool,
}

impl<M, PK> ModelDescriptor<M, PK> {
    /// Mark the model `@@subscribe`. A separate step from
    /// [`ModelDescriptor::new`] so descriptors built without it keep
    /// compiling unchanged.
    pub const fn with_subscribed(self) -> Self {
        Self {
            subscribed: true,
            ..self
        }
    }

    pub const fn event_target(&self) -> EventTarget {
        EventTarget {
            model: self.schema_name,
            notify_subscribers: self.subscribed,
        }
    }
}
