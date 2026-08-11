use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

#[cfg(feature = "axum")]
use axum::{extract::FromRequestParts, http::request::Parts, response::Response};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
#[cfg(feature = "axum")]
use http::StatusCode;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value};
use sha2::{Digest, Sha256};

use crate::{
    AuthError, Confirmation, Jwk, JwksDocument, SignedRequestPrincipal, decode_verifying_key,
};

pub const DEFAULT_ID_TOKEN_AUDIENCE: &str = "cratestack-issued-tokens";
pub const ID_TOKEN_AUDIENCE_ENV: &str = "CRATESTACK_AUTH_ID_TOKEN_AUDIENCE";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct IdTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub azp: String,
    pub iat: i64,
    pub exp: i64,
    /// Unique token identifier (JWT `jti`). The issuer sets a fresh value on
    /// every mint so two tokens with otherwise-identical claims (same
    /// sub/iat/exp — possible when a short-lived id_jwt is refreshed within
    /// the same wall-clock second) are still byte-distinct JWTs. Optional +
    /// `#[serde(default)]` so synthetic/legacy tokens that omit it still
    /// decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    pub cnf: Confirmation,
    #[serde(rename = "mainEmail", skip_serializing_if = "Option::is_none")]
    pub main_email: Option<String>,
    #[serde(rename = "mainPhone", skip_serializing_if = "Option::is_none")]
    pub main_phone: Option<String>,
    #[serde(rename = "mainAddress", skip_serializing_if = "Option::is_none")]
    pub main_address: Option<Value>,
    #[serde(rename = "profileVersion")]
    pub profile_version: i32,
    #[serde(rename = "enrollmentStatus")]
    pub enrollment_status: String,
    #[serde(rename = "kycStatus", skip_serializing_if = "Option::is_none")]
    pub kyc_status: Option<String>,
    /// Server-issued authorization role. Carried as a *verified* claim that
    /// the issuer derives from its own user store — never from a
    /// caller-supplied `client_id`/`azp`. Verifiers gate elevated access on
    /// this claim. Defaults to `"user"` so tokens minted before this claim
    /// existed (and any third-party token without it) are non-privileged.
    #[serde(default = "default_role")]
    pub role: String,
    /// SD-JWT selectively-disclosable claim digests (base64url(SHA-256(disclosure))).
    #[serde(default, rename = "_sd", skip_serializing_if = "Vec::is_empty")]
    pub sd: Vec<String>,
    /// SD-JWT digest hash algorithm — fixed to `sha-256`.
    #[serde(default, rename = "_sd_alg", skip_serializing_if = "Option::is_none")]
    pub sd_alg: Option<String>,
}

pub struct IdTokenClaimsParams<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub subject: &'a str,
    pub bound_key_id: &'a str,
    /// The holder's bound public key as an OKP/Ed25519 JWK, embedded in `cnf.jwk`
    /// so JWKS-verifying services can check device-signed requests without a
    /// device-key registry. Pass `None` for service-to-service tokens whose
    /// `kid` is resolvable via static trust / JWKS.
    pub bound_key_jwk: Option<Jwk>,
    pub profile_version: i32,
    pub enrollment_status: &'a str,
    pub kyc_status: Option<String>,
    pub main_email: Option<String>,
    pub main_phone: Option<String>,
    pub main_address: Option<Value>,
    /// SD-JWT selectively-disclosable claims to attach as `_sd` digests in the issued
    /// token. Each disclosure carries its own salt so distinct holders don't share
    /// digests for the same claim/value pair.
    pub disclosures: Vec<DisclosureClaim>,
}

/// A claim that the issuer adds to the SD-JWT as a digest in `_sd[]` and ships out
/// alongside the JWT in the compact form `<jwt>~<disclosure_b64>~`.
#[derive(Clone, Debug)]
pub struct DisclosureClaim {
    pub name: String,
    pub value: Value,
}

