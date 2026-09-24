//! [`CoseEnvelope`]: the COSE implementation of `CratestackEnvelope`.

mod builder;
mod trait_impl;

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, NonceStore};

use crate::alg::CoseMode;
use crate::keys::{CoseSigner, CoseVerifierResolver};
use crate::opened::Opened;

pub use builder::{Clock, CoseEnvelopeBuilder, CtiSource};

/// Which side of the exchange an envelope is on.
///
/// `CratestackEnvelope` has one `seal` and one `open`, and which message
/// each handles depends on the side: a server opens requests and seals
/// responses, a client seals requests and opens responses. The role is
/// fixed at construction rather than inferred from the [`Binding`] (a
/// response binding carries `request_digest` and `status`), so that a
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

    /// Seal a request payload for `bind` (a request binding: no
    /// `request_digest`, no `status`), with `iat` from the clock and a
    /// fresh `cti`.
    pub async fn seal_request(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError> {
        crate::seal::seal(&self.inner, payload, bind, true).await
    }

    /// Seal a response payload for `bind` (a response binding: both
    /// `request_digest` and `status`). Responses carry `alg` and `kid`
    /// only.
    pub async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError> {
        crate::seal::seal(&self.inner, payload, bind, false).await
    }

    /// Verify a request and run the replay checks, without a
    /// `CratestackContext` (the #1006 axum layer runs before one exists).
    /// Needs a nonce store.
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
        f.debug_struct("CoseEnvelope")
            .field("role", &self.inner.role)
            .field("mode", &self.inner.mode)
            .field("alg", &self.inner.signer.alg())
            .field("kid", &self.inner.signer.kid())
            .field("skew_secs", &self.inner.skew_secs)
            .finish_non_exhaustive()
    }
}
