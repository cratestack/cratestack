//! [`RevocationWatch`], one registered stream's side of the registry.

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::watch;

use super::state::{EndSender, State};
use crate::error::StreamEndNotice;

/// One live stream (or socket session) registered with
/// [`StreamRevocations::watch`](super::StreamRevocations::watch). Await
/// [`ended`](Self::ended) alongside the stream; dropping the watch
/// deregisters it.
pub struct RevocationWatch {
    id: u64,
    state: Arc<Mutex<State>>,
    // Keeps the channel open until this watch is dropped.
    _sender: EndSender,
    receiver: watch::Receiver<Option<StreamEndNotice>>,
}

impl RevocationWatch {
    pub(super) fn new(id: u64, state: Arc<Mutex<State>>, sender: EndSender) -> Self {
        let receiver = sender.subscribe();
        Self {
            id,
            state,
            _sender: sender,
            receiver,
        }
    }

    /// Resolves with the reason the stream must end, as soon as one of its
    /// keys is revoked (or already was). Cancel-safe.
    pub async fn ended(&mut self) -> StreamEndNotice {
        let end = self
            .receiver
            .wait_for(|end| end.is_some())
            .await
            .ok()
            .and_then(|end| end.clone());
        match end {
            Some(end) => end,
            // The sender lives as long as `self`, so this cannot happen.
            None => std::future::pending().await,
        }
    }

    /// [`ended`](Self::ended) of an optional watch; never resolves for
    /// `None`. For a `select!` arm of a socket session that may have no
    /// registry.
    pub async fn ended_or_pending(watch: &mut Option<Self>) -> StreamEndNotice {
        match watch {
            Some(watch) => watch.ended().await,
            None => std::future::pending().await,
        }
    }

    /// The end, if one has been recorded; never waits.
    pub fn try_ended(&self) -> Option<StreamEndNotice> {
        self.receiver.borrow().clone()
    }
}

impl Drop for RevocationWatch {
    fn drop(&mut self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .deregister(self.id);
    }
}

impl std::fmt::Debug for RevocationWatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RevocationWatch")
            .field("ended", &self.try_ended())
            .finish()
    }
}
