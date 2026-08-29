//! [`IdTokenVerifier`]: JWKS-backed id_jwt verification — signature, issuer,
//! audience, expiry, and the `cnf.kid` request-key binding — with an
//! in-memory verifying-key cache refreshed on a cache miss.

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::Client;

use super::{
    claims::{DEFAULT_ID_TOKEN_AUDIENCE, ID_TOKEN_AUDIENCE_ENV, IdTokenClaims},
    disclosure::{split_sd_jwt, verify_disclosures},
    jwk::verifying_key_from_jwk,
    principal::UserPrincipal,
    token_parsing::parse_token_parts,
};
use crate::{AuthError, JwksDocument, decode_verifying_key};

#[derive(Clone)]
pub struct IdTokenVerifier {
    issuer: String,
    jwks_url: String,
    audience: String,
    http_client: Client,
    cached_keys: Arc<Mutex<HashMap<String, VerifyingKey>>>,
}

impl IdTokenVerifier {
    pub fn new(issuer: &str, jwks_url: &str, audience: Option<&str>) -> Result<Self, AuthError> {
        crate::ensure_crypto_provider();
        let http_client = Client::builder()
            .timeout(StdDuration::from_secs(5))
            .build()
            .map_err(|error| AuthError::IdTokenJwksUnavailable(error.to_string()))?;
        Ok(Self {
            issuer: issuer.to_string(),
            jwks_url: jwks_url.to_string(),
            audience: audience.unwrap_or(DEFAULT_ID_TOKEN_AUDIENCE).to_string(),
            http_client,
            cached_keys: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn audience_from_env() -> Option<String> {
        env::var(ID_TOKEN_AUDIENCE_ENV).ok()
    }

    pub async fn validate(
        &self,
        token: &str,
        expected_request_key_id: &str,
    ) -> Result<UserPrincipal, AuthError> {
        let (claims, disclosure_strings) =
            self.verified_claims(token, expected_request_key_id).await?;
        let disclosed_claims = verify_disclosures(&claims, &disclosure_strings)?;

        Ok(UserPrincipal {
            user_id: claims.sub,
            audience: claims.aud,
            client_id: claims.azp,
            issued_at: claims.iat,
            expires_at: claims.exp,
            bound_key_id: claims.cnf.kid,
            profile_version: claims.profile_version,
            enrollment_status: claims.enrollment_status,
            kyc_status: claims.kyc_status,
            role: claims.role,
            main_email: claims.main_email,
            main_phone: claims.main_phone,
            main_address: claims.main_address,
            disclosed_claims,
        })
    }

    /// Like [`Self::validate`], but returns the holder's bound public key carried
    /// in `cnf.jwk` (proof-of-possession) instead of a principal. `Ok(None)`
    /// means the token verified but carried no `cnf.jwk` (e.g. a service token),
    /// so the caller should treat the request key as unresolved. Every crypto
    /// check from [`Self::validate`] still applies — the issuer signature, the
    /// audience/issuer/expiry, and `cnf.kid == expected_request_key_id` — so the
    /// returned key is genuinely vouched-for by the issuer for THIS request key.
    pub async fn validate_bound_request_key(
        &self,
        token: &str,
        expected_request_key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError> {
        let (claims, _disclosures) = self.verified_claims(token, expected_request_key_id).await?;
        claims
            .cnf
            .jwk
            .as_ref()
            .map(verifying_key_from_jwk)
            .transpose()
    }

    /// Shared id_jwt verification: parse, check alg/iss/aud/expiry and the
    /// `cnf.kid` request-key binding, then verify the issuer signature against
    /// the JWKS. Returns the validated claims and any SD-JWT disclosure strings.
    async fn verified_claims(
        &self,
        token: &str,
        expected_request_key_id: &str,
    ) -> Result<(IdTokenClaims, Vec<String>), AuthError> {
        let (jwt_compact, disclosure_strings) = split_sd_jwt(token);
        let (header, claims, signing_input, signature) = parse_token_parts(jwt_compact)?;
        if header.alg != "EdDSA" {
            return Err(AuthError::UnsupportedIdTokenAlgorithm(header.alg));
        }
        if claims.iss != self.issuer {
            return Err(AuthError::IdTokenIssuerMismatch);
        }
        if claims.aud != self.audience {
            return Err(AuthError::IdTokenAudienceMismatch);
        }
        if claims.exp <= chrono::Utc::now().timestamp() {
            return Err(AuthError::IdTokenExpired);
        }
        if claims.cnf.kid != expected_request_key_id {
            return Err(AuthError::IdTokenBindingMismatch);
        }

        let verifying_key = self.verifying_key(&header.kid).await?;
        let signature = Signature::from_slice(&signature)
            .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| AuthError::IdTokenVerificationFailed)?;

        Ok((claims, disclosure_strings))
    }

    async fn verifying_key(&self, kid: &str) -> Result<VerifyingKey, AuthError> {
        if let Some(cached) = self
            .cached_keys
            .lock()
            .ok()
            .and_then(|keys| keys.get(kid).copied())
        {
            return Ok(cached);
        }

        self.refresh_keys().await?;
        self.cached_keys
            .lock()
            .map_err(|_| AuthError::IdTokenJwksUnavailable("jwks cache poisoned".to_string()))?
            .get(kid)
            .copied()
            .ok_or_else(|| AuthError::UnknownIdTokenSigningKey(kid.to_string()))
    }

    async fn refresh_keys(&self) -> Result<(), AuthError> {
        let response = self
            .http_client
            .get(&self.jwks_url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|error| AuthError::IdTokenJwksUnavailable(error.to_string()))?;
        if !response.status().is_success() {
            return Err(AuthError::IdTokenJwksUnavailable(format!(
                "jwks endpoint returned {}",
                response.status()
            )));
        }

        let document: JwksDocument = response
            .json()
            .await
            .map_err(|error| AuthError::IdTokenJwksUnavailable(error.to_string()))?;
        let keys = document
            .keys
            .into_iter()
            .filter_map(|jwk| {
                let x = jwk.x?;
                decode_verifying_key(&x)
                    .ok()
                    .map(|verifying_key| (jwk.kid, verifying_key))
            })
            .collect::<HashMap<_, _>>();

        let mut cached = self
            .cached_keys
            .lock()
            .map_err(|_| AuthError::IdTokenJwksUnavailable("jwks cache poisoned".to_string()))?;
        *cached = keys;
        Ok(())
    }
}
