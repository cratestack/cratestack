//! Checking a signature or tag with a [`CoseVerifyKey`]: the one place
//! key material meets received bytes.

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

/// Ed25519, exactly as strict as `VerifyingKey::verify_strict`, over the
/// message in pieces.
///
/// `verify_strict` takes one contiguous slice. The pieces go through
/// `MultipartVerifier::multipart_verify` instead (the same computation as
/// `ed25519-dalek`'s `hazmat` stream verifier, without the `hazmat`
/// feature), which is `verify_strict` minus two checks. Both parse the
/// signature with a canonical `S` (`S < L`, so the `S + L` twin is refused)
/// and compare the recomputed `R` with the received bytes, which also
/// refuses a non-canonical `R` encoding. What `verify_strict` adds, and this
/// function therefore adds back (`ed25519-dalek` 3.0.0,
/// `VerifyingKey::verify_strict`):
///
/// 1. `R` must decompress to a curve point;
/// 2. neither `R` nor the key `A` may be of small order. A small-order key
///    is a "weak" key: with `A` and `R` both the identity and `S = 0`,
///    the cofactorless equation holds for every message.
///
/// `R` is decompressed through `VerifyingKey::from_bytes`, which is exactly
/// `CompressedEdwardsY::decompress`, the call `verify_strict` makes, and
/// `is_weak` is its `is_small_order`, so no direct `curve25519-dalek`
/// dependency is needed. `tests/ed25519_streaming.rs` checks this function
/// against `verify_strict` on the contiguous bytes for weak keys,
/// small-order `R`, the `S + L` twin and every single-byte tamper.
fn ed25519_strict(key: &ed25519_dalek::VerifyingKey, chunks: &[&[u8]], signature: &[u8]) -> bool {
    let Ok(signature) = EdSignature::from_slice(signature) else {
        return false;
    };
    let r_is_acceptable =
        ed25519_dalek::VerifyingKey::from_bytes(signature.r_bytes()).is_ok_and(|r| !r.is_weak());
    r_is_acceptable && !key.is_weak() && key.multipart_verify(chunks, &signature).is_ok()
}

/// `signature` rewritten with a low `s`, or `None` if it is not a valid
/// ESP256 signature at all.
pub(crate) fn esp256_low_s(signature: &[u8]) -> Option<Vec<u8>> {
    let signature = p256::ecdsa::Signature::from_slice(signature).ok()?;
    Some(signature.normalize_s().to_bytes().to_vec())
}