/// Serialized output of [`issue_sd_id_token`]: the SD-JWT compact form (`<jwt>~d1~d2~`)
/// plus the parsed disclosure strings (so the caller can persist them with the
/// issued token if it wants to reissue without re-running the cuid generator).
#[derive(Clone, Debug)]
pub struct IssuedSdIdToken {
    pub compact: String,
    pub jwt: String,
    pub disclosures: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JwtHeader {
    alg: String,
    typ: String,
    kid: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserPrincipal {
    pub user_id: String,
    pub audience: String,
    pub client_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub bound_key_id: String,
    pub profile_version: i32,
    pub enrollment_status: String,
    pub kyc_status: Option<String>,
    /// Verified authorization role from the id_jwt `role` claim. Source of
    /// truth for privileged-access gating across services.
    pub role: String,
    pub main_email: Option<String>,
    pub main_phone: Option<String>,
    pub main_address: Option<Value>,
    /// Claims the holder disclosed alongside this token. Only populated when the
    /// presented compact form was an SD-JWT (`<jwt>~d1~d2~`) and each disclosure
    /// hashed back to a digest in `_sd[]`.
    pub disclosed_claims: HashMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RequestPrincipal {
    pub transport: SignedRequestPrincipal,
    pub user: Option<UserPrincipal>,
}

#[derive(Clone, Debug)]
pub struct CurrentPrincipal(pub RequestPrincipal);

#[derive(Clone, Debug)]
pub struct AuthenticatedPrincipal(pub RequestPrincipal);

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

impl CurrentPrincipal {
    pub fn user(&self) -> Option<&UserPrincipal> {
        self.0.user.as_ref()
    }
}

#[cfg(feature = "axum")]
impl<S> FromRequestParts<S> for CurrentPrincipal
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RequestPrincipal>()
            .cloned()
            .map(Self)
            .ok_or_else(|| {
                principal_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "principal_unavailable",
                    "Request principal was not installed by authentication middleware",
                )
            })
    }
}

#[cfg(feature = "axum")]
impl<S> FromRequestParts<S> for AuthenticatedPrincipal
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let principal = CurrentPrincipal::from_request_parts(parts, state).await?;
        if principal.0.user.is_none() {
            return Err(principal_error_response(
                StatusCode::UNAUTHORIZED,
                "authenticated_principal_required",
                "Protected endpoint requires a validated id_jwt bound to the request signature key",
            ));
        }

        Ok(Self(principal.0))
    }
}

pub fn issue_id_token(
    signing_key: &SigningKey,
    issuer_kid: &str,
    claims: &IdTokenClaims,
) -> Result<String, AuthError> {
    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        typ: "JWT".to_string(),
        kid: issuer_kid.to_string(),
    };
    let encoded_header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&header)
            .map_err(|error| AuthError::IdTokenEncoding(error.to_string()))?,
    );
    let encoded_claims = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(claims)
            .map_err(|error| AuthError::IdTokenEncoding(error.to_string()))?,
    );
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// Issue an SD-JWT: a regular JWT whose `_sd[]` array carries digests of the supplied
/// disclosures, plus the disclosure strings appended in the compact form
/// `<jwt>~<disclosure1>~...~`. Holders forward selected disclosures to verifiers via
/// the `id_jwt` Authorization parameter; verifiers recompute digests and look them up
/// in `_sd[]` to recover claim values.
pub fn issue_sd_id_token(
    signing_key: &SigningKey,
    issuer_kid: &str,
    base_claims: &IdTokenClaims,
    disclosures: &[DisclosureClaim],
) -> Result<IssuedSdIdToken, AuthError> {
    let mut claims = base_claims.clone();
    let mut disclosure_strings: Vec<String> = Vec::with_capacity(disclosures.len());
    let mut digests: Vec<String> = claims.sd.clone();
    for disclosure in disclosures {
        let salt = cuid2::create_id();
        let array = serde_json::to_vec(&serde_json::json!([
            salt,
            disclosure.name,
            disclosure.value
        ]))
        .map_err(|error| AuthError::IdTokenEncoding(error.to_string()))?;
        let encoded = URL_SAFE_NO_PAD.encode(array);
        digests.push(disclosure_digest(&encoded));
        disclosure_strings.push(encoded);
    }
    if !disclosures.is_empty() {
        claims.sd = digests;
        claims.sd_alg.get_or_insert_with(|| "sha-256".to_owned());
    }

    let jwt = issue_id_token(signing_key, issuer_kid, &claims)?;
    let mut compact = jwt.clone();
    for disclosure in &disclosure_strings {
        compact.push('~');
        compact.push_str(disclosure);
    }
    if !disclosure_strings.is_empty() {
        compact.push('~');
    }

    Ok(IssuedSdIdToken {
        compact,
        jwt,
        disclosures: disclosure_strings,
    })
}

