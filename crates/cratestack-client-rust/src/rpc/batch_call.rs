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

/// Recursively remove `null`-valued entries from JSON objects, descending into
/// nested objects and array elements. Array `null` *elements* are left intact
/// (their position is significant); only object *entries* are dropped — the
/// shape that `None` optional fields serialize to. Keeps `serde_json::Value::Null`
/// off the CBOR wire, where it would otherwise mis-encode as an empty array.
fn strip_json_null_entries(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, child| !child.is_null());
            for child in map.values_mut() {
                strip_json_null_entries(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                strip_json_null_entries(item);
            }
        }
        _ => {}
    }
}

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
            .map(|mut value| {
                // Strip `null` object entries before the value is handed to the
                // codec. `serde::Serialize` emits `None` optional fields as
                // `serde_json::Value::Null`, and the CBOR codec encodes
                // `serde_json::Value::Null` as the empty-array marker (`0x80`),
                // NOT CBOR null (`0xf6`) — see `cratestack-codec-cbor`. A server
                // decoding the corresponding `Option<T>` field then fails with
                // "expected text, got array". The generated request structs
                // carry `#[serde(default)]` on optional fields, so an absent key
                // decodes as `None` exactly as an explicit null would have. This
                // mirrors the server-side projection that strips null map
                // entries before its own encode, keeping both directions
                // null-free on the wire.
                strip_json_null_entries(&mut value);
                value
            })
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

#[cfg(test)]
mod null_strip_tests {
    use super::strip_json_null_entries;
    use cratestack_codec_cbor::CborCodec;
    use cratestack_core::CoolCodec;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Req {
        required: String,
        // Generated request structs carry `#[serde(default)]` on optionals so an
        // absent key decodes as `None` — the property the strip relies on.
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

    /// The `BatchableCall::new` path: serde -> `serde_json::Value` -> strip nulls.
    /// After stripping, the value encodes to CBOR cleanly and the typed struct
    /// decodes its `None` optionals back from the absent keys.
    #[test]
    fn stripped_none_optionals_round_trip_through_cbor() {
        let req = Req {
            required: "x".to_owned(),
            optional: None,
            nested: Some(Inner { maybe: None, kept: "k".to_owned() }),
        };
        let mut value = serde_json::to_value(&req).expect("to_value");
        assert!(value.get("optional").expect("present").is_null());
        strip_json_null_entries(&mut value);
        assert!(value.get("optional").is_none(), "top-level null entry dropped");
        assert!(
            value["nested"].get("maybe").is_none(),
            "nested null entry dropped"
        );
        assert_eq!(value["nested"]["kept"], serde_json::json!("k"));

        let bytes = CborCodec.encode(&value).expect("encode");
        let decoded: Req = CborCodec.decode(&bytes).expect("decode");
        assert_eq!(decoded, req);
    }

    /// Regression guard: WITHOUT the strip, a `serde_json::Value::Null` optional
    /// mis-encodes as the CBOR empty-array marker and the typed `Option<String>`
    /// decode fails ("expected text, got array") — the exact cross-service bug.
    #[test]
    fn unstripped_null_breaks_typed_decode() {
        let value = serde_json::json!({ "required": "x", "optional": null });
        let bytes = CborCodec.encode(&value).expect("encode");
        assert!(
            CborCodec.decode::<Req>(&bytes).is_err(),
            "unstripped Value::Null must break the typed decode"
        );
    }
}
