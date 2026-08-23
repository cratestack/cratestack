//! Per-service signing identity + multi-issuer verification.
//!
//! ## Why this module exists
//!
//! A single-issuer setup gives one service (an identity/auth service, say)
//! the *only* signing identity in the system: it issues `id_token`s and
//! publishes its public key at `/jwks.json`. Every other service that needs
//! to authenticate a user fetches that JWKS to verify id-token signatures
//! (and the SD-JWT disclosures stitched onto them).
//!
//! A signed-upload-ticket pattern needs the same shape, but issued by the
//! *owning* service for an asset (a vendor/catalog/order service, say) so a
//! generic upload handler can stay domain-naive while still enforcing
//! per-asset ACLs. Rather than build a parallel "upload-tickets-only"
//! mechanism, this module generalises the single-issuer pattern so any
//! backend service can become a JWT issuer.
//!
//! Other near-term consumers of the same plumbing:
//!
//! * **s2s request signing rotation.** Receivers can JWKS-fetch the
//!   sender's verifying key by `kid` instead of carrying public keys
//!   in env config.
//! * **Partner webhook signing.** When delivery-gateway spins off as
//!   the opaque 3rd-party API, partners verify webhook signatures via
//!   `delivery-gateway./jwks.json`.
//! * **Cross-service async events.** Producers sign events with their
//!   service key; consumers verify via JWKS — same code path.
//! * **Per-service scoped tokens.** Anything narrow (a vendor-service
//!   "private preview" token, a catalog "fast-search" token, etc.)
//!   piggy-backs on the same signing identity.
//!
//! ## Surface
//!
//! * [`ServiceSigningKey`] — load-or-mint persistent Ed25519 identity
//!   for a service.
//! * [`mint_signed_token`] — generic `JWT(claims)` minter for any
//!   serde-serializable claim shape.
//! * [`MultiIssuerJwksVerifier`] — fetches + caches JWKS per trusted
//!   issuer, verifies signatures, returns deserialised claims.
//! * [`jwks_router`] — mountable axum router that serves a service's
//!   public key at `/jwks.json` (and `/.well-known/jwks.json` for
//!   discovery convention).
//!
//! Claim-shape-specific validation (audience, scope, expiry slack,
//! nonce consumption, ...) stays with the consumer — this module
//! only owns the signature/exp envelope.

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

#[cfg(feature = "axum")]
use axum::{Router, http::header, response::IntoResponse, routing::get};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{AuthError, Jwk, JwksDocument, decode_signing_key, decode_verifying_key, issuer_jwk};

// ───────────────────────────────────────────────────────────────────
// ServiceSigningKey
// ───────────────────────────────────────────────────────────────────

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
        rand::rngs::OsRng.fill_bytes(&mut secret);
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

// ───────────────────────────────────────────────────────────────────
// Generic JWT minter
// ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JwtHeader {
    alg: String,
    typ: String,
    kid: String,
}

/// Mint a compact-form Ed25519 JWT for arbitrary serializable
/// claims. The header is fixed to `{alg: "EdDSA", typ: "JWT", kid}`;
/// the consumer's job is to populate envelope claims (`iss`, `iat`,
/// `exp`, `sub`, ...) inside `C`.
///
/// Use [`ServiceSigningKey::mint`] in service code; this free
/// function is exposed for tests and for callers that hold a raw
/// [`SigningKey`].
pub fn mint_signed_token<C: Serialize>(
    signing_key: &SigningKey,
    kid: &str,
    claims: &C,
) -> Result<String, AuthError> {
    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        typ: "JWT".to_string(),
        kid: kid.to_string(),
    };
    let encoded_header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&header).map_err(|err| AuthError::IdTokenEncoding(err.to_string()))?,
    );
    let encoded_claims = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(claims).map_err(|err| AuthError::IdTokenEncoding(err.to_string()))?,
    );
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

// ───────────────────────────────────────────────────────────────────
// MultiIssuerJwksVerifier
// ───────────────────────────────────────────────────────────────────

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
    inner: Arc<VerifierInner>,
}

struct VerifierInner {
    issuers: HashMap<String, IssuerEntry>,
    http_client: Client,
}

