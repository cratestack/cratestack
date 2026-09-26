//! [`PrincipalMapper`]: from a verified request to the `VerifiedPrincipal`
//! the rate limiter and the idempotency layer key on (ADR 0006 §12).

use async_trait::async_trait;
use cratestack_core::{CratestackError, VerifiedSigner};
use http::Method;

use super::resolver::ResolvedRoute;

/// A request the envelope verified, as the [`PrincipalMapper`] sees it.
///
/// It has no public constructor: only the layer makes one, and only after
/// [`super::ServerEnvelope::open_request`] succeeded. A mapper therefore
/// cannot be handed an unverified signer, by a caller or by mistake:
///
/// ```compile_fail,E0451
/// use cratestack_axum::envelope_layer::VerifiedRequest;
/// let _ = VerifiedRequest { signer: todo!(), method: todo!(), route: todo!() };
/// ```
///
/// ```compile_fail,E0624
/// use cratestack_axum::envelope_layer::VerifiedRequest;
/// let _ = VerifiedRequest::new(todo!(), todo!(), todo!());
/// ```
///
/// The type itself is public, so a mapper can name it (this one compiles,
/// which shows the two above fail for the privacy and nothing else):
///
/// ```
/// use cratestack_axum::envelope_layer::VerifiedRequest;
/// fn mapper(verified: &VerifiedRequest<'_>) -> String {
///     format!("{:?}", verified.signer().thumbprint())
/// }
/// ```
#[derive(Debug)]
pub struct VerifiedRequest<'a> {
    signer: &'a VerifiedSigner,
    method: &'a Method,
    route: &'a ResolvedRoute,
}

impl<'a> VerifiedRequest<'a> {
    pub(super) fn new(
        signer: &'a VerifiedSigner,
        method: &'a Method,
        route: &'a ResolvedRoute,
    ) -> Self {
        Self {
            signer,
            method,
            route,
        }
    }

    /// The key that verified the request.
    pub fn signer(&self) -> &'a VerifiedSigner {
        self.signer
    }

    /// The request method.
    pub fn method(&self) -> &'a Method {
        self.method
    }

    /// The op it addressed.
    pub fn route(&self) -> &'a ResolvedRoute {
        self.route
    }
}

/// Names the principal a verified request is charged to: the value of the
/// `VerifiedPrincipal` extension the layer inserts, which the rate limiter's
/// default key (`princ:` bucket, no cardinality budget) and the idempotency
/// layer's default fingerprint (`princ:` namespace) use.
///
/// The default is [`ThumbprintPrincipal`]. Replace it to charge a signer to
/// something coarser (its device's owner, its tenant), looked up
/// asynchronously if need be. It only names a principal for rate limiting
/// and idempotency; turning a signer into an *identity* for authorization
/// is an `AuthProvider`'s job (the adapter of cratestack#1077), never this
/// trait's (decision D2).
///
/// Async and fallible (API-review decision, 2026-09-26):
///
/// - `Err(CratestackError::Unauthorized(_))` refuses the request with the
///   layer's coarse, **unsigned** `401` (a revoked device, say): the same
///   answer as a failed verification, so the peer learns nothing more.
/// - Any other `Err` (a lookup that failed) is a **sealed** `500`, since
///   the request did verify; the detail is logged, never sent.
///
/// **What the layer enforces whatever this returns:** it is called only
/// with a [`VerifiedRequest`], after verification. An empty principal is
/// refused (a sealed `500`), since it would put every signer in one bucket
/// and one idempotency namespace. A constant principal does the same thing
/// and cannot be detected: choose a mapper that separates callers.
#[async_trait]
pub trait PrincipalMapper: Send + Sync + 'static {
    /// The principal to insert for this request.
    async fn principal(&self, verified: &VerifiedRequest<'_>) -> Result<String, CratestackError>;
}

/// `cose:` followed by the lowercase hex of the verifying key's full RFC
/// 9679 thumbprint, never its 8-byte `kid` (two keys can share one).
#[derive(Debug, Clone, Copy, Default)]
pub struct ThumbprintPrincipal;

#[async_trait]
impl PrincipalMapper for ThumbprintPrincipal {
    async fn principal(&self, verified: &VerifiedRequest<'_>) -> Result<String, CratestackError> {
        let hex: String = verified
            .signer
            .thumbprint()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(format!("cose:{hex}"))
    }
}

/// A synchronous mapper is a closure.
#[async_trait]
impl<F> PrincipalMapper for F
where
    F: Fn(&VerifiedRequest<'_>) -> Result<String, CratestackError> + Send + Sync + 'static,
{
    async fn principal(&self, verified: &VerifiedRequest<'_>) -> Result<String, CratestackError> {
        self(verified)
    }
}
