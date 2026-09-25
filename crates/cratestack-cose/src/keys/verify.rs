//! Checking a signature or tag with a [`CoseVerifyKey`]: the one place
//! key material meets received bytes.

use curve25519_dalek::Scalar;
use ed25519_dalek::Signature as EdSignature;
use ed25519_dalek::ed25519::signature::MultipartVerifier as _;
use p256::ecdsa::signature::DigestVerifier;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::hmac::hmac_tag;
use super::verify_key::{CoseVerifyKey, Repr};
use crate::alg::CoseAlg;
use crate::tbs::Tbs;

impl CoseVerifyKey {
    /// Check `signature` over the to-be-signed structure for `alg`. `false`
    /// for any other algorithm than the key's own, a signature of the wrong
    /// length, or a bad signature. Every algorithm reads the structure in
    /// pieces ([`Tbs::with_chunks`]); none builds it contiguously.
    ///
    /// - Ed25519: see [`ed25519_strict`].
    /// - ESP256 accepts **low-`S` only**. ECDSA's `(r, s)` and `(r, n - s)`
    ///   both verify, so without this rule a third party could re-spell a
    ///   signed message, and the `request_digest` a response is bound to
    ///   hashes the bytes: a hop that swapped `s` for `n - s` would make
    ///   every honest response fail at the client. The sealer normalises
    ///   every ESP256 signature, from any signer, so an honest peer never
    ///   sends a high `s`.
    /// - MAC tags are compared in constant time.
    pub(crate) fn verify(&self, alg: CoseAlg, tbs: &Tbs<'_>, signature: &[u8]) -> bool {
        if alg != self.alg() || signature.len() != alg.signature_len() {
            return false;
        }
        match &self.repr {
            Repr::Ed25519(key) => tbs.with_chunks(|chunks| ed25519_strict(key, chunks, signature)),
            Repr::P256(key) => p256::ecdsa::Signature::from_slice(signature).is_ok_and(|sig| {
                sig.normalize_s() == sig
                    && tbs.with_chunks(|chunks| {
                        key.verify_digest(
                            |digest: &mut Sha256| {
                                for chunk in chunks {
                                    Digest::update(digest, chunk);
                                }
                                Ok(())
                            },
                            &sig,
                        )
                        .is_ok()
                    })
            }),
            Repr::Hmac { secret, .. } => {
                let expected = tbs.with_chunks(|chunks| hmac_tag(secret, alg, chunks));
                bool::from(expected.as_slice().ct_eq(signature))
            }
        }
    }
}

/// Ed25519, as strict as `VerifyingKey::verify_strict` in its default
/// configuration, over the message in pieces.
///
/// `verify_strict` takes one contiguous slice. The pieces go through
/// `MultipartVerifier::multipart_verify` instead (the same computation as
/// `ed25519-dalek`'s `hazmat` stream verifier, without the `hazmat`
/// feature), which is `verify_strict` minus checks 2 and 3 below, and both
/// make check 1 only in `ed25519-dalek`'s default configuration. Both
/// compare the recomputed `R` with the received bytes, which also refuses a
/// non-canonical `R` encoding. What this function adds (`ed25519-dalek`
/// 3.0.0, `VerifyingKey::verify_strict`):
///
/// 1. **`S < L`, checked here** with `Scalar::from_canonical_bytes`, the
///    check `ed25519-dalek` itself makes when it parses a signature, but
///    only by default: its `legacy_compatibility` feature swaps it for "the
///    top three bits are clear", which accepts the `S + L` twin of every
///    signature (in `verify_strict` too). Cargo unifies features across
///    the build, so any crate in a consumer's graph could turn that on;
///    checking here keeps this function's strictness independent of
///    `ed25519-dalek`'s features. With the feature on, this function is
///    stricter than `verify_strict`, deliberately.
/// 2. `R` must decompress to a curve point. Kept for parity with
///    `verify_strict` only: it cannot change an outcome, since the
///    recomputed `R` is always a point's canonical encoding and so never
///    equals bytes that are not one.
/// 3. Neither `R` nor the key `A` may be of small order. A small-order key
///    is a "weak" key: with `A` and `R` both the identity and `S = 0`,
///    the cofactorless equation holds for every message. Mixed-order
///    points (a small-order component on top of a large-order one) are
///    accepted, as `verify_strict` accepts them.
///
/// `R` is decompressed through `VerifyingKey::from_bytes`, which is exactly
/// `CompressedEdwardsY::decompress`, the call `verify_strict` makes, and
/// `is_weak` is its `is_small_order`. `tests/ed25519_streaming.rs` checks
/// this function against `verify_strict` on the contiguous bytes for weak
/// keys, small-order `R`, mixed-order keys and `R` and every single-bit
/// flip, and that it refuses the `S + L` twin.
fn ed25519_strict(key: &ed25519_dalek::VerifyingKey, chunks: &[&[u8]], signature: &[u8]) -> bool {
    let Ok(signature) = EdSignature::from_slice(signature) else {
        return false;
    };
    let s_is_canonical = bool::from(Scalar::from_canonical_bytes(*signature.s_bytes()).is_some());
    let r_is_acceptable =
        ed25519_dalek::VerifyingKey::from_bytes(signature.r_bytes()).is_ok_and(|r| !r.is_weak());
    s_is_canonical
        && r_is_acceptable
        && !key.is_weak()
        && key.multipart_verify(chunks, &signature).is_ok()
}

/// `signature` rewritten with a low `s`, or `None` if it is not a valid
/// ESP256 signature at all.
pub(crate) fn esp256_low_s(signature: &[u8]) -> Option<Vec<u8>> {
    let signature = p256::ecdsa::Signature::from_slice(signature).ok()?;
    Some(signature.normalize_s().to_bytes().to_vec())
}
