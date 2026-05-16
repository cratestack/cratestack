// -----------------------------------------------------------------------------
// `BatchableCall` + `BatchHandle` — the prepared-call / typed-key duo
// that the typed batch surface is built around. Sits alongside
// `rpc::batch::{BatchBuilder, BatchResults}`, which consume them.
// -----------------------------------------------------------------------------

use cratestack_core::CoolError;

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
/// The input is eagerly converted to `serde_json::Value` at
/// construction time so the same prepared call can flow into either
/// consumption mode without re-borrowing the input. Conversion errors
/// surface lazily — eagerly on `.await`, per-handle on the batch path.
#[must_use = "BatchableCall does nothing until `.await`ed or `.queue(&mut batch)`d"]
pub struct BatchableCall<C, O> {
    rpc: RpcClient<C>,
    op_id: String,
    input_value: Result<serde_json::Value, CoolError>,
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
        let input_value = serde_json::to_value(input)
            .map_err(|error| CoolError::Codec(format!("encode batch input: {error}")));
        Self {
            rpc,
            op_id: op_id.into(),
            input_value,
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
        let id = match self.input_value {
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
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let value = self.input_value.map_err(RpcClientError::Codec)?;
            self.rpc.call::<serde_json::Value, O>(&self.op_id, &value).await
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
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _output: std::marker::PhantomData,
        }
    }
}

impl<O> Copy for BatchHandle<O> {}

impl<O> std::fmt::Debug for BatchHandle<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchHandle").field("id", &self.id).finish()
    }
}
