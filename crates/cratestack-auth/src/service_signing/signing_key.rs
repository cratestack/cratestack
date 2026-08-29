//! [`ServiceSigningKey`] — a backend service's own signing identity.

use std::env;
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use rand::TryRng;
use rand::rngs::SysRng;
use serde::Serialize;

use crate::{AuthError, Jwk, JwksDocument, decode_signing_key, issuer_jwk};

use super::mint_signed_token;

/// Persistent Ed25519 signing identity for a backend service.
///
/// Production deployments load the key from an env var so it's
/// stable across rolling restarts (verifiers cache JWKS by `kid`,
/// and a fresh `kid` on every restart would defeat that cache and
/// stress the JWKS endpoint). For tests + local dev,
/// [`Self::ephemeral`] mints a fresh identity.
///
/// The `kid` is the JWKS key id — pick a stable, human-readable
/// label scoped to the service (e.g. `"vendor-service-v1"`). When
/// you rotate the key, bump the suffix and ship both keys side by
/// side until cached JWKS clients have refreshed.
#[derive(Clone)]
pub struct ServiceSigningKey {
    issuer: String,
    kid: String,
    signing_key: Arc<SigningKey>,
}

impl ServiceSigningKey {
    /// Build from already-loaded material. The `issuer` is the
    /// canonical name of the service in the trust list (e.g.
    /// `"https://vendor-service.internal"` or just
    /// `"vendor-service"`). The `kid` is the JWKS key id.
    pub fn new(issuer: impl Into<String>, kid: impl Into<String>, signing_key: SigningKey) -> Self {
        Self {
            issuer: issuer.into(),
            kid: kid.into(),
            signing_key: Arc::new(signing_key),
        }
    }

    /// Load from `signing_key_env` (URL-safe base64 no-pad of the
    /// 32-byte secret half). Returns `Err(MissingSigningKeyEnv)` if
    /// the env var is unset or empty so the caller can decide
    /// whether to fall back to [`Self::ephemeral`] (dev) or fail
    /// boot (production).
    pub fn from_env(
        issuer: impl Into<String>,
        kid: impl Into<String>,
        signing_key_env: &str,
    ) -> Result<Self, AuthError> {
        let raw = env::var(signing_key_env)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AuthError::MissingSigningKeyEnv(signing_key_env.to_string()))?;
        let signing_key = decode_signing_key(&raw)?;
        Ok(Self::new(issuer, kid, signing_key))
    }

    /// Mint a fresh ephemeral identity. Test + local-dev only —
    /// every restart produces a new `kid` worth of churn at any
    /// configured verifier's JWKS cache.
    pub fn ephemeral(issuer: impl Into<String>, kid: impl Into<String>) -> Self {
        let mut secret = [0u8; 32];
        // `SysRng` is rand 0.10's rename of `OsRng` — the same stateless
        // handle on the OS entropy source, deliberately NOT `rand::rng()`
        // (a thread-local ChaCha12 reseeded from the OS): key material
        // should come straight from the OS, as it did before the bump.
        // The `expect` preserves the old behaviour too: rand 0.8's
        // infallible `fill_bytes` panicked on an entropy failure, and this
        // constructor has no `Result` to widen into.
        SysRng
            .try_fill_bytes(&mut secret)
            .expect("OS entropy source is unavailable");
        Self::new(issuer, kid, SigningKey::from_bytes(&secret))
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// JWK for this key — what `/jwks.json` should publish.
    pub fn jwk(&self) -> Jwk {
        issuer_jwk(&self.signing_key, &self.kid)
    }

    /// JWKS document for this key — convenience wrapper for
    /// single-key services. Multi-key services (mid-rotation) build
    /// their own [`crate::jwks`] vector.
    pub fn jwks_document(&self) -> JwksDocument {
        crate::jwks(vec![self.jwk()])
    }

    /// Sign `claims` with this identity. Convenience wrapper around
    /// [`mint_signed_token`].
    pub fn mint<C: Serialize>(&self, claims: &C) -> Result<String, AuthError> {
        mint_signed_token(&self.signing_key, &self.kid, claims)
    }
}