pub fn decode_id_token_claims_unverified(token: &str) -> Result<IdTokenClaims, AuthError> {
    let (jwt_compact, _) = split_sd_jwt(token);
    let (_, claims, _, _) = parse_token_parts(jwt_compact)?;
    Ok(claims)
}

/// Decode the disclosure strings appended to an SD-JWT compact form. Each result is
/// the `(claim_name, claim_value)` pair embedded in the disclosure. Any disclosure
/// that doesn't decode is dropped silently — verification is the consumer's job.
pub fn decode_disclosures_unverified(token: &str) -> Vec<(String, Value)> {
    let (_, disclosures) = split_sd_jwt(token);
    disclosures
        .into_iter()
        .filter_map(|encoded| parse_disclosure_string(&encoded).ok())
        .collect()
}

pub fn issuer_jwk(signing_key: &SigningKey, kid: &str) -> Jwk {
    let verifying_key = signing_key.verifying_key();
    Jwk {
        kty: "OKP".to_string(),
        kid: kid.to_string(),
        alg: "EdDSA".to_string(),
        key_use: "sig".to_string(),
        crv: Some("Ed25519".to_string()),
        x: Some(URL_SAFE_NO_PAD.encode(verifying_key.as_bytes())),
    }
}

/// Build an OKP/Ed25519 JWK for a verifying (public) key. Used to embed a
/// holder's device key in an id_jwt `cnf.jwk` so JWKS-verifying services can
/// check device-signed requests without their own device-key registry.
pub fn verifying_key_jwk(verifying_key: &VerifyingKey, kid: &str) -> Jwk {
    Jwk {
        kty: "OKP".to_string(),
        kid: kid.to_string(),
        alg: "EdDSA".to_string(),
        key_use: "sig".to_string(),
        crv: Some("Ed25519".to_string()),
        x: Some(URL_SAFE_NO_PAD.encode(verifying_key.as_bytes())),
    }
}

/// Recover the Ed25519 public key from an OKP JWK (the inverse of
/// [`verifying_key_jwk`]). Rejects non-OKP / non-Ed25519 JWKs and a missing `x`.
pub fn verifying_key_from_jwk(jwk: &Jwk) -> Result<VerifyingKey, AuthError> {
    if jwk.kty != "OKP" {
        return Err(AuthError::InvalidPublicKey(format!(
            "unsupported cnf jwk kty: {}",
            jwk.kty
        )));
    }
    if jwk.crv.as_deref() != Some("Ed25519") {
        return Err(AuthError::InvalidPublicKey(format!(
            "unsupported cnf jwk crv: {:?}",
            jwk.crv
        )));
    }
    let x = jwk
        .x
        .as_deref()
        .ok_or_else(|| AuthError::InvalidPublicKey("cnf jwk missing x".to_string()))?;
    decode_verifying_key(x)
}

pub fn encode_signing_key(signing_key: &SigningKey) -> String {
    URL_SAFE_NO_PAD.encode(signing_key.to_bytes())
}

pub fn decode_signing_key(encoded: &str) -> Result<SigningKey, AuthError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AuthError::IdTokenDecoding("invalid signing key length".to_string()))?;
    Ok(SigningKey::from_bytes(&bytes))
}

pub fn default_id_token_claims(params: IdTokenClaimsParams<'_>) -> IdTokenClaims {
    let iat = chrono::Utc::now();
    // Placeholder `exp` only. The real issuer OVERWRITES `claims.exp` with
    // the policy TTL (short-lived, refreshed) before signing. This long
    // default exists so synthetic tokens minted directly in tests/harnesses
    // (which never go through a real issuer) stay valid for the duration of
    // a test run.
    let exp = iat + chrono::Duration::days(365);
    IdTokenClaims {
        iss: params.issuer.to_string(),
        sub: params.subject.to_string(),
        aud: DEFAULT_ID_TOKEN_AUDIENCE.to_string(),
        azp: params.client_id.to_string(),
        iat: iat.timestamp(),
        exp: exp.timestamp(),
        // No `jti` by default; a real issuer stamps a fresh one per mint.
        // Synthetic tokens built directly in tests don't need it.
        jti: None,
        cnf: Confirmation {
            kid: params.bound_key_id.to_string(),
            jwk: params.bound_key_jwk,
        },
        main_email: params.main_email,
        main_phone: params.main_phone,
        main_address: params.main_address,
        profile_version: params.profile_version,
        enrollment_status: params.enrollment_status.to_string(),
        kyc_status: params.kyc_status,
        // Default to the non-privileged role. A real issuer overwrites this
        // with the server-derived role; every other construction site
        // (tests, fixtures) gets a plain user token.
        role: default_role(),
        sd: Vec::new(),
        sd_alg: None,
    }
}

