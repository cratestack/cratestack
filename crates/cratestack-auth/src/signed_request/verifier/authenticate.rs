//! The actual verify/authenticate request path for
//! [`SignedRequestVerifier`], layered on top of the construction/config
//! surface in [`super::config`].

use chrono::{DateTime, Utc};
use ed25519_dalek::{Verifier, VerifyingKey};
use http::{Method, Uri};

use super::SignedRequestVerifier;
use crate::id_token::RequestPrincipal;
use crate::signed_request::canonical::{canonical_signature_base, content_sha256_base64url};
use crate::signed_request::keys::decode_signature;
use crate::signed_request::types::SignedRequestPrincipal;
use crate::signed_request::validate::{
    validate_content_hash, validate_signature_algorithm, validate_timestamp,
};
use crate::{AuthError, SignatureHeader, parse_signature_header};

impl SignedRequestVerifier {
    pub async fn verify(
        &self,
        method: &Method,
        uri: &Uri,
        body: &[u8],
        authorization: &str,
    ) -> Result<SignedRequestPrincipal, AuthError> {
        let header = parse_signature_header(authorization)?;
        validate_signature_algorithm(header.alg.as_deref())?;
        let timestamp = DateTime::parse_from_rfc3339(&header.timestamp)
            .map_err(|error| AuthError::InvalidSignatureTimestamp(error.to_string()))?;
        if timestamp.offset().local_minus_utc() != 0 {
            return Err(AuthError::InvalidSignatureTimestamp(
                "timestamp must use UTC offset".to_string(),
            ));
        }
        let timestamp = timestamp.with_timezone(&Utc);
        validate_timestamp(timestamp, self.max_skew)?;

        let content_sha256 = content_sha256_base64url(body);
        validate_content_hash(&header, &content_sha256)?;

        let signature_base = canonical_signature_base(
            method,
            uri.path(),
            uri.query(),
            &content_sha256,
            &header.timestamp,
            &header.nonce,
            &header.key_id,
        );

        let mut via_id_token_pop = false;
        let verifying_key = match self.resolve_verifying_key(&header.key_id).await? {
            Some(key) => key,
            // PoP fallback ONLY for services with no device registry of their own.
            // A wired DeviceKeyResolver returning None is AUTHORITATIVE — it is
            // exactly how auth-service reports a revoked/disabled/unknown device —
            // and must never be overridden by a (still-unexpired) id_jwt's cnf.jwk,
            // or a revoked device could keep signing until its token expires.
            None if self.device_key_resolver.is_none() => {
                let key = self.resolve_id_token_bound_key(&header).await?;
                via_id_token_pop = true;
                key
            }
            None => return Err(AuthError::UnknownSigningKey(header.key_id.clone())),
        };
        let signature = decode_signature(&header.signature)?;
        verifying_key
            .verify(signature_base.as_bytes(), &signature)
            .map_err(|_| AuthError::SignatureVerificationFailed)?;

        self.nonce_store
            .claim(&header.key_id, &header.nonce, timestamp, self.replay_window)
            .await?;

        Ok(SignedRequestPrincipal {
            key_id: header.key_id,
            timestamp,
            nonce: header.nonce,
            id_jwt: header.id_jwt,
            alg: header.alg,
            content_sha256,
            via_id_token_pop,
        })
    }

    /// Walk the static map first, then fall through to the JWKS
    /// resolver if one is wired. Returns `None` when neither knows
    /// the kid; the caller maps that to [`AuthError::UnknownSigningKey`].
    async fn resolve_verifying_key(&self, key_id: &str) -> Result<Option<VerifyingKey>, AuthError> {
        if let Some(key) = self.trusted_keys.get(key_id).copied() {
            return Ok(Some(key));
        }
        // JWKS (service signers) is resolved BEFORE the device directory so a
        // device can never shadow a trusted service `kid` (enrollment accepts a
        // caller-supplied proposedKeyId). But a JWKS fetch error must NOT fail
        // the whole request — otherwise an unrelated trusted issuer's outage
        // would 500 valid device-signed requests. Treat a JWKS error as a miss
        // and fall through to the device resolver.
        if let Some(resolver) = self.jwks_resolver.as_ref() {
            match resolver.lookup_verifying_key_by_kid(key_id).await {
                Ok(Some((_issuer, key))) => return Ok(Some(key)),
                Ok(None) => {}
                Err(_jwks_unavailable) => {}
            }
        }
        if let Some(resolver) = self.device_key_resolver.as_ref()
            && let Some(key) = resolver.lookup_device_verifying_key(key_id).await?
        {
            return Ok(Some(key));
        }
        Ok(None)
    }

    /// Final resolution tier (proof-of-possession). A device key that the static
    /// map, JWKS, and device resolver all missed can still be trusted when the
    /// request carries an id_jwt that (a) cryptographically verifies against a
    /// trusted issuer's JWKS and (b) binds THIS request key in `cnf` (matching
    /// `kid`, plus the key itself in `cnf.jwk`). The issuer thereby vouches for
    /// the device key, so any service — not just the one that owns the device
    /// registry — can verify end-user device-signed requests. Without an id_jwt
    /// or an id-token verifier the request key stays unresolved.
    ///
    /// Only reached when NO `DeviceKeyResolver` is wired (see `verify`): a service
    /// that owns a device registry (auth-service) keeps authoritative, live
    /// revocation — its resolver's `None` is final and this fallback never runs.
    ///
    /// REVOCATION CAVEAT (resource services only): this trusts the cnf-bound key
    /// for the id_jwt's full lifetime, so a device/user revoked after mint keeps
    /// working on resource services until the token expires. That is the SAME
    /// trust window resource services already grant the id_jwt's *identity* claims
    /// (they don't reload `user.disabled` either). Bounding that latency (short
    /// id_jwt TTL + refresh, or a polled revocation list) is tracked in
    /// `docs/known-issues.md` — `TODO(revocation)` — and fixes both windows at once.
    async fn resolve_id_token_bound_key(
        &self,
        header: &SignatureHeader,
    ) -> Result<VerifyingKey, AuthError> {
        let unknown = || AuthError::UnknownSigningKey(header.key_id.clone());
        let (Some(verifier), Some(id_jwt)) =
            (self.id_token_verifier.as_ref(), header.id_jwt.as_deref())
        else {
            return Err(unknown());
        };
        verifier
            .validate_bound_request_key(id_jwt, &header.key_id)
            .await?
            .ok_or_else(unknown)
    }

    pub async fn authenticate(
        &self,
        method: &Method,
        uri: &Uri,
        body: &[u8],
        authorization: &str,
    ) -> Result<RequestPrincipal, AuthError> {
        let transport = self.verify(method, uri, body, authorization).await?;
        let user = match (&self.id_token_verifier, transport.id_jwt.as_deref()) {
            (Some(verifier), Some(id_jwt)) => {
                Some(verifier.validate(id_jwt, &transport.key_id).await?)
            }
            _ => None,
        };

        Ok(RequestPrincipal { transport, user })
    }
}
