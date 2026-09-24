//! The signer an envelope verified (ADR 0006 §1, §12).

use bytes::Bytes;

use super::CratestackContext;

/// The key that signed a request body, as verified by
/// [`CratestackEnvelope::open`](crate::CratestackEnvelope::open).
///
/// It is a **recorded fact, not an identity**. It is kept apart from
/// `auth`/`principal`, which an [`AuthProvider`](crate::AuthProvider)
/// fills. How a verified signer feeds the rate-limit and idempotency
/// principal (§12), and whether it can stand in for an `Authorization`
/// header, is decided by cratestack#1006. Recording a signer therefore does
/// not make [`CratestackContext::is_authenticated`] true.
///
/// Today it holds only the `kid`, which in ADR 0006 §3 is the 8-byte
/// thumbprint prefix. It is stored as opaque bytes of any length because
/// Q5 forbids baking in sizes. The fields are private so the COSE
/// implementation (cratestack#1005) can add the algorithm or a resolved
/// device or service id without a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSigner {
    kid: Bytes,
}

impl VerifiedSigner {
    pub fn new(kid: impl Into<Bytes>) -> Self {
        Self { kid: kid.into() }
    }

    /// The verified key id, as it appeared in the protected header.
    pub fn kid(&self) -> &[u8] {
        &self.kid
    }
}

impl CratestackContext {
    /// Record the signer an envelope just verified, replacing any earlier
    /// one. Call it only after verification succeeded.
    ///
    /// This is public because envelope implementations live in other crates.
    /// It is therefore trusted exactly as far as any in-process layer is,
    /// the same trust level as inserting `cratestack-axum`'s
    /// `VerifiedPrincipal` extension. What makes it unforgeable from the
    /// *wire* is that the backing field is private and `#[serde(skip)]`: a
    /// context deserialized from a payload never carries a signer.
    pub fn record_verified_signer(&mut self, signer: VerifiedSigner) {
        self.verified_signer = Some(signer);
    }

    /// The signer recorded by the envelope, if the body was signed and
    /// verified. `None` for unsigned traffic, including every body that
    /// [`NoEnvelope`](crate::NoEnvelope) opened.
    pub fn verified_signer(&self) -> Option<&VerifiedSigner> {
        self.verified_signer.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_signer_is_readable_but_not_an_identity() {
        let mut ctx = CratestackContext::anonymous();
        ctx.record_verified_signer(VerifiedSigner::new(vec![1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(
            ctx.verified_signer().map(VerifiedSigner::kid),
            Some(&[1, 2, 3, 4, 5, 6, 7, 8][..])
        );
        assert!(!ctx.is_authenticated());
    }

    #[test]
    fn a_signer_does_not_survive_serialization() {
        let mut ctx = CratestackContext::anonymous();
        ctx.record_verified_signer(VerifiedSigner::new(&b"kid"[..]));
        let json = serde_json::to_string(&ctx).expect("serialize");
        let decoded: CratestackContext = serde_json::from_str(&json).expect("deserialize");
        assert!(
            decoded.verified_signer().is_none(),
            "leaked onto the wire: {json}"
        );
    }

    #[test]
    fn forged_signer_field_in_payload_is_ignored() {
        let decoded: CratestackContext = serde_json::from_str(
            r#"{"auth":null,"principal":null,"extensions":{},"verified_signer":{"kid":[1,2]}}"#,
        )
        .expect("unknown/skipped field should be ignored");
        assert!(decoded.verified_signer().is_none());
    }
}
