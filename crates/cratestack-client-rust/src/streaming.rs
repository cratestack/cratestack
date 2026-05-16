// -----------------------------------------------------------------------------
// Chunked cbor-seq decoder + typed streaming response consumer
//
// The buffered path (`decode_cbor_sequence` in `codec`) needs the full
// response body before yielding the first item. On a flaky / metered
// network — typical for mobile clients — that costs time-to-first-byte
// AND memory: a 5 MB streamed list buffers all 5 MB before any item
// reaches the UI.
//
// `CborSeqChunkDecoder` does the boundary detection; the typed pump
// `pump_streamed_response_typed` feeds decoded `T` items into an
// `mpsc::Sender`. The FFI/callback shape lives in
// `streaming_callback.rs`.
// -----------------------------------------------------------------------------

use cratestack_core::CoolError;
use serde::de::DeserializeOwned;

use crate::error::ClientError;

/// Stateful boundary scanner for `application/cbor-seq` streams. Bytes
/// arrive in arbitrary chunks; this type buffers them and emits the
/// byte ranges of any complete top-level CBOR items observed so far.
/// The CBOR-level parse uses `minicbor::Decoder::skip` for boundary
/// detection (cheap, doesn't allocate); the per-item serde decode
/// happens at the caller's leisure on each returned slice.
///
/// Exposed publicly so non-`RuntimeHandle` callers — e.g. apps that
/// run the HTTP request themselves (dio in Flutter, `fetch` in Wasm,
/// platform networking on iOS/Android) — can reuse the
/// boundary-detection logic without re-implementing it.
pub struct CborSeqChunkDecoder {
    buffer: Vec<u8>,
}

impl CborSeqChunkDecoder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
        }
    }

    /// Append `chunk` to the internal buffer and return the bytes of
    /// every complete top-level CBOR item now in it. Drains those bytes
    /// from the buffer; any trailing bytes that don't yet form a
    /// complete item stay buffered for the next call.
    pub fn feed_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, CoolError> {
        self.buffer.extend_from_slice(chunk);
        let mut items: Vec<Vec<u8>> = Vec::new();
        let mut consumed = 0;
        loop {
            let remaining = &self.buffer[consumed..];
            if remaining.is_empty() {
                break;
            }
            let mut decoder = minicbor::decode::Decoder::new(remaining);
            match decoder.skip() {
                Ok(()) => {
                    let item_len = decoder.position();
                    if item_len == 0 {
                        return Err(CoolError::Codec(
                            "cbor-seq decoder made no progress".to_owned(),
                        ));
                    }
                    items.push(remaining[..item_len].to_vec());
                    consumed += item_len;
                }
                Err(error) if error.is_end_of_input() => {
                    // Truncated final item — wait for the next chunk.
                    break;
                }
                Err(error) => {
                    return Err(CoolError::Codec(format!(
                        "cbor-seq decode failed: {error}",
                    )));
                }
            }
        }
        if consumed > 0 {
            self.buffer.drain(..consumed);
        }
        Ok(items)
    }

    /// Bytes currently buffered (waiting for frame completion). After
    /// the upstream stream closes, a non-zero value here indicates a
    /// truncated final frame — the server hung up mid-item.
    pub fn pending_len(&self) -> usize {
        self.buffer.len()
    }
}

impl Default for CborSeqChunkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Pump a reqwest streaming response into an `mpsc::Sender`. Each
/// complete cbor-seq item gets deserialized to `T` and sent through;
/// transport / decode errors become terminal `Err` items.
///
/// Generic over the consumer-facing error type `E`. REST callers pass
/// `std::convert::identity` (so `E = ClientError`); RPC callers pass
/// `client_error_to_rpc` (so `E = RpcClientError`). Keeping a single
/// pump avoids a second forwarding task per stream.
pub(crate) async fn pump_streamed_response_typed<T, E, F>(
    response: reqwest::Response,
    tx: tokio::sync::mpsc::Sender<Result<T, E>>,
    convert_error: F,
) where
    T: DeserializeOwned + Send + 'static,
    E: Send + 'static,
    F: Fn(ClientError) -> E + Send + 'static,
{
    use futures_util::StreamExt;

    let mut byte_stream = response.bytes_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(error) => {
                let _ = tx
                    .send(Err(convert_error(ClientError::Transport(error))))
                    .await;
                return;
            }
        };
        let items = match decoder.feed_chunk(&chunk) {
            Ok(items) => items,
            Err(error) => {
                let _ = tx.send(Err(convert_error(ClientError::Codec(error)))).await;
                return;
            }
        };
        for item_bytes in items {
            let decoded: Result<T, E> = minicbor_serde::from_slice(&item_bytes).map_err(|error| {
                convert_error(ClientError::Codec(CoolError::Codec(format!(
                    "decode cbor-seq item: {error}",
                ))))
            });
            if tx.send(decoded).await.is_err() {
                // Receiver dropped — caller cancelled, stop work.
                return;
            }
        }
    }

    if decoder.pending_len() > 0 {
        let _ = tx
            .send(Err(convert_error(ClientError::InvalidResponse(format!(
                "stream ended with {} bytes buffered (incomplete final item)",
                decoder.pending_len(),
            )))))
            .await;
    }
}
