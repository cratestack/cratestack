//! What a client carries to seal its calls (cratestack#1007).
//!
//! One field on [`CratestackClient`], so the constructors (`new`,
//! `with_http_client`, `with_middleware_client`) initialise it with
//! `Default` and none of them knows whether the `cose` feature is on. Without
//! the feature the struct is empty and `at` is a plain clone.

use crate::client::core::CratestackClient;
use crate::client::route::RouteRef;
use crate::codec::HttpClientCodec;

#[cfg(feature = "cose")]
use crate::client::contract::Contracts;
#[cfg(feature = "cose")]
use crate::envelope::ClientEnvelope;
#[cfg(feature = "cose")]
use crate::error::ClientError;

/// A route detached from the call that named it.
#[cfg(feature = "cose")]
#[derive(Debug, Clone)]
pub(crate) struct OwnedRoute {
    pub(crate) template: String,
    pub(crate) params: Vec<String>,
}

#[derive(Clone, Default)]
pub(crate) struct Sealing {
    #[cfg(feature = "cose")]
    pub(crate) envelope: Option<ClientEnvelope>,
    /// The op contract digests the binding needs (`client/contract.rs`).
    #[cfg(feature = "cose")]
    pub(crate) contracts: Option<Contracts>,
    /// The route of the call in flight, set by [`CratestackClient::at`].
    #[cfg(feature = "cose")]
    pub(crate) route: Option<OwnedRoute>,
}

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// A copy of this client that knows which route its next call is for,
    /// so a sealed call can bind the route template (see [`RouteRef`]).
    ///
    /// Generated code calls it before every REST operation
    /// (`runtime.at(RouteRef::new("/widgets/{id}", &[&id])).get(..)`) and the
    /// RPC client before every op. It is a copy rather than a `*_at` twin of
    /// each of the twelve request methods, the same shape as
    /// [`with_idempotency`](Self::with_idempotency); it does nothing unless an
    /// envelope is set.
    #[must_use]
    pub fn at(&self, route: RouteRef<'_>) -> Self {
        #[cfg(not(feature = "cose"))]
        let _ = route;
        #[allow(unused_mut)]
        let mut client = self.clone();
        #[cfg(feature = "cose")]
        if client.sealing.envelope.is_some() {
            client.sealing.route = Some(OwnedRoute {
                template: route.template().to_owned(),
                params: route.params().iter().map(|p| (*p).to_owned()).collect(),
            });
        }
        client
    }
}

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
    /// The envelope is CBOR: fails with `BadInput` for a codec whose body is
    /// not `application/cbor` (`JsonCodec`).
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
    ///
    /// **Streams** ([`RpcClient::call_streaming`](crate::RpcClient), the
    /// `*_streamed` methods, subscriptions) are refused with
    /// [`EnvelopeError::StreamsUnsupported`](crate::EnvelopeError) until
    /// ADR 0006 P1.
    ///
    /// **Authorizers** still run, over the *inner* payload with
    /// `Content-Type: application/cbor` (none for a bodiless call): the
    /// request the server's `AuthProvider` sees after it opens the seal.
    pub fn with_envelope(mut self, envelope: ClientEnvelope) -> Result<Self, ClientError> {
        if C::CONTENT_TYPE != "application/cbor" {
            return Err(ClientError::BadInput(format!(
                "a COSE envelope wraps CBOR; this client's codec is {}",
                C::CONTENT_TYPE
            )));
        }
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

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// This client as it must send a raw bridge request to `path`
    /// ([`execute_raw_transport`](Self::execute_raw_transport)): itself when
    /// it has no envelope, otherwise scoped to the request's route.
    ///
    /// A raw request has no generated code to name its route, so only RPC
    /// paths qualify: `/rpc/{op_id}` binds the op id, `/rpc/batch` binds
    /// `batch`. A raw REST path has no template to bind, and is refused
    /// rather than sealed for a route the server would not recognise.
    pub(crate) fn scoped_for_raw_path(
        &self,
        path: &str,
    ) -> Result<std::borrow::Cow<'_, Self>, crate::error::ClientError> {
        #[cfg(feature = "cose")]
        if self.sealing.envelope.is_some() {
            let op = crate::client::raw_path::rpc_op(path).ok_or_else(|| {
                crate::error::ClientError::BadInput(format!(
                    "a sealed raw request must be an RPC one (/rpc/{{op_id}} or /rpc/batch); \
                         '{path}' has no route template to bind"
                ))
            })?;
            return Ok(std::borrow::Cow::Owned(self.at(RouteRef::rpc(op))));
        }
        let _ = path;
        Ok(std::borrow::Cow::Borrowed(self))
    }
}