struct IssuerEntry {
    jwks_url: String,
    cached_keys: Mutex<HashMap<String, VerifyingKey>>,
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

/// Minimal envelope claims any signed JWT issued via this kit must
/// carry. Used only for the verifier's pre-deserialisation peek;
/// the consumer's own claim struct can include additional fields.
#[derive(Clone, Debug, Deserialize)]
struct EnvelopeClaims {
    iss: String,
    exp: i64,
    #[serde(default)]
    iat: Option<i64>,
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

    /// Verify a compact-form JWT. Checks signature against the
    /// issuer's JWKS, that `iss` is in the trust list, and that
    /// `exp` is in the future. Claim-specific validation (audience,
    /// scope, nonce, ...) is the consumer's job.
    pub async fn verify<C: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedToken<C>, AuthError> {
        let (header, claims_value, signing_input, signature_bytes) = parse_jwt_parts(token)?;
        if header.alg != "EdDSA" {
            return Err(AuthError::UnsupportedIdTokenAlgorithm(header.alg));
        }

        let envelope: EnvelopeClaims = serde_json::from_value(claims_value.clone())
            .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

        let issuer_entry = self
            .inner
            .issuers
            .get(&envelope.iss)
            .ok_or_else(|| AuthError::UntrustedIssuer(envelope.iss.clone()))?;

        if envelope.exp <= chrono::Utc::now().timestamp() {
            return Err(AuthError::IdTokenExpired);
        }

        let verifying_key = self
            .verifying_key_for(&envelope.iss, issuer_entry, &header.kid)
            .await?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| AuthError::IdTokenVerificationFailed)?;

        let claims: C = serde_json::from_value(claims_value)
            .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

        Ok(VerifiedToken {
            issuer: envelope.iss,
            kid: header.kid,
            expires_at: envelope.exp,
            issued_at: envelope.iat,
            claims,
        })
    }

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

