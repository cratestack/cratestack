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
/// It names the **key that verified**, not the key the message claimed:
/// `open` accepts a candidate only if its own `kid` equals the header's,
/// and then records that candidate's full RFC 9679 thumbprint. Anything
/// that derives a principal from a verified signer (#1006's rate-limit and
/// idempotency hooks) should key on [`thumbprint`](Self::thumbprint): an
/// 8-byte `kid` prefix can collide (ADR 0006 §3; two keys sharing one are
/// about 2³² keys away, and a test in `cratestack-cose` constructs a real
/// pair), so the `kid` alone does not name one key.
///
/// The algorithm is the raw IANA "COSE Algorithms" value (`-19` Ed25519,
/// `-9` ESP256, `4`/`5` HMAC), not a typed enum: core must not depend on
/// `cratestack-cose`, and a raw registry value needs no core change when a
/// new algorithm (Q5's hybrid slot) arrives.
///
/// The fields are private, so a device or service id resolved from the key
/// can still be added without a breaking change. The constructor took only
/// a `kid` when cratestack#1004 merged; it now requires the thumbprint and
/// the algorithm, so no envelope can record a signer without naming the key
/// (a breaking change, acceptable pre-1.0 with no downstream user yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSigner {
    kid: Bytes,
    thumbprint: [u8; 32],
    alg: i64,
}

impl VerifiedSigner {
    /// `kid` as it appeared in the protected header, `thumbprint` the full
    /// RFC 9679 thumbprint of the key that verified, `alg` its IANA COSE
    /// algorithm value.
    pub fn new(kid: impl Into<Bytes>, thumbprint: [u8; 32], alg: i64) -> Self {
        Self {
            kid: kid.into(),
            thumbprint,
            alg,
        }
    }

    /// The verified key id, as it appeared in the protected header. The
    /// envelope checked that it is the verifying key's own `kid`.
    pub fn kid(&self) -> &[u8] {
        &self.kid
    }

    /// The RFC 9679 thumbprint of the key that verified: the unambiguous
    /// name of the signer.
    pub fn thumbprint(&self) -> &[u8; 32] {
        &self.thumbprint
    }

    /// The IANA COSE algorithm value the message was verified with.
    pub fn alg(&self) -> i64 {
        self.alg
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
        ctx.record_verified_signer(VerifiedSigner::new(
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            [9; 32],
            -19,
        ));
        let signer = ctx.verified_signer().expect("recorded");
        assert_eq!(signer.kid(), &[1, 2, 3, 4, 5, 6, 7, 8][..]);
        assert_eq!(signer.thumbprint(), &[9; 32]);
        assert_eq!(signer.alg(), -19);
        assert!(!ctx.is_authenticated());
    }

    #[test]
    fn a_signer_does_not_survive_serialization() {
        let mut ctx = CratestackContext::anonymous();
        ctx.record_verified_signer(VerifiedSigner::new(&b"kid"[..], [0; 32], 5));
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
