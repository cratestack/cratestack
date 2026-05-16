// -----------------------------------------------------------------------------
// FFI / callback-shaped streaming consumer
//
// Synchronous from the caller's perspective: pass a callback, return
// when the stream is done. The callback receives raw item bytes
// (one CBOR-encoded value per call) so the FFI side can decode using
// whatever native CBOR library it prefers.
// -----------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

use crate::error::ClientError;
use crate::runtime::wire::{RuntimeErrorCode, RuntimeErrorWire};
use crate::streaming::CborSeqChunkDecoder;

/// FFI-shaped chunk delivered to the `execute_streamed` callback.
/// `Item` carries one CBOR-encoded item's raw bytes; `Error` is
/// terminal; `End` is terminal and indicates a clean stream close.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeChunkWire {
    /// One complete cbor-seq item. The bytes are CBOR-encoded — decode
    /// on the FFI side with whatever native library the host has.
    Item(Vec<u8>),
    /// Terminal: the stream ended cleanly. No further chunks follow.
    End,
    /// Terminal: the stream failed mid-flight. No further chunks follow.
    Error(RuntimeErrorWire),
}

/// Drive a streaming response through a callback. Used by
/// `RuntimeHandle::execute_streamed` — the callback returns `false` to
/// cancel the stream early. The function returns once the stream is
/// done (by completion, error, or cancellation).
pub(crate) async fn pump_streamed_response_callback<F>(
    response: reqwest::Response,
    mut on_chunk: F,
) -> Result<(), RuntimeErrorWire>
where
    F: FnMut(RuntimeChunkWire) -> bool,
{
    use futures_util::StreamExt;

    let mut byte_stream = response.bytes_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    loop {
        let chunk_result = byte_stream.next().await;
        let chunk = match chunk_result {
            Some(Ok(c)) => c,
            Some(Err(error)) => {
                let err = RuntimeErrorWire::from(ClientError::Transport(error));
                on_chunk(RuntimeChunkWire::Error(err.clone()));
                return Err(err);
            }
            None => {
                if decoder.pending_len() > 0 {
                    let err = RuntimeErrorWire {
                        code: RuntimeErrorCode::InvalidResponse,
                        http_status: None,
                        message: format!(
                            "stream ended with {} bytes buffered (incomplete final item)",
                            decoder.pending_len(),
                        ),
                        remote_code: None,
                        remote_body: None,
                    };
                    on_chunk(RuntimeChunkWire::Error(err.clone()));
                    return Err(err);
                }
                on_chunk(RuntimeChunkWire::End);
                return Ok(());
            }
        };
        let items = match decoder.feed_chunk(&chunk) {
            Ok(items) => items,
            Err(error) => {
                let err = RuntimeErrorWire::from(ClientError::Codec(error));
                on_chunk(RuntimeChunkWire::Error(err.clone()));
                return Err(err);
            }
        };
        for item_bytes in items {
            if !on_chunk(RuntimeChunkWire::Item(item_bytes)) {
                // Caller cancelled.
                return Ok(());
            }
        }
    }
}