    async fn verifying_key_for(
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

// ───────────────────────────────────────────────────────────────────
// JWKS router
// ───────────────────────────────────────────────────────────────────

/// Mountable axum router that serves the given JWKS document at
/// `/jwks.json` and `/.well-known/jwks.json`.
///
/// Service binaries typically:
///
/// ```text
/// let signing_key = ServiceSigningKey::from_env(
///     "vendor-service",
///     "vendor-service-v1",
///     "MY_SERVICE_SIGNING_KEY",
/// )?;
/// let app = Router::new()
///     .merge(jwks_router(signing_key.jwks_document()))
///     .route("/healthz", get(healthz));
/// ```
///
/// The `JwksDocument` is captured by value at mount time, so on
/// rotation you'd either rebuild the router or merge a router that
/// reads JWKS from a shared `Arc<RwLock<JwksDocument>>` — the
/// helper covers the static-document case, which is what every
/// service needs in steady state.
///
/// Gated behind this crate's `axum` Cargo feature (default-on) — see
/// the crate root doc comment.
#[cfg(feature = "axum")]
pub fn jwks_router(document: JwksDocument) -> Router {
    let document = Arc::new(document);
    let document_alt = document.clone();
    Router::new()
        .route(
            "/jwks.json",
            get({
                let document = document.clone();
                move || serve_jwks(document.clone())
            }),
        )
        .route(
            "/.well-known/jwks.json",
            get({
                let document = document_alt.clone();
                move || serve_jwks(document.clone())
            }),
        )
}

#[cfg(feature = "axum")]
async fn serve_jwks(document: Arc<JwksDocument>) -> impl IntoResponse {
    let body = serde_json::to_vec(&*document).unwrap_or_else(|_| b"{\"keys\":[]}".to_vec());
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
}

// ───────────────────────────────────────────────────────────────────
// Internal: minimal JWT compact-form parser
// ───────────────────────────────────────────────────────────────────

fn parse_jwt_parts(token: &str) -> Result<(JwtHeader, Value, String, Vec<u8>), AuthError> {
    let mut parts = token.splitn(3, '.');
    let encoded_header = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing header".to_string()))?;
    let encoded_claims = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing claims".to_string()))?;
    let encoded_signature = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing signature".to_string()))?;
    if parts.next().is_some() {
        return Err(AuthError::IdTokenDecoding(
            "unexpected extra segments".to_string(),
        ));
    }

    let header_bytes = URL_SAFE_NO_PAD
        .decode(encoded_header)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;
    let header: JwtHeader = serde_json::from_slice(&header_bytes)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(encoded_claims)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;
    let claims: Value = serde_json::from_slice(&claims_bytes)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

    let signature_bytes = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

    let signing_input = format!("{encoded_header}.{encoded_claims}");
    Ok((header, claims, signing_input, signature_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "axum")]
    use axum::body::to_bytes;
    use ed25519_dalek::SigningKey;
    use serde::{Deserialize, Serialize};
    #[cfg(feature = "axum")]
    use tower::ServiceExt;

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    struct UploadTicketClaims {
        iss: String,
        sub: String,
        iat: i64,
        exp: i64,
        owner_type: String,
        owner_id: String,
        purpose: String,
        nonce: String,
    }

    fn fixture_signing_key() -> SigningKey {
        // Deterministic test key — same across runs.
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn future_exp() -> i64 {
        chrono::Utc::now().timestamp() + 300
    }

    fn past_exp() -> i64 {
        chrono::Utc::now().timestamp() - 300
    }

    #[test]
    fn ephemeral_signing_key_round_trips_through_mint() {
        let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
        let claims = UploadTicketClaims {
            iss: "vendor-service".into(),
            sub: "user_123".into(),
            iat: chrono::Utc::now().timestamp(),
            exp: future_exp(),
            owner_type: "vendor".into(),
            owner_id: "vnd_1".into(),
            purpose: "vendor_logo".into(),
            nonce: "n1".into(),
        };
        let token = key.mint(&claims).expect("mint");
        // Compact form: three segments separated by '.'.
        assert_eq!(token.matches('.').count(), 2);
    }

    #[test]
    fn from_env_returns_missing_signing_key_env_when_unset() {
        // Use a name that's exceedingly unlikely to be set in CI.
        // We pattern-match instead of `expect_err` so ServiceSigningKey
        // doesn't have to derive `Debug` (which would leak key bytes).
        match ServiceSigningKey::from_env(
            "vendor-service",
            "vendor-service-test",
            "CRATESTACK_AUTH_TEST_DEFINITELY_NOT_SET_KEY_a8c8e",
        ) {
            Ok(_) => panic!("from_env must error when its env var is unset"),
            Err(err) => {
                assert!(matches!(err, AuthError::MissingSigningKeyEnv(_)));
            }
        }
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn jwks_router_serves_the_configured_keyset() {
        let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
        let router = jwks_router(key.jwks_document());

        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/jwks.json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let document: JwksDocument = serde_json::from_slice(&body).unwrap();
        assert_eq!(document.keys.len(), 1);
        assert_eq!(document.keys[0].kid, "vendor-service-test");

        let well_known = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/.well-known/jwks.json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(well_known.status(), 200);
    }

    #[tokio::test]
    async fn verifier_rejects_token_from_untrusted_issuer() {
        let key = ServiceSigningKey::new(
            "vendor-service",
            "vendor-service-test",
            fixture_signing_key(),
        );
        let claims = UploadTicketClaims {
            iss: "vendor-service".into(),
            sub: "user_123".into(),
            iat: chrono::Utc::now().timestamp(),
            exp: future_exp(),
            owner_type: "vendor".into(),
            owner_id: "vnd_1".into(),
            purpose: "vendor_logo".into(),
            nonce: "n1".into(),
        };
        let token = key.mint(&claims).unwrap();

        // Trust list does NOT include vendor-service.
        let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
            "catalog-service".to_string(),
            "http://127.0.0.1:0/jwks.json".to_string(),
        )]))
        .unwrap();

        let err = verifier
            .verify::<UploadTicketClaims>(&token)
            .await
            .expect_err("must reject untrusted issuer");
        assert!(matches!(err, AuthError::UntrustedIssuer(ref iss) if iss == "vendor-service"));
    }

