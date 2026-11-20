//! Telling a session's open subscriptions why it ended, before it closes
//! (cratestack#1381). Each subscription is its own stream with its own
//! writer in its own task, so the session cannot write to it: it announces
//! the end on a channel every stream task watches, and waits (briefly) for
//! them to put their `err` frame on the wire. Closing first would race that
//! frame, and a client reading per stream would see only the close.

use std::time::Duration;

use cratestack_core::StreamEndNotice;
use tokio::sync::{mpsc, watch};

/// How long the session waits for its streams to write their terminal frame.
const STREAM_END_GRACE: Duration = Duration::from_secs(1);

/// The session's side: announces the end, then waits for the streams.
pub(super) struct Ending {
    announce: watch::Sender<Option<StreamEndNotice>>,
    live: mpsc::Receiver<()>,
    handle: Handle,
}

/// A stream task's side. Holding it marks the stream live; dropping it, once
/// the stream's terminal frame is written, marks it finished.
#[derive(Clone)]
pub(crate) struct Handle {
    ended: watch::Receiver<Option<StreamEndNotice>>,
    _live: mpsc::Sender<()>,
}

impl Ending {
    pub(super) fn new() -> Self {
        let (announce, ended) = watch::channel(None);
        let (live_tx, live) = mpsc::channel(1);
        let handle = Handle {
            ended,
            _live: live_tx,
        };
        Self {
            announce,
            live,
            handle,
        }
    }

    /// A handle for one new stream task.
    pub(super) fn handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Announces `end` to every stream, then waits for them to finish or the
    /// grace to lapse. Consumes `self`: a session ends once.
    pub(super) async fn end_streams(self, end: &StreamEndNotice) {
        let Self {
            announce,
            mut live,
            handle,
        } = self;
        drop(handle);
        announce.send_replace(Some(end.clone()));
        // Every sender dropped (all streams finished) closes the channel.
        let _ = tokio::time::timeout(STREAM_END_GRACE, live.recv()).await;
    }
}

impl Handle {
    /// Resolves with the end the session announced; pending until then.
    pub(crate) async fn session_ended(&mut self) -> StreamEndNotice {
        // Copied out before the next await: the guard is not `Send`.
        let end = self
            .ended
            .wait_for(Option::is_some)
            .await
            .ok()
            .and_then(|end| end.clone());
        match end {
            Some(end) => end,
            // The session is gone without announcing: nothing to say.
            None => std::future::pending().await,
        }
    }
}
