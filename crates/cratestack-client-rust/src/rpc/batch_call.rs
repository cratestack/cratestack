// -----------------------------------------------------------------------------
// `BatchableCall` + `BatchHandle` — the prepared-call / typed-key duo
// that the typed batch surface is built around. Sits alongside
// `rpc::batch::{BatchBuilder, BatchResults}`, which consume them.
// -----------------------------------------------------------------------------

use cratestack_core::CratestackError;
use cratestack_core::rpc::RpcFrameValue;

use crate::codec::HttpClientCodec;
use crate::rpc::batch::BatchBuilder;
use crate::rpc::client::RpcClient;
use crate::rpc::error::RpcClientError;

/// A typed unary RPC call that has been *prepared* but not yet sent.
///
/// Produced by every macro-generated unary RPC method on the typed
/// client (model CRUD + unary procedures). Two consumption modes:
///
/// - **Eager.** `.await` directly — `IntoFuture` desugars to the same
///   HTTP request `RpcClient::call` would have made.
/// - **Batched.** `.queue(&mut batch)` registers the call with a
///   [`BatchBuilder`] for a single multiplexed `POST /rpc/batch`.
///   Returns a typed [`BatchHandle`] for `.take(...)` on the results
///   after `batch.send().await` resolves.
///
/// The input is encoded once, at construction time, by the request codec
/// (cratestack#1312), so the same prepared call can flow into either
/// consumption mode without re-borrowing it. A `serde_json::Value` detour
/// would rewrite `Uuid` and `Bytes` into their JSON shapes (text, integer
/// array) and the server rejects those over CBOR with `400
/// invalid_argument`. The batched mode builds its `RpcRequest` frame from
/// those same bytes as an [`RpcFrameValue`], which keeps a CBOR byte string
/// lossless (cratestack#1352).
///
/// Conversion errors surface lazily — eagerly on `.await`, per-handle on
/// the batch path.
#[must_use = "BatchableCall does nothing until `.await`ed or `.queue(&mut batch)`d"]
pub struct BatchableCall<C, O> {
    rpc: RpcClient<C>,
    op_id: String,
    /// Request-codec bytes of the input, for the eager (`.await`) mode.
    input_body: Result<Vec<u8>, CratestackError>,
    /// `fn() -> O` instead of `O` so `BatchableCall` is `Send` + `Sync`
    /// regardless of whether `O` is — the marker is variance-only.
    _output: std::marker::PhantomData<fn() -> O>,
}

impl<C, O> BatchableCall<C, O>
where
    C: HttpClientCodec + Clone + Send + 'static,
    O: serde::de::DeserializeOwned + Send + 'static,
{
    /// Construct a prepared call. Callers should generally use the
    /// macro-generated typed methods rather than building this by hand.
    pub fn new<I>(rpc: RpcClient<C>, op_id: impl Into<String>, input: &I) -> Self
    where
        I: serde::Serialize,
    {
        // cratestack#677: this used to strip `null` object entries here
        // before handing the value to the codec — a workaround for
        // `serde_json::Value::Null` mis-encoding as the CBOR empty-array
        // marker (`0x80`) instead of CBOR null (`0xf6`). That root cause
        // was fixed in `CborCodec::encode` (cratestack#657, via
        // `serialize_unit_as_null`), which makes the strip both
        // unnecessary and actively harmful: it recursed into nested
        // objects, so an explicit `null` on a `model.<Model>.update`
        // patch — meaning "clear this nullable column" — was
        // indistinguishable from an untouched field by the time it
        // reached the codec, silently dropping the clear. See
        // `crates/cratestack-pg/tests/rpc_batch_explicit_null_clear.rs`
        // for the regression test (fails if the strip is restored).
        let input_body = rpc.inner.codec.encode(input);
        Self {
            rpc,
            op_id: op_id.into(),
            input_body,
            _output: std::marker::PhantomData,
        }
    }

    /// Queue this call into a [`BatchBuilder`] for deferred
    /// execution. The returned [`BatchHandle`] is the key to
    /// retrieve the typed result via [`BatchResults::take`] after
    /// [`BatchBuilder::send`] resolves.
    ///
    /// Input-encoding errors observed at construction time are
    /// preserved per-handle, so a single bad input in a batch
    /// produces a per-handle `take(...)?` error rather than
    /// poisoning the whole batch.
    pub fn queue(self, batch: &mut BatchBuilder<C>) -> BatchHandle<O> {
        // The frame is read back out of the very bytes the eager path sends,
        // so a `Uuid` / `Bytes` keeps its codec form (cratestack#1352).
        let frame = self
            .input_body
            .and_then(|body| self.rpc.inner.codec.decode::<RpcFrameValue>(&body));
        let id = match frame {
            Ok(value) => batch.push_frame(self.op_id, value),
            Err(error) => batch.push_failed_frame(error),
        };
        BatchHandle {
            id,
            _output: std::marker::PhantomData,
        }
    }
}

impl<C, O> std::future::IntoFuture for BatchableCall<C, O>
where
    C: HttpClientCodec + Clone + Send + 'static,
    O: serde::de::DeserializeOwned + Send + 'static,
{
    type Output = Result<O, RpcClientError>;
    type IntoFuture = crate::task::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let body = self.input_body.map_err(RpcClientError::Codec)?;
            self.rpc.call_encoded::<O>(&self.op_id, body).await
        })
    }
}

/// A typed key returned by [`BatchableCall::queue`]. Pair it with
/// [`BatchResults::take`] to extract the typed output for that op
/// from the batch response.
///
/// Carries `O` only as a phantom type — there's no runtime overhead.
/// Cheap to clone; clones share identity (you can `take(handle)` only
/// once, but the type tracks across passes).
pub struct BatchHandle<O> {
    pub(crate) id: u64,
    pub(crate) _output: std::marker::PhantomData<fn() -> O>,
}

impl<O> Clone for BatchHandle<O> {
    // Hand-written (not derived) so the impl doesn't pick up a spurious
    // `O: Clone` bound — `O` is purely a phantom. `BatchHandle` is `Copy`,
    // so the canonical clone is just a copy of `*self`.
    fn clone(&self) -> Self {
        *self
    }
}

impl<O> Copy for BatchHandle<O> {}

impl<O> std::fmt::Debug for BatchHandle<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchHandle").field("id", &self.id).finish()
    }
}

#[cfg(test)]
mod no_null_strip_tests;
