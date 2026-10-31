//! One stream's two halves, with errors flattened to "the stream is gone":
//! the binding never needs more than that, and it keeps the crate's error
//! types out of the rest of this crate.

use web_transport_quinn as wtq;

/// The peer reset the stream, stopped reading, or the session went away.
#[derive(Debug)]
pub(crate) struct StreamGone;

pub(crate) struct SendHalf(wtq::SendStream);

impl SendHalf {
    pub(super) fn new(stream: wtq::SendStream) -> Self {
        Self(stream)
    }

    pub(crate) async fn write_all(&mut self, bytes: &[u8]) -> Result<(), StreamGone> {
        self.0.write_all(bytes).await.map_err(|_| StreamGone)
    }

    pub(crate) fn finish(&mut self) {
        let _ = self.0.finish();
    }

    pub(crate) fn reset(&mut self, code: u32) {
        let _ = self.0.reset(code);
    }

    /// Resolves when the client stops reading (its cancel, design §6a.2)
    /// or the session ends.
    pub(crate) async fn stopped(&self) {
        let _ = self.0.stopped().await;
    }
}

pub(crate) struct RecvHalf(wtq::RecvStream);

impl RecvHalf {
    pub(super) fn new(stream: wtq::RecvStream) -> Self {
        Self(stream)
    }

    /// The next chunk, at most `max` bytes; `Ok(None)` at the clean end of
    /// the stream.
    pub(crate) async fn read_chunk(&mut self, max: usize) -> Result<Option<Vec<u8>>, StreamGone> {
        match self.0.read_chunk(max, true).await {
            Ok(chunk) => Ok(chunk.map(|chunk| chunk.bytes.to_vec())),
            Err(_) => Err(StreamGone),
        }
    }

    pub(crate) fn stop(&mut self, code: u32) {
        let _ = self.0.stop(code);
    }
}