    #[tokio::test]
    async fn verifier_rejects_expired_token() {
        let key = ServiceSigningKey::new(
            "vendor-service",
            "vendor-service-test",
            fixture_signing_key(),
        );
        let claims = UploadTicketClaims {
            iss: "vendor-service".into(),
            sub: "user_123".into(),
            iat: chrono::Utc::now().timestamp() - 600,
            exp: past_exp(),
            owner_type: "vendor".into(),
            owner_id: "vnd_1".into(),
            purpose: "vendor_logo".into(),
            nonce: "n1".into(),
        };
        let token = key.mint(&claims).unwrap();

        // Even the trusted path rejects expired.
        let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
            "vendor-service".to_string(),
            "http://127.0.0.1:0/jwks.json".to_string(),
        )]))
        .unwrap();

        let err = verifier
            .verify::<UploadTicketClaims>(&token)
            .await
            .expect_err("must reject expired token");
        assert!(matches!(err, AuthError::IdTokenExpired));
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn end_to_end_mint_and_verify_via_jwks_router() {
        // Fire up the JWKS router on an ephemeral port and verify
        // a token end-to-end against the live HTTP JWKS.
        let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
        let app = jwks_router(key.jwks_document());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let claims = UploadTicketClaims {
            iss: "vendor-service".into(),
            sub: "user_123".into(),
            iat: chrono::Utc::now().timestamp(),
            exp: future_exp(),
            owner_type: "vendor".into(),
            owner_id: "vnd_1".into(),
            purpose: "vendor_logo".into(),
            nonce: "n1".into(),
        };
        let token = key.mint(&claims).unwrap();

        let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
            "vendor-service".to_string(),
            format!("http://{addr}/jwks.json"),
        )]))
        .unwrap();

        let verified = verifier
            .verify::<UploadTicketClaims>(&token)
            .await
            .expect("must verify");
        assert_eq!(verified.issuer, "vendor-service");
        assert_eq!(verified.kid, "vendor-service-test");
        assert_eq!(verified.claims, claims);

        // A second verification reuses the cached JWKS — no HTTP
        // call. We can't directly assert "no HTTP" without a
        // recording client, but at minimum it must still pass.
        let _ = verifier
            .verify::<UploadTicketClaims>(&token)
            .await
            .expect("cached verify");

        server.abort();
    }

    #[cfg(feature = "axum")]
    #[tokio::test]
    async fn verifier_rejects_token_with_tampered_payload() {
        let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
        let app = jwks_router(key.jwks_document());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let claims = UploadTicketClaims {
            iss: "vendor-service".into(),
            sub: "user_123".into(),
            iat: chrono::Utc::now().timestamp(),
            exp: future_exp(),
            owner_type: "vendor".into(),
            owner_id: "vnd_1".into(),
            purpose: "vendor_logo".into(),
            nonce: "n1".into(),
        };
        let token = key.mint(&claims).unwrap();
        // Replace the middle segment (claims) with another
        // arbitrary base64 — signature now mismatches.
        let mut parts: Vec<&str> = token.split('.').collect();
        let evil_claims = URL_SAFE_NO_PAD
            .encode(b"{\"iss\":\"vendor-service\",\"exp\":99999999999,\"sub\":\"attacker\"}");
        parts[1] = &evil_claims;
        let tampered = parts.join(".");

        let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
            "vendor-service".to_string(),
            format!("http://{addr}/jwks.json"),
        )]))
        .unwrap();

        let err = verifier
            .verify::<UploadTicketClaims>(&tampered)
            .await
            .expect_err("must reject tampered payload");
        assert!(matches!(err, AuthError::IdTokenVerificationFailed));

        server.abort();
    }

    #[test]
    fn trusted_issuers_are_alphabetised() {
        let verifier = MultiIssuerJwksVerifier::new(HashMap::from([
            ("vendor-service".to_string(), "x".to_string()),
            ("catalog-service".to_string(), "y".to_string()),
            ("order-service".to_string(), "z".to_string()),
        ]))
        .unwrap();
        assert_eq!(
            verifier.trusted_issuers(),
            vec!["catalog-service", "order-service", "vendor-service"],
        );
    }
}
