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

use cratestack_core::CratestackError;
use serde::de::DeserializeOwned;

use crate::codec::{CBOR_SEQUENCE_CONTENT_TYPE, HttpClientCodec, media_type_matches};
use crate::error::ClientError;

/// Re-exported from `cratestack-codec-cbor`, where the scanner moved so
/// server-side byte-stream bindings (WebTransport) share it with this
/// client. The `cratestack_client_rust::CborSeqChunkDecoder` path is
/// unchanged.
pub use cratestack_codec_cbor::CborSeqChunkDecoder;

/// Pump a reqwest streaming response into an `mpsc::Sender`. Each
/// complete cbor-seq item gets deserialized to `T` and sent through;
/// transport / decode errors become terminal `Err` items.
///
/// Generic over the consumer-facing error type `E`. REST callers pass
/// `std::convert::identity` (so `E = ClientError`); RPC callers pass
/// `client_error_to_rpc` (so `E = RpcClientError`). Keeping a single
/// pump avoids a second forwarding task per stream.
///
/// Checks the response's `Content-Type` before treating any bytes as
/// cbor-seq frames (fix for the negotiation bug where a server that
/// picked buffered `application/cbor` instead of `application/cbor-seq`
/// made this function try to frame-decode a bare CBOR map and die on
/// the first chunk with a confusing `decode cbor-seq item: unexpected
/// type array...` error — see `select_response_content_type` in
/// `cratestack-axum`). `application/cbor-seq` streams incrementally as
/// before; any other content type this `codec` can decode is buffered
/// in full and forwarded item-by-item (no incremental first-item
/// latency win, but correct); anything else is a terminal `Err` naming
/// what was received.
pub(crate) async fn pump_streamed_response_typed<C, T, E, F>(
    codec: C,
    response: reqwest::Response,
    tx: tokio::sync::mpsc::Sender<Result<T, E>>,
    convert_error: F,
) where
    C: HttpClientCodec,
    T: DeserializeOwned + Send + 'static,
    E: Send + 'static,
    F: Fn(ClientError) -> E + Send + 'static,
{
    use futures_util::StreamExt;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();

    if !media_type_matches(&content_type, CBOR_SEQUENCE_CONTENT_TYPE) {
        pump_buffered_fallback(codec, response, &content_type, tx, convert_error).await;
        return;
    }

    let mut byte_stream = response.bytes_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(error) => {
                let _ = tx
                    .send(Err(convert_error(ClientError::Transport(error.into()))))
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
                convert_error(ClientError::Codec(CratestackError::Codec(format!(
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

/// Fallback for a "streaming" call that negotiated a non-`cbor-seq`
/// response (e.g. `application/cbor` or `application/json`): buffers the
/// whole body — same as the non-streaming `post_list`/`call` path — then
/// forwards each decoded item through `tx` individually, so the caller's
/// `Receiver`-based API keeps working. If `codec` can't decode
/// `content_type` at all, sends a single terminal `Err` naming both what
/// was received and what the streaming path needs.
async fn pump_buffered_fallback<C, T, E, F>(
    codec: C,
    response: reqwest::Response,
    content_type: &str,
    tx: tokio::sync::mpsc::Sender<Result<T, E>>,
    convert_error: F,
) where
    C: HttpClientCodec,
    T: DeserializeOwned + Send + 'static,
    E: Send + 'static,
    F: Fn(ClientError) -> E + Send + 'static,
{
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(error) => {
            let _ = tx
                .send(Err(convert_error(ClientError::Transport(error.into()))))
                .await;
            return;
        }
    };

    if content_type.is_empty() {
        let _ = tx
            .send(Err(convert_error(ClientError::InvalidResponse(format!(
                "streaming response is missing Content-Type (needed {CBOR_SEQUENCE_CONTENT_TYPE} \
                 or another type the client codec can decode)",
            )))))
            .await;
        return;
    }

    match codec.decode_sequence_response::<T>(content_type, &body) {
        Ok(items) => {
            for item in items {
                if tx.send(Ok(item)).await.is_err() {
                    // Receiver dropped — caller cancelled, stop work.
                    return;
                }
            }
        }
        Err(error) => {
            let _ = tx
                .send(Err(convert_error(ClientError::Codec(
                    CratestackError::Codec(format!(
                        "streaming response had Content-Type {content_type:?}, which the client \
                     couldn't decode as a sequence (needed {CBOR_SEQUENCE_CONTENT_TYPE} or a \
                     decodable buffered type): {error}",
                    )),
                ))))
                .await;
        }
    }
}
