//! Pokes: "something in one of your scopes changed — pull".
//!
//! A poke carries no data, only the fact of a change, so the pull path
//! stays the one place rows (and read policies) flow through, and an
//! offline client loses nothing by missing pokes. The source is Postgres
//! `NOTIFY` (sent by `cratestack_sync_record` at commit), so every server
//! instance pokes its own connected clients whichever instance — or
//! whichever non-sync code path — made the write.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cratestack_sqlx::sqlx::PgPool;
use cratestack_sqlx::sqlx::postgres::PgListener;
use tokio::sync::broadcast;

/// Sent after the listener reconnects: notifications may have been missed,
/// so every stream pokes its client once.
pub(crate) const ALL_SCOPES: &str = "*";
const CHANNEL: &str = "cratestack_sync";

#[derive(Clone)]
pub struct PokeHub {
    sender: broadcast::Sender<String>,
    open_streams: Arc<Mutex<HashMap<String, usize>>>,
}

impl Default for PokeHub {
    fn default() -> Self {
        Self::new()
    }
}

impl PokeHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self {
            sender,
            open_streams: Arc::default(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }

    /// Poke one scope in-process (tests, or a deployment without LISTEN).
    pub fn notify(&self, scope: &str) {
        let _ = self.sender.send(scope.to_owned());
    }

    /// Run forever: `LISTEN cratestack_sync`, rebroadcast each scope.
    pub async fn listen(&self, pool: PgPool) {
        loop {
            match self.listen_once(&pool).await {
                Ok(()) => {}
                Err(error) => tracing::warn!(%error, "sync poke listener dropped; reconnecting"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn listen_once(&self, pool: &PgPool) -> Result<(), cratestack_sqlx::sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen(CHANNEL).await?;
        self.notify(ALL_SCOPES);
        loop {
            let notification = listener.recv().await?;
            self.notify(notification.payload());
        }
    }

    /// Claim one of `principal`'s stream slots; `None` when at `max`.
    pub(crate) fn open_stream(&self, principal: &str, max: usize) -> Option<StreamSlot> {
        let mut open = self.open_streams.lock().ok()?;
        let count = open.entry(principal.to_owned()).or_default();
        if *count >= max {
            return None;
        }
        *count += 1;
        Some(StreamSlot {
            principal: principal.to_owned(),
            open_streams: self.open_streams.clone(),
        })
    }
}

/// Releases its stream slot on drop, however the stream ends.
pub(crate) struct StreamSlot {
    principal: String,
    open_streams: Arc<Mutex<HashMap<String, usize>>>,
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        if let Ok(mut open) = self.open_streams.lock()
            && let Some(count) = open.get_mut(&self.principal)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&self.principal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_slots_are_capped_per_principal_and_released_on_drop() {
        let hub = PokeHub::new();
        let first = hub.open_stream("alice", 2).unwrap();
        let _second = hub.open_stream("alice", 2).unwrap();
        assert!(hub.open_stream("alice", 2).is_none());
        assert!(hub.open_stream("bob", 2).is_some());
        drop(first);
        assert!(hub.open_stream("alice", 2).is_some());
    }
}
