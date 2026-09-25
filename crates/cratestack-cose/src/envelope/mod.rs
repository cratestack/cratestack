//! [`CoseEnvelope`]: the COSE implementation of `CratestackEnvelope`.

mod builder;
mod trait_impl;

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::{Binding, CratestackCodec, CratestackError, NonceStore};
use serde::Serialize;

use crate::alg::CoseMode;
use crate::keys::{CoseSigner, CoseVerifierResolver};
use crate::opened::Opened;
use crate::request_nonce::RequestNonce;
use crate::seal::PAYLOAD_CAPACITY_HINT;

pub use builder::CoseEnvelopeBuilder;
use builder::{Clock, CtiSource, NonceSource};

/// Which side of the exchange an envelope is on.
///
/// `CratestackEnvelope` has one `seal` and one `open`, and which message
/// each handles depends on the side: a server opens requests and seals
/// responses, a client seals requests and opens responses. The role is
/// fixed at construction rather than inferred from the [`Binding`] (a
/// response binding carries a `ResponseBinding`), so that a
/// router that builds the wrong binding gets a `500` instead of silently
/// skipping the replay checks a request needs. The typed methods
/// ([`CoseEnvelope::open_request`] and the rest) name the direction
/// explicitly and ignore the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoseRole {
    /// Opens requests (with the replay checks), seals responses.
    Server,
    /// Seals requests (with `iat` and a fresh `cti`), opens responses.
    Client,
}

/// A COSE_Sign1 or COSE_Mac0 envelope (ADR 0006 §§1, 3-5).
///
/// Cheap to clone: the configuration sits behind one `Arc`. Build it with
/// [`CoseEnvelope::server`] or [`CoseEnvelope::client`].
///
/// **Errors**, for every method (§10): a failed verification is the coarse
/// `CratestackError::Unauthorized`; a failing backend (key resolver, nonce
/// store, signer) is `CratestackError::Internal`, a `500`; and **local
/// misuse** (a binding whose shape does not fit the call, an empty
/// `audience`, a request opened without a nonce store, a clock or `cti`
/// source returning nonsense, a signer returning a signature of the wrong
/// length, a codec whose `encode_into` does not append) is also
/// `CratestackError::Internal`. Misuse depends only on local state, never
/// on the received bytes, so a `500` for it reveals nothing about a
/// message. A codec error from a `*_value` method is returned as the codec
/// reported it.
#[derive(Clone)]
pub struct CoseEnvelope {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) role: CoseRole,
    pub(crate) mode: CoseMode,
    pub(crate) signer: Arc<dyn CoseSigner>,
    pub(crate) resolver: Arc<dyn CoseVerifierResolver>,
    pub(crate) nonce_store: Option<Arc<dyn NonceStore>>,
    pub(crate) skew_secs: u64,
    pub(crate) clock: Clock,
    pub(crate) cti: CtiSource,
    pub(crate) nonce: NonceSource,
}

impl Inner {
    fn debug_fields(&self, out: &mut fmt::DebugStruct<'_, '_>) -> fmt::Result {
        out.field("role", &self.role)
            .field("mode", &self.mode)
            .field("alg", &self.signer.alg())
            .field("kid", &self.signer.kid())
            .field("skew_secs", &self.skew_secs)
            .field("nonce_store", &self.nonce_store.is_some())
            .finish_non_exhaustive()
    }
}

impl CoseEnvelope {
    /// The side this envelope is on.
    pub fn role(&self) -> CoseRole {
        self.inner.role
    }

    /// The message structure this envelope emits and accepts. An envelope
    /// accepts only its own mode's tag: a Sign1 envelope refuses a Mac0
    /// message outright, whatever keys its resolver holds.
    pub fn mode(&self) -> CoseMode {
        self.inner.mode
    }

    /// A fresh `Cratestack-Nonce` for an outgoing request, from the
    /// builder's nonce source. Fails with `CratestackError::Internal` if
    /// that source fails.
    pub fn request_nonce(&self) -> Result<RequestNonce, CratestackError> {
        (self.inner.nonce)().map(RequestNonce::from_bytes)
    }

    /// Seal an encoded request payload for `bind` (a request binding:
    /// `response` is `None`), with `iat` from the clock and a
    /// fresh `cti`. The payload is copied into the message once; see
    /// [`seal_request_value`](Self::seal_request_value) to avoid that.
    pub async fn seal_request(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError> {
        crate::seal::seal(&self.inner, bind, true, payload.len(), |out| {
            out.extend_from_slice(payload);
            Ok(())
        })
        .await
    }

    /// Seal an encoded response payload for `bind` (a response binding:
    /// `response` names the request answered, its kind and the status).
    /// Responses carry `alg` and `kid` only.
    pub async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError> {
        crate::seal::seal(&self.inner, bind, false, payload.len(), |out| {
            out.extend_from_slice(payload);
            Ok(())
        })
        .await
    }

    /// [`seal_request`](Self::seal_request) for a value `codec` has not
    /// encoded yet: it is encoded straight into the message buffer. The
    /// bytes are identical to encoding first and sealing after. A codec
    /// error is returned as the codec reported it.
    pub async fn seal_request_value<C, T>(
        &self,
        codec: &C,
        value: &T,
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError>
    where
        C: CratestackCodec,
        T: Serialize + ?Sized,
    {
        crate::seal::seal(&self.inner, bind, true, PAYLOAD_CAPACITY_HINT, |out| {
            codec.encode_into(value, out)
        })
        .await
    }

    /// [`seal_response`](Self::seal_response) for a value, encoded in
    /// place as [`seal_request_value`](Self::seal_request_value) does.
    pub async fn seal_response_value<C, T>(
        &self,
        codec: &C,
        value: &T,
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError>
    where
        C: CratestackCodec,
        T: Serialize + ?Sized,
    {
        crate::seal::seal(&self.inner, bind, false, PAYLOAD_CAPACITY_HINT, |out| {
            codec.encode_into(value, out)
        })
        .await
    }

    /// Verify a request and run the replay checks, without a
    /// `CratestackContext` (the #1006 axum layer runs before one exists).
    /// Needs a nonce store; without one it is local misuse (a `500`).
    pub async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<Opened, CratestackError> {
        crate::open::open(&self.inner, body, bind, true).await
    }

    /// Verify a response against the request it answers.
    pub async fn open_response(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<Opened, CratestackError> {
        crate::open::open(&self.inner, body, bind, false).await
    }
}

impl fmt::Debug for CoseEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.debug_fields(&mut f.debug_struct("CoseEnvelope"))
    }
}
