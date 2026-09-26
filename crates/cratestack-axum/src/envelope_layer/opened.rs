//! The values a [`super::ServerEnvelope`] hands back: [`OpenedRequest`],
//! the opaque per-request [`SealContext`], and a [`Sealed`] response.

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::VerifiedSigner;

/// A request an envelope verified: the payload to hand the router, the
/// signer to record, and the context its response is sealed with.
///
/// An implementation builds it with [`OpenedRequest::new`] only after the
/// checks passed. The layer does not hand it to plug-ins: the principal
/// mapper gets a [`super::VerifiedRequest`], which only the layer can make.
#[derive(Clone)]
pub struct OpenedRequest {
    payload: Bytes,
    signer: VerifiedSigner,
    context: SealContext,
}

impl OpenedRequest {
    /// `payload` exactly as signed (a slice of the body is fine), `signer`
    /// the key that verified it. The seal context is
    /// [`SealContext::empty`]; see [`with_seal_context`](Self::with_seal_context).
    pub fn new(payload: Bytes, signer: VerifiedSigner) -> Self {
        Self {
            payload,
            signer,
            context: SealContext::empty(),
        }
    }

    /// Carry `context` to this request's `seal_response`.
    pub fn with_seal_context(self, context: SealContext) -> Self {
        Self { context, ..self }
    }

    /// The payload, as signed.
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// The key that verified.
    pub fn signer(&self) -> &VerifiedSigner {
        &self.signer
    }

    /// What `seal_response` will be handed for this request.
    pub fn seal_context(&self) -> &SealContext {
        &self.context
    }

    pub(super) fn into_parts(self) -> (Bytes, VerifiedSigner, SealContext) {
        (self.payload, self.signer, self.context)
    }
}

/// The payload's length, not its bytes: a verified body is application data
/// and `{:?}` is how it would reach a log.
impl fmt::Debug for OpenedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenedRequest")
            .field("payload_len", &self.payload.len())
            .field("signer", &self.signer)
            .field("seal_context", &self.context)
            .finish()
    }
}

/// Opaque per-request state an envelope passes from `open_request` to the
/// `seal_response` of the same request (API-review decision, 2026-09-26).
///
/// The layer never looks inside: it only carries it. A composite envelope
/// (cratestack#1078) records which framing opened the request, so that a
/// Mac0 request is answered with Mac0 and a Sign1 one with Sign1; a
/// single-framing envelope such as `CoseEnvelope` needs nothing and leaves
/// it [`empty`](Self::empty).
#[derive(Clone, Default)]
pub struct SealContext(Option<Arc<dyn Any + Send + Sync>>);

impl SealContext {
    /// No context: what a single-framing envelope returns, and what the
    /// layer passes when sealing the response to an unsigned request.
    pub fn empty() -> Self {
        Self(None)
    }

    /// Carry `value`.
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Some(Arc::new(value)))
    }

    /// The value, if it is a `T`.
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.0.as_deref().and_then(|value| value.downcast_ref())
    }

    /// Whether this is [`empty`](Self::empty).
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}

impl fmt::Debug for SealContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.is_empty() {
            "SealContext(empty)"
        } else {
            "SealContext(..)"
        })
    }
}

/// A sealed response body and the media type the layer stamps on it as its
/// `Content-Type` (API-review decision, 2026-09-26: a composite answers
/// each request in its own framing, so the type is per response, not per
/// envelope).
///
/// The layer checks the type before sending: it must be a valid header
/// value naming `application/cose` or a type the envelope claims.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed {
    body: Bytes,
    media_type: Cow<'static, str>,
}

impl Sealed {
    /// `body`, sealed, to be sent as `media_type`.
    pub fn new(body: Bytes, media_type: impl Into<Cow<'static, str>>) -> Self {
        Self {
            body,
            media_type: media_type.into(),
        }
    }

    /// The sealed bytes.
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// The `Content-Type` to send them as.
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub(super) fn into_parts(self) -> (Bytes, Cow<'static, str>) {
        (self.body, self.media_type)
    }
}

impl fmt::Debug for Sealed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sealed")
            .field("body_len", &self.body.len())
            .field("media_type", &self.media_type)
            .finish()
    }
}
