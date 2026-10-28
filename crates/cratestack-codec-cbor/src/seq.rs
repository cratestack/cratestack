//! Boundary scanner for CBOR sequences (RFC 8742): top-level CBOR items
//! written back to back with no outer framing. HTTP streaming responses
//! (`application/cbor-seq`) and WebTransport subscription streams both
//! carry this shape, which is why the scanner lives here and not in a
//! client or server crate.

use cratestack_core::CratestackError;

/// Stateful boundary scanner for `application/cbor-seq` streams. Bytes
/// arrive in arbitrary chunks; this type buffers them and emits the
/// byte ranges of any complete top-level CBOR items observed so far.
/// The CBOR-level parse uses `minicbor::Decoder::skip` for boundary
/// detection (cheap, doesn't allocate); the per-item serde decode
/// happens at the caller's leisure on each returned slice.
///
/// Exposed publicly so callers that read the bytes themselves — e.g. apps
/// that run the HTTP request themselves (dio in Flutter, `fetch` in Wasm,
/// platform networking on iOS/Android) — can reuse the
/// boundary-detection logic without re-implementing it.
pub struct CborSeqChunkDecoder {
    buffer: Vec<u8>,
}

impl CborSeqChunkDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Append `chunk` to the internal buffer and return the bytes of
    /// every complete top-level CBOR item now in it. Drains those bytes
    /// from the buffer; any trailing bytes that don't yet form a
    /// complete item stay buffered for the next call.
    pub fn feed_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, CratestackError> {
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
                        return Err(CratestackError::Codec(
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
                    return Err(CratestackError::Codec(format!(
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
