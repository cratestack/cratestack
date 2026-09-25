//! `ServiceSigningKey` -> [`CoseSigner`].

use std::fmt;

use cratestack_auth::ServiceSigningKey;
use cratestack_core::CratestackError;
use ed25519_dalek::Signer as _;
use ed25519_dalek::ed25519::signature::MultipartSigner as _;

use crate::alg::CoseAlg;
use crate::keys::{CoseSigner, CoseVerifyKey};
use crate::thumbprint::KID_LEN;

/// A service's `cratestack_auth::ServiceSigningKey` signing COSE_Sign1
/// messages with Ed25519 (`-19`).
///
/// **Two different key ids.** [`ServiceSigningKey::kid`] is the human JWKS
/// label the service chose (`"vendor-service-v1"`), which is what its
/// JWTs carry. The COSE `kid` this signer puts on the wire is something
/// else: the first 8 bytes of the RFC 9679 thumbprint of the key's public
/// half (§3), computed here from the key, never from the label. A
/// verifier resolves this signer by that thumbprint prefix (for example
/// with a `StaticVerifierResolver` holding [`verify_key`](Self::verify_key)),
/// so a label that is renamed, reused or shared between two keys cannot
/// make one key verify as another.
///
/// It holds the `ServiceSigningKey` itself (whose secret sits behind an
/// `Arc`) rather than copying the seed into an
/// [`Ed25519Signer`](crate::Ed25519Signer), so the secret stays in one
/// place. Its signatures are byte-identical to that signer's for the same
/// seed, streaming included (`tests/auth_service_signer.rs` checks it).
#[derive(Clone)]
pub struct ServiceKeySigner {
    key: ServiceSigningKey,
    kid: [u8; KID_LEN],
}

impl ServiceKeySigner {
    pub fn new(key: ServiceSigningKey) -> Self {
        let kid = CoseVerifyKey::from_ed25519(key.signing_key().verifying_key()).kid();
        Self { key, kid }
    }

    /// The matching verification key, for a peer's resolver.
    pub fn verify_key(&self) -> CoseVerifyKey {
        CoseVerifyKey::from_ed25519(self.key.signing_key().verifying_key())
    }

    /// The wrapped service identity (its issuer, JWKS label and JWKS).
    pub fn service_key(&self) -> &ServiceSigningKey {
        &self.key
    }
}

impl From<ServiceSigningKey> for ServiceKeySigner {
    fn from(key: ServiceSigningKey) -> Self {
        Self::new(key)
    }
}

impl fmt::Debug for ServiceKeySigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceKeySigner")
            .field("issuer", &self.key.issuer())
            .field("jwks_kid", &self.key.kid())
            .field("cose_kid", &self.kid)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl CoseSigner for ServiceKeySigner {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Ed25519
    }

    fn kid(&self) -> &[u8] {
        &self.kid
    }

    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        Ok(self
            .key
            .signing_key()
            .sign(to_be_signed)
            .to_bytes()
            .to_vec())
    }

    /// Both PureEdDSA passes over the pieces, exactly as
    /// [`Ed25519Signer`](crate::Ed25519Signer) does.
    fn sign_chunks(&self, to_be_signed: &[&[u8]]) -> Option<Result<Vec<u8>, CratestackError>> {
        Some(
            self.key
                .signing_key()
                .try_multipart_sign(to_be_signed)
                .map(|signature| signature.to_bytes().to_vec())
                .map_err(|_| CratestackError::Internal("Ed25519 signing failed".to_owned())),
        )
    }
}
