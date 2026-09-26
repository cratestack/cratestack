//! [`ServerEnvelope`] for `cratestack_cose::CoseEnvelope`, the default
//! (maintainer decision D7): the typed `open_request` / `seal_response`, so
//! no `CratestackContext` is needed. Feature `cose`.

use async_trait::async_trait;
use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, VerifiedSigner};
use cratestack_cose::CoseEnvelope;

use super::opened::{OpenedRequest, SealContext, Sealed};
use super::server_envelope::ServerEnvelope;

#[async_trait]
impl ServerEnvelope for CoseEnvelope {
    /// `application/cose; cose-type="cose-sign1"` or `"cose-mac0"`, by mode.
    fn media_type(&self) -> &'static str {
        self.mode().media_type()
    }

    /// The signer is the key that verified: its full RFC 9679 thumbprint
    /// (not the 8-byte `kid`, which can collide), a copy of its `kid` (so
    /// the signer does not keep the body alive) and its algorithm. The same
    /// fields the `CratestackEnvelope::open` path records. One framing, so
    /// no seal context.
    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError> {
        let opened = CoseEnvelope::open_request(self, body, bind).await?;
        let signer = VerifiedSigner::new(
            Bytes::copy_from_slice(&opened.kid),
            opened.thumbprint,
            opened.alg.id(),
        );
        Ok(OpenedRequest::new(opened.payload, signer))
    }

    /// One copy of the payload into the message (D1), sent as this
    /// envelope's mode's media type.
    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
        _context: &SealContext,
    ) -> Result<Sealed, CratestackError> {
        let body = CoseEnvelope::seal_response(self, payload, bind).await?;
        Ok(Sealed::new(body, self.mode().media_type()))
    }
}
