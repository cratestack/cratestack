//! The typed opening methods of [`CoseEnvelope`]: requests (one binding, or
//! several candidates) and responses.

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError};

use super::CoseEnvelope;
use crate::opened::Opened;

impl CoseEnvelope {
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

    /// [`open_request`](Self::open_request) against several candidate
    /// bindings (differing, for the axum layer, only in the op-contract
    /// digest): the message is parsed once and its key resolved once, and
    /// only the signature verification is repeated per candidate, in
    /// order. Returns the opened request and the index of the binding that
    /// verified. A failed candidate records no nonce; an empty list is the
    /// coarse `401`.
    pub async fn open_request_any(
        &self,
        body: Bytes,
        binds: &[Binding<'_>],
    ) -> Result<(Opened, usize), CratestackError> {
        crate::open::open_any(&self.inner, body, binds, true).await
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
