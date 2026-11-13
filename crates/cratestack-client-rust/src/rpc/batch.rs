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

use cratestack_core::CratestackError;

use crate::codec::HttpClientCodec;
use cratestack_core::rpc::RpcFrameValue;

use crate::rpc::batch_results::BatchResults;
use crate::rpc::client::RpcClient;
use crate::rpc::error::RpcClientError;

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
    encode_errors: std::collections::HashMap<u64, CratestackError>,
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

    pub(crate) fn push_frame(&mut self, op_id: String, input: RpcFrameValue) -> u64 {
        let id = self.next_id();
        self.frames.push(cratestack_core::rpc::RpcRequest {
            id,
            op: op_id,
            input,
            idem: None,
        });
        id
    }

    pub(crate) fn push_failed_frame(&mut self, error: CratestackError) -> u64 {
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
    pub async fn send(self) -> Result<BatchResults<C>, RpcClientError> {
        let encode_errors = self.encode_errors;
        let (content_type, frames) = if self.frames.is_empty() {
            (String::new(), std::collections::HashMap::new())
        } else {
            let (content_type, response_frames) = self.rpc.batch_response(&self.frames).await?;
            (
                content_type,
                response_frames.into_iter().map(|f| (f.id, f)).collect(),
            )
        };
        Ok(BatchResults::new(
            self.rpc.inner.codec.clone(),
            content_type,
            frames,
            encode_errors,
        ))
    }
}
