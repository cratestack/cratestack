//! Turning sealing on: [`CratestackClient::with_envelope`] and the op
//! contract digests it binds (`sealing.rs` holds the state they set).

use crate::client::core::CratestackClient;
use crate::codec::HttpClientCodec;

#[cfg(feature = "cose")]
use crate::client::contract::Contracts;
#[cfg(feature = "cose")]
use crate::envelope::ClientEnvelope;
#[cfg(feature = "cose")]
use crate::error::ClientError;

#[cfg(feature = "cose")]
impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// Seal every request and open every response with `envelope`, over
    /// REST and RPC alike. The client then never sends a plain request and
    /// never accepts a plain response (ADR 0006 `Required` mode): an answer
    /// without a seal is [`EnvelopeError::Unsigned`](crate::EnvelopeError).
    ///
    /// Needs the op contract digests, which the generated `Client::new`
    /// supplies ([`with_contracts`](Self::with_contracts)); a bare client
    /// fails its first call with `BadInput`.
    ///
    /// The codec's `CONTENT_TYPE` is the type of every sealed request, and
    /// its [`payload_accept`](HttpClientCodec::payload_accept) the types it
    /// reads back; CBOR is the default and sends nothing extra, any other
    /// type travels in the `Cratestack-Payload-Type` / `-Accept` headers and
    /// is bound in the seal (cratestack#1168), so the server's layer must
    /// have opted in to it. Fails with `BadInput` for a codec whose types can
    /// never be sealed (an envelope, a stream, a multipart body, or anything
    /// outside the lowercase `type/subtype` grammar). A response sealed under
    /// a type the codec did not list is
    /// [`EnvelopeError::UnexpectedPayloadType`](crate::EnvelopeError), and is
    /// never decoded. A sealed `/rpc/batch` is CBOR both ways, so a batch over
    /// another codec fails with `BadInput` before anything is sent.
    ///
    /// **Retries.** A sealed request carries a fresh `cti`, and the server
    /// answers a replayed one with an unsigned `401`. So sealed requests are
    /// marked [`RequestIdempotency::new(false)`](crate::RequestIdempotency)
    /// for `reqwest-middleware` whatever their method, and a retry layer
    /// must re-enter the client, not replay the bytes.
    ///
    /// **Redirects.** A sealed request is bound to one route, so the client
    /// never follows a redirect: [`CratestackClient::new`] builds its
    /// `reqwest::Client` with `redirect::Policy::none()`, and an answer from
    /// any URL other than the one sealed for is
    /// [`EnvelopeError::Unverified`](crate::EnvelopeError). A client supplied
    /// through `with_http_client` or `with_middleware_client` **must not
    /// follow redirects** either; the check after the fact catches a `303`
    /// that turned the call into a plain `GET`, but not a hop that already
    /// received the sealed bytes.
    ///
    /// **On `wasm32` the redirect is followed by the browser.** reqwest
    /// 0.13's wasm client cannot set `fetch`'s `redirect: "error"` mode (it
    /// exposes only `no-cors`), and `fetch` follows a redirect before this
    /// client sees the response. The `response.url()` check therefore
    /// detects the redirect only after the redirected request has been
    /// sent: it stops the answer being accepted, not the request being
    /// re-sent. Do not put a redirecting hop in front of a browser client.
    ///
    /// **Compact keys** adapt under the seal as on a plain call (`compact` module docs).
    ///
    /// **Streams** ([`RpcClient::call_streaming`](crate::RpcClient), the
    /// `*_streamed` methods, subscriptions) are refused with
    /// [`EnvelopeError::StreamsUnsupported`](crate::EnvelopeError) until
    /// ADR 0006 P1.
    ///
    /// **Authorizers** still run, over the *inner* payload with the codec's
    /// `Content-Type` (none for a bodiless call): the request the server's
    /// `AuthProvider` sees after it opens the seal.
    pub fn with_envelope(mut self, envelope: ClientEnvelope) -> Result<Self, ClientError> {
        crate::envelope::check_payload_types(C::CONTENT_TYPE, self.codec.payload_accept())?;
        self.sealing.envelope = Some(envelope);
        Ok(self)
    }

    /// The generating schema's `OP_CONTRACTS`: the digest of each op's wire
    /// shape, looked up per call and bound into the sealed request (binding
    /// version 2, cratestack#1123). Called by the generated `Client::new`,
    /// next to [`with_schema_sha`](Self::with_schema_sha). A call to an op
    /// the table lacks fails with `BadInput` and is never sent.
    #[must_use]
    pub fn with_contracts(mut self, contracts: cratestack_core::OpContracts) -> Self {
        self.sealing.contracts = Some(Contracts::Table(contracts));
        self
    }

    /// Bind one digest in every sealed call, for a hand-built client of a
    /// single op (raw calls, tests). Prefer [`with_contracts`](Self::with_contracts).
    #[must_use]
    pub fn with_contract_sha(mut self, digest: [u8; 32]) -> Self {
        self.sealing.contracts = Some(Contracts::Pinned(digest));
        self
    }
}

/// Without the feature the generated `Client::new` still calls this, and it
/// does nothing.
#[cfg(not(feature = "cose"))]
impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    #[doc(hidden)]
    #[must_use]
    pub fn with_contracts(self, contracts: cratestack_core::OpContracts) -> Self {
        let _ = contracts;
        self
    }
}
