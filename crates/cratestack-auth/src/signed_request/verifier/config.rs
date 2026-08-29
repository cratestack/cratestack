//! [`SignedRequestVerifier`]'s fields and every constructor/builder method.
//! The actual verify/authenticate request path lives in
//! [`super::authenticate`], which needs `pub(super)` access to these
//! fields — hence they aren't plain-private.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Duration;
use ed25519_dalek::VerifyingKey;

use crate::AuthError;
use crate::id_token::IdTokenVerifier;
use crate::service_signing::MultiIssuerJwksVerifier;
use crate::signed_request::consts::{
    DEFAULT_SIGNATURE_MAX_SKEW_SECONDS, DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS,
    SIGNATURE_MAX_SKEW_SECONDS_ENV, SIGNATURE_REPLAY_WINDOW_SECONDS_ENV,
};
use crate::signed_request::device_key_resolver::DeviceKeyResolver;
use crate::signed_request::env::{
    parse_trusted_issuers_from_env_optional, parse_trusted_keys_from_env_optional,
    parse_window_seconds,
};
use crate::signed_request::nonce_redis::RedisNonceStore;
use crate::signed_request::nonce_store::{InMemoryNonceStore, NonceStore};

#[derive(Clone)]
pub struct SignedRequestVerifier {
    pub(super) trusted_keys: Arc<BTreeMap<String, VerifyingKey>>,
    /// Optional JWKS-backed key resolver. When the static
    /// `trusted_keys` map misses, the verifier falls through to
    /// JWKS lookup across every registered issuer. This is what
    /// lets a signing service rotate kids without coordinating an
    /// env-var redeploy of every receiver.
    pub(super) jwks_resolver: Option<MultiIssuerJwksVerifier>,
    pub(super) max_skew: Duration,
    pub(super) replay_window: Duration,
    pub(super) nonce_store: Arc<dyn NonceStore>,
    pub(super) id_token_verifier: Option<Arc<IdTokenVerifier>>,
    /// Last-resort resolver for device keys, consulted when both the
    /// static map and the JWKS resolver miss. Enables device
    /// proof-of-possession (see [`DeviceKeyResolver`]).
    pub(super) device_key_resolver: Option<Arc<dyn DeviceKeyResolver>>,
}

impl SignedRequestVerifier {
    pub fn new<I>(trusted_keys: I) -> Self
    where
        I: IntoIterator<Item = (String, VerifyingKey)>,
    {
        Self {
            trusted_keys: Arc::new(trusted_keys.into_iter().collect()),
            jwks_resolver: None,
            max_skew: Duration::seconds(DEFAULT_SIGNATURE_MAX_SKEW_SECONDS),
            replay_window: Duration::seconds(DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS),
            nonce_store: Arc::new(InMemoryNonceStore::default()),
            id_token_verifier: None,
            device_key_resolver: None,
        }
    }

    /// Build a verifier with JWKS-only key resolution — no statically
    /// trusted keys. Use when every signer is rotation-capable and
    /// the env-pinned shortlist isn't needed.
    pub fn jwks_only(jwks_resolver: MultiIssuerJwksVerifier) -> Self {
        Self {
            trusted_keys: Arc::new(BTreeMap::new()),
            jwks_resolver: Some(jwks_resolver),
            max_skew: Duration::seconds(DEFAULT_SIGNATURE_MAX_SKEW_SECONDS),
            replay_window: Duration::seconds(DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS),
            nonce_store: Arc::new(InMemoryNonceStore::default()),
            id_token_verifier: None,
            device_key_resolver: None,
        }
    }

    pub fn from_env() -> Result<Self, AuthError> {
        let trusted_keys = parse_trusted_keys_from_env_optional()?;
        let trusted_issuers = parse_trusted_issuers_from_env_optional()?;

        if trusted_keys.is_empty() && trusted_issuers.is_empty() {
            return Err(AuthError::MissingTrustedSigningKeys);
        }

        let max_skew = Duration::seconds(parse_window_seconds(
            SIGNATURE_MAX_SKEW_SECONDS_ENV,
            DEFAULT_SIGNATURE_MAX_SKEW_SECONDS,
        )?);
        let replay_window = Duration::seconds(parse_window_seconds(
            SIGNATURE_REPLAY_WINDOW_SECONDS_ENV,
            DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS,
        )?);

        let mut verifier = Self::new(trusted_keys).with_windows(max_skew, replay_window);
        if !trusted_issuers.is_empty() {
            verifier = verifier.with_jwks_resolver(MultiIssuerJwksVerifier::new(trusted_issuers)?);
        }
        Ok(verifier)
    }

    /// Attach (or replace) the JWKS-based resolver used as a fallback
    /// when the static `trusted_keys` map misses on a `kid`.
    pub fn with_jwks_resolver(mut self, jwks_resolver: MultiIssuerJwksVerifier) -> Self {
        self.jwks_resolver = Some(jwks_resolver);
        self
    }

    /// Attach a device-key resolver, consulted after `trusted_keys` and the
    /// JWKS resolver both miss on a `kid`. This is what lets device-signed
    /// requests (whose keys live in a service's device registry, not any
    /// JWKS) be verified with full proof-of-possession.
    pub fn with_device_key_resolver(mut self, resolver: Arc<dyn DeviceKeyResolver>) -> Self {
        self.device_key_resolver = Some(resolver);
        self
    }

    pub fn with_windows(mut self, max_skew: Duration, replay_window: Duration) -> Self {
        self.max_skew = max_skew;
        self.replay_window = replay_window;
        self
    }

    pub fn with_nonce_store(mut self, nonce_store: Arc<dyn NonceStore>) -> Self {
        self.nonce_store = nonce_store;
        self
    }

    pub fn with_id_token_verifier(mut self, verifier: IdTokenVerifier) -> Self {
        self.id_token_verifier = Some(Arc::new(verifier));
        self
    }

    pub fn with_optional_id_token_verifier(
        self,
        issuer: Option<&str>,
        jwks_url: Option<&str>,
        audience: Option<&str>,
    ) -> Result<Self, AuthError> {
        match (issuer, jwks_url) {
            (Some(issuer), Some(jwks_url)) => {
                Ok(self.with_id_token_verifier(IdTokenVerifier::new(issuer, jwks_url, audience)?))
            }
            _ => Ok(self),
        }
    }

    pub fn with_optional_redis_nonce_store(
        self,
        redis_url: Option<&str>,
    ) -> Result<Self, AuthError> {
        match redis_url {
            Some(redis_url) => self.with_redis_nonce_store(redis_url),
            None => Ok(self),
        }
    }

    pub fn with_redis_nonce_store(self, redis_url: &str) -> Result<Self, AuthError> {
        let nonce_store = RedisNonceStore::new(redis_url)?;
        Ok(self.with_nonce_store(Arc::new(nonce_store)))
    }
}
