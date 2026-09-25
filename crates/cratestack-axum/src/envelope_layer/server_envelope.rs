//! [`ServerEnvelope`]: the seam between the layer and whatever verifies and
//! signs (the COSE envelope by default).

use async_trait::async_trait;
use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, VerifiedSigner};

/// What the layer needs from an envelope on the server side: open a request
/// against its binding, seal a response against its binding.
///
/// `cratestack_cose::CoseEnvelope` implements it. Implement it yourself to
/// put a different verifier behind the same layer: an envelope whose keys
/// live in an HSM or a KMS (for COSE, a custom `CoseSigner` /
/// `CoseVerifierResolver` on a `CoseEnvelope` is usually less work), or the
/// Sign1 + Mac0 composite of cratestack#1078.
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
///
/// # What the implementation must do
///
/// - Verify **everything** the binding names (audience, method, route, path
///   parameters, query, schema digest, payload media type) and enforce
///   replay protection before returning `Ok`. The layer cannot check that a
///   verifier verified.
/// - Return `Unauthorized` for every verification failure, and something
///   else (`Internal`, `Unavailable`) only for a local or backend failure.
/// - Return the payload exactly as signed, and the signer that actually
///   verified it (for COSE: the key's full thumbprint, never the claimed
///   `kid` alone).
#[async_trait]
pub trait ServerEnvelope: Send + Sync + 'static {
    /// The `Content-Type` of a sealed response, e.g.
    /// `application/cose; cose-type="cose-sign1"`. Checked to be a valid
    /// header value when the layer is built.
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
    /// run the replay checks, and return the payload and the signer.
    async fn open_request(
        &self,
        body: Bytes,
        bind: &Binding<'_>,
    ) -> Result<OpenedRequest, CratestackError>;

    /// Seal `payload` (the response body the router produced, CBOR) for
    /// `bind`, a response binding.
    async fn seal_response(
        &self,
        payload: &[u8],
        bind: &Binding<'_>,
    ) -> Result<Bytes, CratestackError>;
}

/// A request an envelope verified: the payload to hand the router, and the
/// signer to record.
///
/// An implementation builds it with [`OpenedRequest::new`] only after the
/// checks passed. The layer does not hand it to plug-ins: the principal
/// mapper gets a [`super::VerifiedRequest`], which only the layer can make.
#[derive(Clone)]
pub struct OpenedRequest {
    payload: Bytes,
    signer: VerifiedSigner,
}

impl OpenedRequest {
    /// `payload` exactly as signed (a slice of the body is fine), `signer`
    /// the key that verified it.
    pub fn new(payload: Bytes, signer: VerifiedSigner) -> Self {
        Self { payload, signer }
    }

    /// The payload, as signed.
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// The key that verified.
    pub fn signer(&self) -> &VerifiedSigner {
        &self.signer
    }

    pub(super) fn into_parts(self) -> (Bytes, VerifiedSigner) {
        (self.payload, self.signer)
    }
}

/// The payload's length, not its bytes: a verified body is application data
/// and `{:?}` is how it would reach a log.
impl std::fmt::Debug for OpenedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedRequest")
            .field("payload_len", &self.payload.len())
            .field("signer", &self.signer)
            .finish()
    }
}
