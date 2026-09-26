//! [`ServerEnvelope`]: the seam between the layer and whatever verifies and
//! signs (the COSE envelope by default).

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use cratestack_core::{Binding, CratestackError};

use super::opened::{OpenedRequest, SealContext, Sealed};

/// What the layer needs from an envelope on the server side: open a request
/// against its binding, seal a response against its binding.
///
/// `cratestack_cose::CoseEnvelope` implements it (feature `cose`).
/// Implement it yourself to put a different verifier behind the same layer:
/// an envelope whose keys live in an HSM or a KMS (for COSE, a custom
/// `CoseSigner` / `CoseVerifierResolver` on a `CoseEnvelope` is usually
/// less work), or the Sign1 + Mac0 composite of cratestack#1078. With the
/// `envelope` feature alone, no COSE crate is compiled.
///
/// It is not `cratestack_core::CratestackEnvelope`, which is `Clone` (so not
/// object-safe) and records its signer on a context; the layer runs before
/// any context exists and holds the envelope as `dyn`.
///
/// # What the layer guarantees whatever the implementation does
///
/// - `open_request` is the only way a request reaches the router as
///   "signed", and it is called for every request whose `Content-Type` is
///   `application/cose`, whether or not
///   [`is_envelope_content_type`](Self::is_envelope_content_type) says so.
/// - An `Err(CratestackError::Unauthorized(_))` becomes the layer's own
///   coarse, unsigned `401`; the message is dropped. Any other `Err` is a
///   `500` whose detail is logged, never sent.
/// - The binding passed in is built by the layer. The response binding for
///   [`seal_response`](Self::seal_response) reuses the request's values and
///   adds the request digest the layer computed (SHA-256 of the exact bytes
///   handed to `open_request`) and the status.
/// - The [`SealContext`] `open_request` returned comes back, untouched, to
///   the `seal_response` of the same request, and to no other; a response
///   to an unsigned request gets [`SealContext::empty`].
/// - A [`Sealed`] whose media type is not `application/cose` (any
///   parameters) or one this envelope claims through
///   `is_envelope_content_type` is never sent: the layer answers with an
///   unsigned `500` instead, so a response is never labelled as plain.
///
/// # What the implementation must do
///
/// - Verify **everything** the binding names (audience, method, route, path
///   parameters, query, schema digest, payload media type, bound headers)
///   and enforce replay protection before returning `Ok`. The layer cannot
///   check that a verifier verified.
/// - Return `Unauthorized` for every verification failure, and something
///   else (`Internal`, `Unavailable`) only for a local or backend failure.
/// - Return the payload exactly as signed, and the signer that actually
///   verified it (for COSE: the key's full thumbprint, never the claimed
///   `kid` alone).
#[async_trait]
pub trait ServerEnvelope: Send + Sync + 'static {
    /// The `Content-Type` of a sealed response, e.g.
    /// `application/cose; cose-type="cose-sign1"`. When the layer is built
    /// it must be a valid header value, and `application/cose` or a type
    /// [`is_envelope_content_type`](Self::is_envelope_content_type) claims.
    fn media_type(&self) -> &'static str;

    /// Whether `content_type` (a request's `Content-Type`, or one `Accept`
    /// entry, parameters included) names this envelope's framing. Only
    /// needed for a framing other than `application/cose`, which the layer
    /// always recognises itself; the default recognises nothing more.
    fn is_envelope_content_type(&self, content_type: &str) -> bool {
        let _ = content_type;
        false
    }

    /// Verify `body` (the request body exactly as received) against `bind`,
    /// run the replay checks, and return the payload, the signer, and
    /// anything the response to this request must know (the
    /// [`SealContext`], e.g. that a composite answered Mac0 with Mac0).
    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError>;

    /// Seal `payload` (the response body the router produced, CBOR) for
    /// `bind`, a response binding, with the `context` its request's
    /// `open_request` returned.
    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
        context: &SealContext,
    ) -> Result<Sealed, CratestackError>;
}

/// An envelope shared with the rest of the application (or built once and
/// handed to several layers) is still an envelope.
#[async_trait]
impl<T: ServerEnvelope + ?Sized> ServerEnvelope for Arc<T> {
    fn media_type(&self) -> &'static str {
        (**self).media_type()
    }

    fn is_envelope_content_type(&self, content_type: &str) -> bool {
        (**self).is_envelope_content_type(content_type)
    }

    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError> {
        (**self).open_request(body, bind).await
    }

    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
        context: &SealContext,
    ) -> Result<Sealed, CratestackError> {
        (**self).seal_response(payload, bind, context).await
    }
}
