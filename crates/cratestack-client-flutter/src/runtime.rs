//! The Flutter-facing runtime handle.

use std::sync::Mutex;

use cratestack_client_rust::{RuntimeErrorCode, RuntimeHandle};

use crate::types::{
    FlutterChunkWire, FlutterHeader, FlutterRequest, FlutterResponse, FlutterRuntimeConfig,
    FlutterRuntimeError,
};

pub struct FlutterRuntime {
    inner: Mutex<RuntimeHandle>,
}

impl FlutterRuntime {
    pub fn new(config: FlutterRuntimeConfig) -> Result<Self, FlutterRuntimeError> {
        let handle = RuntimeHandle::new(config.into()).map_err(FlutterRuntimeError::from)?;
        Ok(Self {
            inner: Mutex::new(handle),
        })
    }

    pub fn execute(&self, request: FlutterRequest) -> Result<FlutterResponse, FlutterRuntimeError> {
        let handle = self.inner.lock().map_err(|error| FlutterRuntimeError {
            code: RuntimeErrorCode::State as u32,
            http_status: None,
            message: format!("failed to lock runtime handle: {error}"),
            remote_code: None,
            remote_body: None,
        })?;
        handle
            .execute(request.into())
            .map(FlutterResponse::from)
            .map_err(FlutterRuntimeError::from)
    }

    /// POST /rpc/{op_id} — unary call.
    ///
    /// `op_id` is the dotted dispatch key the server emits (e.g.
    /// `model.User.list`, `procedure.publishPost`). `input_json` is the
    /// JSON-encoded RPC input body. Returns the full `FlutterResponse`
    /// so the Dart side can decode the body against the right type.
    pub fn rpc_call(
        &self,
        op_id: &str,
        input_json: Vec<u8>,
        headers: Vec<FlutterHeader>,
    ) -> Result<FlutterResponse, FlutterRuntimeError> {
        let request = FlutterRequest {
            method: "POST".to_owned(),
            path: format!("/rpc/{}", op_id),
            canonical_query: None,
            headers,
            body: input_json,
        };
        self.execute(request)
    }

    /// POST /rpc/batch — batched call.
    pub fn rpc_batch(
        &self,
        batch_json: Vec<u8>,
        headers: Vec<FlutterHeader>,
    ) -> Result<FlutterResponse, FlutterRuntimeError> {
        let request = FlutterRequest {
            method: "POST".to_owned(),
            path: "/rpc/batch".to_owned(),
            canonical_query: None,
            headers,
            body: batch_json,
        };
        self.execute(request)
    }

    /// Streaming companion to [`Self::rpc_call`]. POSTs to
    /// `/rpc/{op_id}` with `Accept: application/cbor-seq` and delivers
    /// one [`FlutterChunkWire`] per item as bytes arrive on the wire;
    /// returning `false` from the callback cancels the stream.
    ///
    /// `op_id` is the dotted dispatch key the server emits — typically
    /// `model.X.list` for sequence-returning CRUD or `procedure.<name>`
    /// for list-return procedures. `input` is the codec-encoded RPC
    /// input body; decode the per-item bytes on the Dart side against
    /// the `Output` type the op produces.
    ///
    /// Wrap this with a `flutter_rust_bridge` `StreamSink<FlutterChunkWire>`
    /// in the consuming Flutter app — same pattern as
    /// [`Self::execute_streamed`].
    pub fn rpc_call_streamed<F>(
        &self,
        op_id: &str,
        input: Vec<u8>,
        headers: Vec<FlutterHeader>,
        on_chunk: F,
    ) -> Result<(), FlutterRuntimeError>
    where
        F: FnMut(FlutterChunkWire) -> bool + Send,
    {
        let request = FlutterRequest {
            method: "POST".to_owned(),
            path: format!("/rpc/{}", op_id),
            canonical_query: None,
            headers,
            body: input,
        };
        self.execute_streamed(request, on_chunk)
    }

    /// Streaming companion to [`Self::execute`]. The callback receives
    /// one [`FlutterChunkWire`] per complete cbor-seq item as bytes
    /// arrive on the wire; returning `false` cancels the stream.
    /// Returns when the stream terminates (clean end, error, or
    /// cancellation).
    ///
    /// Designed to be wrapped by `flutter_rust_bridge`'s
    /// `StreamSink<FlutterChunkWire>` in the consuming Flutter app —
    /// see the example in `examples/embedded-flutter/native` for the
    /// thin Dart-callable wrapper pattern.
    pub fn execute_streamed<F>(
        &self,
        request: FlutterRequest,
        mut on_chunk: F,
    ) -> Result<(), FlutterRuntimeError>
    where
        F: FnMut(FlutterChunkWire) -> bool + Send,
    {
        let handle = self.inner.lock().map_err(|error| FlutterRuntimeError {
            code: RuntimeErrorCode::State as u32,
            http_status: None,
            message: format!("failed to lock runtime handle: {error}"),
            remote_code: None,
            remote_body: None,
        })?;
        handle
            .execute_streamed(request.into(), move |chunk| {
                on_chunk(FlutterChunkWire::from(chunk))
            })
            .map_err(FlutterRuntimeError::from)
    }
}