/// The non-privileged default role. Used by serde as the `role` claim default
/// (absent claim → `"user"`) and by [`default_id_token_claims`].
fn default_role() -> String {
    "user".to_owned()
}

/// Returns the issuer's view of the disclosures from `params`. Useful when callers want
/// to construct claims independently from the disclosure list (`default_id_token_claims`
/// always returns an empty `_sd[]` because the digests are filled in by
/// `issue_sd_id_token`).
pub fn take_disclosures(params: IdTokenClaimsParams<'_>) -> (IdTokenClaims, Vec<DisclosureClaim>) {
    let disclosures = params.disclosures;
    let claims = default_id_token_claims(IdTokenClaimsParams {
        disclosures: Vec::new(),
        ..params
    });
    (claims, disclosures)
}

fn split_sd_jwt(token: &str) -> (&str, Vec<String>) {
    let mut iter = token.split('~');
    let jwt = iter.next().unwrap_or(token);
    let disclosures: Vec<String> = iter
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();
    (jwt, disclosures)
}

fn disclosure_digest(disclosure_string: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(disclosure_string.as_bytes()))
}

fn parse_disclosure_string(encoded: &str) -> Result<(String, Value), AuthError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|error| AuthError::IdTokenDecoding(format!("disclosure decode: {error}")))?;
    let array: Vec<Value> = serde_json::from_slice(&bytes)
        .map_err(|error| AuthError::IdTokenDecoding(format!("disclosure parse: {error}")))?;
    if array.len() != 3 {
        return Err(AuthError::IdTokenDecoding(
            "disclosure must be a [salt, name, value] triple".to_owned(),
        ));
    }
    let mut iter = array.into_iter();
    let _salt = iter.next();
    let name = iter
        .next()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| AuthError::IdTokenDecoding("disclosure name must be a string".to_owned()))?;
    let value = iter.next().unwrap_or(Value::Null);
    Ok((name, value))
}

fn verify_disclosures(
    claims: &IdTokenClaims,
    disclosure_strings: &[String],
) -> Result<HashMap<String, Value>, AuthError> {
    if disclosure_strings.is_empty() {
        return Ok(HashMap::new());
    }
    let mut digests: JsonMap<String, Value> = JsonMap::new();
    for digest in &claims.sd {
        digests.insert(digest.clone(), Value::Null);
    }
    let mut disclosed: HashMap<String, Value> = HashMap::new();
    for encoded in disclosure_strings {
        let digest = disclosure_digest(encoded);
        if !digests.contains_key(&digest) {
            return Err(AuthError::IdTokenDecoding(
                "disclosure digest not present in token _sd[]".to_owned(),
            ));
        }
        let (name, value) = parse_disclosure_string(encoded)?;
        disclosed.insert(name, value);
    }
    Ok(disclosed)
}

fn parse_token_parts(
    token: &str,
) -> Result<(JwtHeader, IdTokenClaims, String, Vec<u8>), AuthError> {
    let mut parts = token.split('.');
    let encoded_header = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing jwt header".to_string()))?;
    let encoded_payload = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing jwt payload".to_string()))?;
    let encoded_signature = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing jwt signature".to_string()))?;
    if parts.next().is_some() {
        return Err(AuthError::IdTokenDecoding(
            "jwt compact form must contain exactly three parts".to_string(),
        ));
    }

    let header: JwtHeader = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(encoded_header)
            .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?,
    )
    .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
    let claims: IdTokenClaims = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(encoded_payload)
            .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?,
    )
    .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;

    Ok((
        header,
        claims,
        format!("{encoded_header}.{encoded_payload}"),
        signature,
    ))
}

