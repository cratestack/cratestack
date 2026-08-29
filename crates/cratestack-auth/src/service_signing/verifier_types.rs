//! [`MultiIssuerJwksVerifier`]'s data shape + construction.
//!
//! The `verify()` signature/expiry check lives in
//! [`super::verifier_verify`]; JWKS fetch + cache maintenance lives in
//! [`super::verifier_cache`]. Both operate on the trust list and cache
//! defined here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use ed25519_dalek::VerifyingKey;
use reqwest::Client;

use crate::AuthError;

/// Trust-list-driven JWT verifier. One verifier per consuming
/// service; the trust list is the set of `(issuer, jwks_url)` pairs
/// the consumer is willing to honour. Issuers not in the list are
/// rejected.
///
/// Caches JWKS per issuer (per `kid`); on a `kid` miss it refreshes
/// JWKS for that issuer. Cache-key churn (new `kid` every JWKS
/// refresh) is the deployment's signal to rotate — keep `kid`
/// stable until you actually rotate.
#[derive(Clone)]
pub struct MultiIssuerJwksVerifier {
    pub(super) inner: Arc<VerifierInner>,
}

pub(super) struct VerifierInner {
    pub(super) issuers: HashMap<String, IssuerEntry>,
    pub(super) http_client: Client,
}

pub(super) struct IssuerEntry {
    pub(super) jwks_url: String,
    pub(super) cached_keys: Mutex<HashMap<String, VerifyingKey>>,
}

/// Result of a successful [`MultiIssuerJwksVerifier::verify`] —
/// returns the deserialised claim shape `C` plus the envelope
/// fields the verifier already consumed (so the consumer doesn't
/// have to redeserialise them).
#[derive(Clone, Debug)]
pub struct VerifiedToken<C> {
    pub issuer: String,
    pub kid: String,
    pub expires_at: i64,
    pub issued_at: Option<i64>,
    pub claims: C,
}

impl MultiIssuerJwksVerifier {
    /// Build with a `{issuer → jwks_url}` map. Empty trust list is
    /// allowed — every verification will fail with
    /// [`AuthError::UntrustedIssuer`] until you add at least one.
    pub fn new(trusted: HashMap<String, String>) -> Result<Self, AuthError> {
        crate::ensure_crypto_provider();
        let http_client = Client::builder()
            .timeout(StdDuration::from_secs(5))
            .build()
            .map_err(|err| AuthError::IdTokenJwksUnavailable(err.to_string()))?;
        let issuers = trusted
            .into_iter()
            .map(|(issuer, jwks_url)| {
                (
                    issuer,
                    IssuerEntry {
                        jwks_url,
                        cached_keys: Mutex::new(HashMap::new()),
                    },
                )
            })
            .collect();
        Ok(Self {
            inner: Arc::new(VerifierInner {
                issuers,
                http_client,
            }),
        })
    }

    /// Trusted issuers, sorted alphabetically. Useful for healthcheck
    /// / debug output.
    pub fn trusted_issuers(&self) -> Vec<&str> {
        let mut issuers: Vec<&str> = self.inner.issuers.keys().map(|s| s.as_str()).collect();
        issuers.sort();
        issuers
    }
}
