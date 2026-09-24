//! The signing and key-resolution seams (ADR 0006 §1).

use cratestack_core::CratestackError;

use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;

/// Produces signatures (or MAC tags) without handing out the key.
///
/// `KeyProvider::resolve_signing_key` returns raw key bytes, which fits
/// HMAC but not an asymmetric key that lives in an HSM or a KMS (§1: "keys
/// are signed with, not exported"). A signer is asked to sign the complete
/// `Sig_structure` / `MAC_structure` and nothing else.
///
/// `#[async_trait]` rather than `impl Future`: signers are held as
/// `Arc<dyn CoseSigner>` so one envelope type serves every key backend, and
/// a remote signer is a network call anyway, next to which the boxed
/// future is noise.
///
/// The envelope checks what a signer returns: a `kid` that is not 8 bytes,
/// an algorithm that does not fit the envelope's mode, or a signature whose
/// length is not [`CoseAlg::signature_len`] is refused with a `500` before
/// anything reaches the wire.
#[async_trait::async_trait]
pub trait CoseSigner: Send + Sync + 'static {
    /// The algorithm every signature from this signer uses.
    fn alg(&self) -> CoseAlg;

    /// The key's `kid`: the first 8 bytes of its RFC 9679 thumbprint (§3).
    fn kid(&self) -> &[u8];

    /// Sign (or MAC) `to_be_signed`. For HMAC 256/64 this returns the
    /// truncated 8-byte tag.
    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError>;
}

/// Finds the keys that may have produced a message.
///
/// Contract, which the envelope's error mapping depends on:
///
/// - **An unknown `kid` is `Ok(vec![])`, never `Err`.** Every `Err` is
///   treated as a backend outage and becomes a `500`. A resolver that
///   answered "no such key" with `Err` would turn an unknown `kid` into a
///   distinguishable response, the oracle §10 forbids.
/// - **Several candidates are normal.** Eight bytes of thumbprint collide
///   at about 2³² keys, so a resolver returns every key whose `kid`
///   matches, and the opener tries each (§3, Q5).
/// - **Resolve by `(kid, alg)`.** One device may hold a classical key and a
///   hybrid key side by side during a migration (Q5). The opener also
///   skips any candidate whose key type cannot produce `alg`, so a resolver
///   that ignores `alg` cannot cause key-type confusion, but it does waste a
///   verification per wrong-type candidate.
#[async_trait::async_trait]
pub trait CoseVerifierResolver: Send + Sync + 'static {
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError>;
}