#[cfg(feature = "axum")]
fn principal_error_response(status: StatusCode, code: &str, message: &str) -> Response {
    crate::response::error_response(status, code, message)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    use super::{
        DEFAULT_ID_TOKEN_AUDIENCE, IdTokenClaimsParams, IdTokenVerifier,
        decode_id_token_claims_unverified, default_id_token_claims, issue_id_token, issuer_jwk,
        verifying_key_from_jwk, verifying_key_jwk,
    };
    use crate::{AuthError, TokenResponse};

    const TEST_ISSUER_SIGNING_KID: &str = "issuer-dev-key-1";

    fn test_issuer_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[
            0x6d, 0x01, 0x97, 0x4a, 0x39, 0x8c, 0x27, 0x7c, 0xc0, 0x2d, 0xb4, 0x51, 0x6d, 0x89,
            0xa4, 0x1f, 0x38, 0x21, 0xb6, 0xde, 0x74, 0xd9, 0x41, 0x20, 0x7a, 0xcf, 0x10, 0x63,
            0xf4, 0x9b, 0x8d, 0x29,
        ])
    }

    fn test_issuer_jwk() -> crate::Jwk {
        issuer_jwk(&test_issuer_signing_key(), TEST_ISSUER_SIGNING_KID)
    }

    fn issue_token_pair(claims: super::IdTokenClaims) -> Result<TokenResponse, AuthError> {
        let id_jwt = issue_id_token(&test_issuer_signing_key(), TEST_ISSUER_SIGNING_KID, &claims)?;
        Ok(TokenResponse {
            token_type: "N_A".to_string(),
            issued_token_type: "urn:ietf:params:oauth:token-type:jwt".to_string(),
            id_jwt,
            expires_in: chrono::Duration::days(365).num_seconds(),
            refresh_token: format!("refresh_{}", cuid2::create_id()),
            cnf: claims.cnf,
        })
    }

    #[test]
    fn issues_signed_id_jwt_tokens() {
        let claims = default_id_token_claims(IdTokenClaimsParams {
            issuer: "https://issuer.example",
            client_id: "example-client",
            subject: "usr_123",
            bound_key_id: "vk_123",
            bound_key_jwk: None,
            profile_version: 7,
            enrollment_status: "enrolled",
            kyc_status: Some("approved".to_string()),
            main_email: Some("user@example.com".to_string()),
            main_phone: None,
            main_address: Some(json!({ "country": "CM" })),
            disclosures: Vec::new(),
        });

        let issued = issue_token_pair(claims.clone()).expect("token pair should issue");
        assert_eq!(issued.cnf.kid, "vk_123");
        assert_eq!(issued.id_jwt.split('.').count(), 3);

        let decoded = decode_id_token_claims_unverified(&issued.id_jwt)
            .expect("jwt claims should decode without verification");
        assert_eq!(decoded, claims);
    }

    #[test]
    fn publishes_full_dev_issuer_jwk() {
        let jwk = test_issuer_jwk();
        assert_eq!(jwk.kid, TEST_ISSUER_SIGNING_KID);
        assert_eq!(jwk.kty, "OKP");
        assert_eq!(jwk.crv.as_deref(), Some("Ed25519"));
        assert!(jwk.x.is_some());
    }

    #[tokio::test]
    async fn validates_signed_id_tokens_against_jwks() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("jwks test listener should bind");
        let addr = listener
            .local_addr()
            .expect("jwks test addr should resolve");
        let jwks = crate::jwks(vec![test_issuer_jwk()]);
        let router = axum::Router::new().route(
            "/jwks.json",
            axum::routing::get(move || {
                let jwks = jwks.clone();
                async move { axum::Json(jwks) }
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let claims = default_id_token_claims(IdTokenClaimsParams {
            issuer: "http://127.0.0.1:8081",
            client_id: "example-client",
            subject: "usr_456",
            bound_key_id: "vk_bound",
            bound_key_jwk: None,
            profile_version: 3,
            enrollment_status: "enrolled",
            kyc_status: Some("approved".to_string()),
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosures: Vec::new(),
        });
        let issued = issue_token_pair(claims).expect("token pair should issue");
        let verifier = IdTokenVerifier::new(
            "http://127.0.0.1:8081",
            &format!("http://{addr}/jwks.json"),
            Some("cratestack-issued-tokens"),
        )
        .expect("id token verifier should build");

        let principal = verifier
            .validate(&issued.id_jwt, "vk_bound")
            .await
            .expect("id token should validate");
        assert_eq!(principal.user_id, "usr_456");
        assert_eq!(principal.bound_key_id, "vk_bound");

        server.abort();
    }

    #[test]
    fn verifying_key_jwk_roundtrips() {
        let key = SigningKey::from_bytes(&[9u8; 32]);
        let jwk = verifying_key_jwk(&key.verifying_key(), "vk_device");
        assert_eq!(jwk.kty, "OKP");
        assert_eq!(jwk.crv.as_deref(), Some("Ed25519"));
        assert_eq!(jwk.kid, "vk_device");
        let recovered = verifying_key_from_jwk(&jwk).expect("jwk should decode");
        assert_eq!(recovered, key.verifying_key());

        // Wrong curve / missing x are rejected.
        let mut bad_crv = jwk.clone();
        bad_crv.crv = Some("P-256".to_string());
        assert!(matches!(
            verifying_key_from_jwk(&bad_crv),
            Err(AuthError::InvalidPublicKey(_))
        ));
        let mut no_x = jwk.clone();
        no_x.x = None;
        assert!(matches!(
            verifying_key_from_jwk(&no_x),
            Err(AuthError::InvalidPublicKey(_))
        ));
    }

    #[tokio::test]
    async fn bound_request_key_resolves_device_key_from_cnf() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("jwks test listener should bind");
        let addr = listener
            .local_addr()
            .expect("jwks test addr should resolve");
        let jwks = crate::jwks(vec![test_issuer_jwk()]);
        let router = axum::Router::new().route(
            "/jwks.json",
            axum::routing::get(move || {
                let jwks = jwks.clone();
                async move { axum::Json(jwks) }
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let device_key = SigningKey::from_bytes(&[11u8; 32]);
        let device_jwk = verifying_key_jwk(&device_key.verifying_key(), "vk_device");
        let claims = default_id_token_claims(IdTokenClaimsParams {
            issuer: "http://127.0.0.1:8081",
            client_id: "example-client",
            subject: "usr_device",
            bound_key_id: "vk_device",
            bound_key_jwk: Some(device_jwk),
            profile_version: 1,
            enrollment_status: "enrolled",
            kyc_status: None,
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosures: Vec::new(),
        });
        let issued = issue_token_pair(claims).expect("token pair should issue");
        let verifier = IdTokenVerifier::new(
            "http://127.0.0.1:8081",
            &format!("http://{addr}/jwks.json"),
            Some(DEFAULT_ID_TOKEN_AUDIENCE),
        )
        .expect("id token verifier should build");

        // A JWKS-verified id_jwt whose cnf binds vk_device yields that device key.
        let resolved = verifier
            .validate_bound_request_key(&issued.id_jwt, "vk_device")
            .await
            .expect("bound key resolution should succeed")
            .expect("cnf.jwk should be present");
        assert_eq!(resolved, device_key.verifying_key());

        // The cnf binding is enforced: asking for a different request key fails.
        assert!(matches!(
            verifier
                .validate_bound_request_key(&issued.id_jwt, "vk_other")
                .await,
            Err(AuthError::IdTokenBindingMismatch)
        ));

        // A token without cnf.jwk verifies but resolves no key.
        let no_jwk_claims = default_id_token_claims(IdTokenClaimsParams {
            issuer: "http://127.0.0.1:8081",
            client_id: "example-client",
            subject: "usr_device",
            bound_key_id: "vk_service",
            bound_key_jwk: None,
            profile_version: 1,
            enrollment_status: "enrolled",
            kyc_status: None,
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosures: Vec::new(),
        });
        let no_jwk = issue_token_pair(no_jwk_claims).expect("token pair should issue");
        assert!(
            verifier
                .validate_bound_request_key(&no_jwk.id_jwt, "vk_service")
                .await
                .expect("verification should succeed")
                .is_none()
        );

        server.abort();
    }
}
