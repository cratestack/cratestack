//! Checking a signature or tag with a [`CoseVerifyKey`]: the one place
//! key material meets received bytes.

use ed25519_dalek::Signature as EdSignature;
use p256::ecdsa::signature::DigestVerifier;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::hmac::hmac_tag;
use super::verify_key::{CoseVerifyKey, Repr};
use crate::alg::CoseAlg;
use crate::tbs::TbsView;

impl CoseVerifyKey {
    /// Check `signature` over the to-be-signed structure for `alg`. `false`
    /// for any other algorithm than the key's own, a signature of the wrong
    /// length, or a bad signature.
    ///
    /// - Ed25519 uses `verify_strict`, which rejects small-order keys and
    ///   non-canonical `S`, over the contiguous structure (PureEdDSA).
    /// - ESP256 accepts **low-`S` only**. ECDSA's `(r, s)` and `(r, n - s)`
    ///   both verify, so without this rule one message would have two
    ///   encodings, and the `request_digest` a response is bound to hashes
    ///   the bytes: a hop that swapped `s` for `n - s` would make every
    ///   honest response fail at the client. The sealer normalises every
    ///   ESP256 signature, from any signer, so an honest peer never sends a
    ///   high `s`. Hashed incrementally, without a contiguous copy.
    /// - MAC tags are computed incrementally and compared in constant time.
    pub(crate) fn verify(&self, alg: CoseAlg, tbs: &TbsView<'_>, signature: &[u8]) -> bool {
        if alg != self.alg() || signature.len() != alg.signature_len() {
            return false;
        }
        match &self.repr {
            Repr::Ed25519(key) => EdSignature::from_slice(signature)
                .is_ok_and(|sig| key.verify_strict(tbs.contiguous(), &sig).is_ok()),
            Repr::P256(key) => p256::ecdsa::Signature::from_slice(signature).is_ok_and(|sig| {
                sig.normalize_s() == sig
                    && tbs.tbs().with_chunks(|chunks| {
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
                let expected = tbs
                    .tbs()
                    .with_chunks(|chunks| hmac_tag(secret, alg, chunks));
                bool::from(expected.as_slice().ct_eq(signature))
            }
        }
    }
}

/// `signature` rewritten with a low `s`, or `None` if it is not a valid
/// ESP256 signature at all.
pub(crate) fn esp256_low_s(signature: &[u8]) -> Option<Vec<u8>> {
    let signature = p256::ecdsa::Signature::from_slice(signature).ok()?;
    Some(signature.normalize_s().to_bytes().to_vec())
}
