//! Reads whole frames off one stream: chunks in, frames out, with the
//! frame-size limit enforced while a frame is still incomplete.

use std::collections::VecDeque;

use cratestack_core::CratestackError;
use serde::de::DeserializeOwned;

use crate::codec::{FrameDecoder, WireCodec};
use crate::transport::RecvHalf;

const READ_CHUNK_BYTES: usize = 16 * 1024;

pub(crate) struct FrameReader {
    recv: RecvHalf,
    codec: WireCodec,
    decoder: FrameDecoder,
    ready: VecDeque<Vec<u8>>,
}

/// Why a stream yielded no frame.
#[derive(Debug)]
pub(crate) enum ReadFailure {
    /// The stream ended cleanly with no partial frame.
    Ended,
    /// The peer reset the stream or the session went away.
    Gone,
    /// The bytes were not a valid frame, or a frame was too large, or the
    /// stream ended mid-frame.
    Protocol(CratestackError),
}

impl FrameReader {
    pub(crate) fn new(recv: RecvHalf, codec: WireCodec, max_frame_bytes: usize) -> Self {
        Self {
            recv,
            codec,
            decoder: FrameDecoder::new(codec, max_frame_bytes),
            ready: VecDeque::new(),
        }
    }

    /// The next frame, decoded as `T`. Cancel-safe: the only await is the
    /// chunk read, and a chunk's frames are queued before the next await.
    pub(crate) async fn next<T: DeserializeOwned>(&mut self) -> Result<T, ReadFailure> {
        loop {
            if let Some(bytes) = self.ready.pop_front() {
                return self.codec.decode(&bytes).map_err(ReadFailure::Protocol);
            }
            match self.recv.read_chunk(READ_CHUNK_BYTES).await {
                Ok(Some(chunk)) => {
                    let frames = self.decoder.feed(&chunk).map_err(ReadFailure::Protocol)?;
                    self.ready.extend(frames);
                }
                Ok(None) if self.decoder.pending_len() == 0 => return Err(ReadFailure::Ended),
                Ok(None) => {
                    return Err(ReadFailure::Protocol(CratestackError::BadRequest(
                        "WebTransport stream ended mid-frame".to_owned(),
                    )));
                }
                Err(_) => return Err(ReadFailure::Gone),
            }
        }
    }

    /// Discards whatever is left until the peer's FIN, a reset, or
    /// `grace` passes — whichever comes first.
    pub(crate) async fn drain(&mut self, grace: std::time::Duration) {
        let _ = tokio::time::timeout(grace, async {
            while let Ok(Some(_)) = self.recv.read_chunk(READ_CHUNK_BYTES).await {}
        })
        .await;
    }

    /// Tell the peer to stop sending on this stream.
    pub(crate) fn stop(&mut self, code: u32) {
        self.recv.stop(code);
    }
}
