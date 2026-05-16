// -----------------------------------------------------------------------------
// Typed batch surface
//
// Lets callers compose heterogeneous batches of typed RPC ops into a
// single `POST /rpc/batch` round-trip without dropping to the raw
// `RpcRequest` / `RpcResponseFrame` wire types. Typical use through
// the macro-generated client:
//
//   let mut batch = client.batch();
//   let h_widgets = client.widgets().list(&list_input).queue(&mut batch);
//   let h_ping    = client.procedures().ping(&args).queue(&mut batch);
//   let h_created = client.widgets().create(&new).queue(&mut batch);
//
//   let mut results = batch.send().await?;            // one HTTP call
//   let widgets:  Vec<Widget> = results.take(h_widgets)?;
//   let echoed:   PingArgs    = results.take(h_ping)?;
//   let created:  Widget      = results.take(h_created)?;
//
// `BatchableCall<C, O>` is what every macro-generated unary RPC method
// now returns. It implements `IntoFuture`, so `.await` on it fires the
// call immediately exactly like before — `.queue(&mut batch)` is the
// opt-in deferral path. No `_batched` API duplication; same method,
// two consumption modes.
//
// Sequence-streaming methods (`call_streaming` under the hood) stay as
// `async fn -> Result<RpcStream<O>, _>` and do NOT participate in
// batches — `/rpc/batch` is unary by construction.
// -----------------------------------------------------------------------------

use cratestack_core::CoolError;

use crate::codec::HttpClientCodec;
use crate::rpc::batch_call::BatchHandle;
use crate::rpc::client::RpcClient;
use crate::rpc::error::{RpcClientError, RpcRemoteError, http_status_for_rpc_code};

/// Accumulates queued [`BatchableCall`]s into a single
/// `POST /rpc/batch` round-trip. Build via [`RpcClient::batch_builder`]
/// or the macro-generated `Client::batch()`.
///
/// Send-on-drop is intentionally *not* implemented — the batch only
/// fires when you call `.send().await`. Drops without sending are
/// silent (queued calls just discarded).
#[must_use = "BatchBuilder does nothing until `.send().await`ed"]
pub struct BatchBuilder<C> {
    rpc: RpcClient<C>,
    frames: Vec<cratestack_core::rpc::RpcRequest>,
    /// Frames whose input failed to encode pre-send — recorded by id
    /// so [`BatchResults::take`] can surface the error per-handle
    /// instead of poisoning the whole batch.
    encode_errors: std::collections::HashMap<u64, CoolError>,
    next_id: u64,
}

impl<C> BatchBuilder<C>
where
    C: HttpClientCodec + Clone + Send + 'static,
{
    pub(crate) fn new(rpc: RpcClient<C>) -> Self {
        Self {
            rpc,
            frames: Vec::new(),
            encode_errors: std::collections::HashMap::new(),
            next_id: 0,
        }
    }

    /// Number of queued frames (including ones whose input failed
    /// to encode — those still consume an id and will surface their
    /// error on the matching `take`).
    pub fn len(&self) -> usize {
        self.frames.len() + self.encode_errors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub(crate) fn push_frame(&mut self, op_id: String, input: serde_json::Value) -> u64 {
        let id = self.next_id();
        self.frames.push(cratestack_core::rpc::RpcRequest {
            id,
            op: op_id,
            input,
            idem: None,
        });
        id
    }

    pub(crate) fn push_failed_frame(&mut self, error: CoolError) -> u64 {
        let id = self.next_id();
        self.encode_errors.insert(id, error);
        id
    }

    /// Fire the batch as a single `POST /rpc/batch` and return a
    /// [`BatchResults`] keyed by handle for per-op output extraction.
    ///
    /// The outer `Result` only fails on transport / batch-envelope
    /// errors (the whole batch couldn't be sent or the response
    /// couldn't be parsed). Per-frame failures — both pre-send input
    /// encoding errors and server-side `RpcErrorBody` — are deferred
    /// to the matching `BatchResults::take(handle)?` call.
    pub async fn send(self) -> Result<BatchResults, RpcClientError> {
        let encode_errors = self.encode_errors;
        let frames = if self.frames.is_empty() {
            std::collections::HashMap::new()
        } else {
            let response_frames = self.rpc.batch(&self.frames).await?;
            response_frames.into_iter().map(|f| (f.id, f)).collect()
        };
        Ok(BatchResults {
            frames,
            encode_errors,
        })
    }
}

/// Per-handle results from a sent batch. Each handle can be `take`n
/// exactly once.
pub struct BatchResults {
    frames: std::collections::HashMap<u64, cratestack_core::rpc::RpcResponseFrame>,
    encode_errors: std::collections::HashMap<u64, CoolError>,
}

impl BatchResults {
    /// Extract the typed output for one queued op. Returns:
    ///
    /// - `Ok(output)` — the server emitted an `output` for this frame
    ///   and it decoded into `O`.
    /// - `Err(RpcClientError::Codec(_))` — the input failed to encode
    ///   before send, or the output failed to decode.
    /// - `Err(RpcClientError::Remote(RpcRemoteError { body, .. }))` —
    ///   the server emitted an `error` frame for this op. The
    ///   `status` field is derived from the gRPC-style code in the
    ///   body since `/rpc/batch` returns 200 at the HTTP level
    ///   regardless of per-frame outcomes.
    /// - `Err(RpcClientError::InvalidResponse(_))` — the server
    ///   omitted this frame entirely or the frame had neither
    ///   `output` nor `error` set.
    pub fn take<O>(&mut self, handle: BatchHandle<O>) -> Result<O, RpcClientError>
    where
        O: serde::de::DeserializeOwned,
    {
        if let Some(error) = self.encode_errors.remove(&handle.id) {
            return Err(RpcClientError::Codec(error));
        }
        let frame = self.frames.remove(&handle.id).ok_or_else(|| {
            RpcClientError::InvalidResponse(format!(
                "batch response missing frame for id {}",
                handle.id,
            ))
        })?;
        match (frame.output, frame.error) {
            (Some(output), None) => serde_json::from_value::<O>(output).map_err(|error| {
                RpcClientError::Codec(CoolError::Codec(format!(
                    "decode batch output for id {}: {error}",
                    handle.id,
                )))
            }),
            (None, Some(body)) => Err(RpcClientError::Remote(RpcRemoteError {
                status: http_status_for_rpc_code(&body.code),
                body,
            })),
            (Some(_), Some(_)) | (None, None) => Err(RpcClientError::InvalidResponse(format!(
                "batch frame {} has both `output` and `error` set, or neither",
                handle.id,
            ))),
        }
    }
}
