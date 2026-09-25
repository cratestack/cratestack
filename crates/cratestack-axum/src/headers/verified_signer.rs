//! Carrying the envelope's verified signer from the request extensions into
//! the handler's `CratestackContext` (maintainer decision D2 on
//! cratestack#1006).
//!
//! The envelope layer runs before any context exists, so it can only leave
//! the fact on the request, as a `cratestack_core::VerifiedSigner`
//! extension. Generated handlers build their context from the
//! `AuthProvider`'s answer and then call [`enrich_context_from_envelope`],
//! next to [`super::enrich_context_from_headers`]. A separate function, not a
//! new parameter on that one, so its public signature does not change.
//!
//! This is a recorded fact, **not an authentication**:
//! `CratestackContext::record_verified_signer` leaves `is_authenticated()`
//! and the principal alone. Who the caller is stays the `AuthProvider`'s
//! call (the signer-to-principal adapter is cratestack#1077). The helper is
//! compiled with or without the `cose` feature, so generated code does not
//! depend on it; without the layer nothing ever inserts the extension and
//! this is a no-op.

use cratestack_core::{CratestackContext, VerifiedSigner};

/// Record the `VerifiedSigner` the envelope layer left in `extensions`, if
/// any, on `ctx`. `extensions` is the request's (generated handlers pass
/// `ClientIpContext::extensions`, a clone of them).
///
/// Extensions are in-process only, so nothing on the wire can put one
/// there: a signer is present only when an in-process layer (the envelope
/// layer, or one the application trusts as much) inserted it.
pub fn enrich_context_from_envelope(
    mut ctx: CratestackContext,
    extensions: &http::Extensions,
) -> CratestackContext {
    if let Some(signer) = extensions.get::<VerifiedSigner>() {
        ctx.record_verified_signer(signer.clone());
    }
    ctx
}

#[cfg(test)]
mod tests {
    use cratestack_core::{CratestackContext, VerifiedSigner};

    use super::enrich_context_from_envelope;

    #[test]
    fn a_signer_in_the_extensions_is_recorded_but_does_not_authenticate() {
        let mut extensions = http::Extensions::new();
        extensions.insert(VerifiedSigner::new(vec![1; 8], [7; 32], -19));
        let ctx = enrich_context_from_envelope(CratestackContext::anonymous(), &extensions);
        let signer = ctx.verified_signer().expect("recorded");
        assert_eq!(signer.thumbprint(), &[7; 32]);
        assert!(!ctx.is_authenticated());
    }

    #[test]
    fn without_one_the_context_is_unchanged() {
        let ctx =
            enrich_context_from_envelope(CratestackContext::anonymous(), &http::Extensions::new());
        assert!(ctx.verified_signer().is_none());
    }
}
