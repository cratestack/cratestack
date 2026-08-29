//! JWKS fetch + per-`kid` cache lookup/refresh for
//! [`MultiIssuerJwksVerifier`].
//!
//! The signature/expiry/trust check that calls into this cache lives in
//! [`super::verifier_verify`].

use std::collections::HashMap;

use ed25519_dalek::VerifyingKey;

use crate::{AuthError, JwksDocument, decode_verifying_key};

use super::verifier_types::{IssuerEntry, MultiIssuerJwksVerifier};

impl MultiIssuerJwksVerifier {
    /// Walk every trusted issuer's JWKS looking for a matching `kid`.
    ///
    /// Used by signed-request authentication where the wire format
    /// carries only `keyId=<kid>` without an `iss` claim — the
    /// verifier doesn't know which JWKS owns the kid up front, so it
    /// fans out across the trust list. Returns the first match;
    /// because every signing service registers its own
    /// `ServiceSigningKey` with a service-prefixed `kid` (e.g.
    /// `vendor-service-v1`, `order-service-v3`), collisions across
    /// services would only happen on a configuration mistake.
    ///
    /// Cache miss path: fetches each issuer's JWKS lazily, but only
    /// once per kid lookup — already-warm caches short-circuit.
    pub async fn lookup_verifying_key_by_kid(
        &self,
        kid: &str,
    ) -> Result<Option<(String, VerifyingKey)>, AuthError> {
        // First pass: scan every cached map without any I/O.
        for (issuer, entry) in self.inner.issuers.iter() {
            if let Some(key) = entry
                .cached_keys
                .lock()
                .ok()
                .and_then(|cache| cache.get(kid).copied())
            {
                return Ok(Some((issuer.clone(), key)));
            }
        }

        // Second pass: refresh each issuer's JWKS sequentially and
        // re-check. Sequential keeps the failure mode predictable —
        // one bad endpoint surfaces immediately rather than getting
        // shadowed by a luckier sibling.
        for (issuer, entry) in self.inner.issuers.iter() {
            self.refresh_keys_for(issuer, entry).await?;
            if let Some(key) = entry
                .cached_keys
                .lock()
                .ok()
                .and_then(|cache| cache.get(kid).copied())
            {
                return Ok(Some((issuer.clone(), key)));
            }
        }
        Ok(None)
    }

    pub(super) async fn verifying_key_for(
        &self,
        issuer: &str,
        entry: &IssuerEntry,
        kid: &str,
    ) -> Result<VerifyingKey, AuthError> {
        if let Some(cached) = entry
            .cached_keys
            .lock()
            .ok()
            .and_then(|cache| cache.get(kid).copied())
        {
            return Ok(cached);
        }

        self.refresh_keys_for(issuer, entry).await?;
        entry
            .cached_keys
            .lock()
            .map_err(|_| AuthError::IdTokenJwksUnavailable("jwks cache poisoned".to_string()))?
            .get(kid)
            .copied()
            .ok_or_else(|| AuthError::UnknownIdTokenSigningKey(kid.to_string()))
    }

    async fn refresh_keys_for(&self, _issuer: &str, entry: &IssuerEntry) -> Result<(), AuthError> {
        let response = self
            .inner
            .http_client
            .get(&entry.jwks_url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|err| AuthError::IdTokenJwksUnavailable(err.to_string()))?;
        if !response.status().is_success() {
            return Err(AuthError::IdTokenJwksUnavailable(format!(
                "jwks endpoint returned {}",
                response.status()
            )));
        }
        let document: JwksDocument = response
            .json()
            .await
            .map_err(|err| AuthError::IdTokenJwksUnavailable(err.to_string()))?;
        let keys: HashMap<String, VerifyingKey> = document
            .keys
            .into_iter()
            .filter_map(|jwk| {
                let x = jwk.x?;
                decode_verifying_key(&x).ok().map(|key| (jwk.kid, key))
            })
            .collect();
        let mut cache = entry
            .cached_keys
            .lock()
            .map_err(|_| AuthError::IdTokenJwksUnavailable("jwks cache poisoned".to_string()))?;
        *cache = keys;
        Ok(())
    }
}
