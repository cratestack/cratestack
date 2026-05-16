//! Boundary scanner for `application/cbor-seq` streams.

use std::sync::Mutex;

use cratestack_client_rust::{CborSeqChunkDecoder, RuntimeErrorCode};

use crate::types::FlutterRuntimeError;

/// Stateful boundary scanner for `application/cbor-seq` streams,
/// exposed for Flutter apps that prefer to run the HTTP request
/// themselves (via `dio`, `http`, or platform-native networking) and
/// only delegate the hard part — frame boundary detection across
/// arbitrary chunk sizes — to Rust.
///
/// Typical use with `dio`:
///
/// ```text
/// final decoder = FlutterCborSeqDecoder();
/// final response = await dio.post<ResponseBody>(
///   '/rpc/$opId',
///   data: input,
///   options: Options(
///     responseType: ResponseType.stream,
///     headers: {'Accept': 'application/cbor-seq', 'Content-Type': 'application/cbor'},
///   ),
/// );
/// await for (final chunk in response.data!.stream) {
///   final items = await decoder.feed(Uint8List.fromList(chunk));
///   for (final item in items) {
///     controller.add(cbor.decode(item)); // pure-Dart per-item decode
///   }
/// }
/// if (decoder.pendingLen() > 0) {
///   controller.addError('truncated final cbor-seq frame');
/// }
/// ```
///
/// This is strictly a *decode* helper — it does no I/O. The HTTP
/// request, cancellation, retry, and interceptor concerns live with
/// the Dart-side HTTP client. The boundary-detection logic stays in
/// Rust because it's where `minicbor::Decoder::skip` already lives.
///
/// For Flutter apps that want HTTP-and-decoding to stay in Rust, use
/// [`crate::FlutterRuntime::execute_streamed`] /
/// [`crate::FlutterRuntime::rpc_call_streamed`] instead — those
/// package the HTTP request, this decoder, and the
/// [`crate::FlutterChunkWire`] callback into one entrypoint.
pub struct FlutterCborSeqDecoder {
    inner: Mutex<CborSeqChunkDecoder>,
}

impl FlutterCborSeqDecoder {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CborSeqChunkDecoder::new()),
        }
    }

    /// Feed one chunk of bytes from the HTTP response body. Returns
    /// the bytes of every complete top-level CBOR item now available.
    /// Any trailing bytes that don't yet form a complete item stay
    /// buffered for the next call.
    ///
    /// Each returned `Vec<u8>` is one CBOR-encoded item — decode it on
    /// the Dart side with any pure-Dart CBOR package.
    pub fn feed(&self, chunk: Vec<u8>) -> Result<Vec<Vec<u8>>, FlutterRuntimeError> {
        let mut guard = self.inner.lock().map_err(|error| FlutterRuntimeError {
            code: RuntimeErrorCode::State as u32,
            http_status: None,
            message: format!("failed to lock cbor-seq decoder: {error}"),
            remote_code: None,
            remote_body: None,
        })?;
        guard.feed_chunk(&chunk).map_err(|error| FlutterRuntimeError {
            code: RuntimeErrorCode::Codec as u32,
            http_status: None,
            message: error.to_string(),
            remote_code: None,
            remote_body: None,
        })
    }

    /// Bytes currently buffered (waiting for frame completion). Call
    /// this once after the upstream stream closes — a non-zero value
    /// means the server hung up mid-item and the consumer should
    /// surface that as a terminal error.
    pub fn pending_len(&self) -> usize {
        self.inner
            .lock()
            .map(|guard| guard.pending_len())
            .unwrap_or(0)
    }
}

impl Default for FlutterCborSeqDecoder {
    fn default() -> Self {
        Self::new()
    }
}
