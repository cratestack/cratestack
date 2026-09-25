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
/// anything reaches the wire. An ESP256 signature is normalised to low-`s`.
#[async_trait::async_trait]
pub trait CoseSigner: Send + Sync + 'static {
    /// The algorithm every signature from this signer uses.
    fn alg(&self) -> CoseAlg;

    /// The key's `kid`: the first 8 bytes of its RFC 9679 thumbprint (§3).
    fn kid(&self) -> &[u8];

    /// Sign (or MAC) `to_be_signed`. For HMAC 256/64 this returns the
    /// truncated 8-byte tag. An error is a backend failure (a `500`).
    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError>;

    /// Sign the concatenation of `to_be_signed` without it ever being
    /// built in one buffer, or `None` to have the envelope build it and
    /// call [`sign`](Self::sign) instead (the default).
    ///
    /// This is how the in-process signers (HMAC, ESP256 and Ed25519) keep
    /// the payload in the one place `seal_value` encoded it (maintainer
    /// decisions on cratestack#1005): each hashes its input (Ed25519 twice),
    /// so they can be fed the structure's pieces, which lie in the output
    /// buffer, on the stack and in the header. The design has to stay
    /// dyn-compatible (signers are `Arc<dyn CoseSigner>`), which rules out
    /// a generic "feed this hasher" method; a slice of slices is the
    /// dyn-compatible form of a stream of bytes. It is a provided method,
    /// so adding it broke no signer. It is synchronous because it exists
    /// for in-process signers only: a remote signer needs the bytes (or a
    /// digest) on the wire and keeps `None`, so a KMS or HSM signer is
    /// handed the contiguous structure.
    ///
    /// An override must return exactly what `sign` returns for the
    /// concatenated bytes; the shared vectors check that for the shipped
    /// signers.
    fn sign_chunks(&self, to_be_signed: &[&[u8]]) -> Option<Result<Vec<u8>, CratestackError>> {
        let _ = to_be_signed;
        None
    }
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
///   hybrid key side by side during a migration (Q5).
///
/// **Returning too much is harmless, but wasteful.** The opener accepts a
/// candidate only if the candidate's own `kid` (computed from the key) is
/// the one in the header and the candidate's algorithm is the header's, and
/// the principal it records is the thumbprint of the key that verified. So
/// a resolver that ignores `kid` or `alg` (say, one that returns every
/// active key of a tenant) cannot let one key sign as another, or an HMAC
/// secret stand in for a public key. It only costs a thumbprint comparison
/// per extra candidate, and a lookup that should have been indexed.
#[async_trait::async_trait]
pub trait CoseVerifierResolver: Send + Sync + 'static {
    /// Every key whose `kid` is `kid` and which verifies `alg`; `Ok(vec![])`
    /// if there is none. `Err` only for a backend failure.
    async fn resolve(
        &self,
        kid: &[u8],
        alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError>;
}
