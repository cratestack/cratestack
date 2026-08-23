// -----------------------------------------------------------------------------
// `BatchableCall` + `BatchHandle` — the prepared-call / typed-key duo
// that the typed batch surface is built around. Sits alongside
// `rpc::batch::{BatchBuilder, BatchResults}`, which consume them.
// -----------------------------------------------------------------------------

use cratestack_core::CratestackError;

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
    input_value: Result<serde_json::Value, CratestackError>,
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
        let input_value = serde_json::to_value(input)
            .map_err(|error| CratestackError::Codec(format!("encode batch input: {error}")));
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
    type IntoFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let value = self.input_value.map_err(RpcClientError::Codec)?;
            self.rpc
                .call::<serde_json::Value, O>(&self.op_id, &value)
                .await
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
mod no_null_strip_tests {
    use cratestack_codec_cbor::CborCodec;
    use cratestack_core::CratestackCodec;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Req {
        required: String,
        #[serde(default)]
        optional: Option<String>,
        #[serde(default)]
        nested: Option<Inner>,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Inner {
        #[serde(default)]
        maybe: Option<String>,
        kept: String,
    }

    /// cratestack#677: `BatchableCall::new` no longer strips `null` object
    /// entries before handing the value to the codec (that workaround for
    /// the `0x80`-vs-`0xf6` mis-encoding was obsoleted by cratestack#657's
    /// `serialize_unit_as_null` fix, and was actively dropping explicit
    /// nullable-column clears — see `crates/cratestack-pg/tests/
    /// rpc_batch_explicit_null_clear.rs` for the production-path
    /// regression test). This is the narrower codec-level guarantee the
    /// removal now leans on: an explicit `null`, nested or not, must
    /// still reach the wire as CBOR null and decode cleanly.
    #[test]
    fn unstripped_null_reaches_wire_as_cbor_null_and_decodes_cleanly() {
        let value = serde_json::json!({
            "required": "x",
            "optional": null,
            "nested": { "maybe": null, "kept": "k" },
        });
        let bytes = CborCodec.encode(&value).expect("encode");
        assert!(
            bytes.contains(&0xf6),
            "an explicit null, nested or not, must reach the wire as RFC 8949 null (0xf6): {bytes:02x?}"
        );
        let decoded: Req = CborCodec
            .decode(&bytes)
            .expect("an unstripped null must decode, not error");
        assert_eq!(decoded.required, "x");
        assert_eq!(decoded.optional, None, "top-level null decodes to None");
        assert_eq!(
            decoded.nested,
            Some(Inner {
                maybe: None,
                kept: "k".to_owned(),
            }),
            "nested null decodes to None, sibling field untouched"
        );
    }
}
