use std::{
    collections::{BTreeMap, HashMap},
    env,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use http::{Method, Uri};
use redis::Client as RedisClient;
use sha2::{Digest, Sha256};
use url::form_urlencoded;

use crate::id_token::{IdTokenVerifier, RequestPrincipal};
use crate::service_signing::MultiIssuerJwksVerifier;
use crate::{AuthError, SignatureHeader, parse_signature_header};

pub const SIGNATURE_TRUSTED_KEYS_ENV: &str = "CRATESTACK_AUTH_SIGNATURE_TRUSTED_KEYS";
pub const SIGNATURE_TRUSTED_ISSUERS_ENV: &str = "CRATESTACK_AUTH_SIGNATURE_TRUSTED_ISSUERS";
pub const SIGNATURE_MAX_SKEW_SECONDS_ENV: &str = "CRATESTACK_AUTH_SIGNATURE_MAX_SKEW_SECONDS";
pub const SIGNATURE_REPLAY_WINDOW_SECONDS_ENV: &str =
    "CRATESTACK_AUTH_SIGNATURE_REPLAY_WINDOW_SECONDS";
pub const DEFAULT_SIGNATURE_MAX_SKEW_SECONDS: i64 = 300;
pub const DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS: i64 = 300;
const REDIS_NONCE_KEY_PREFIX: &str = "cratestack:signature-nonce";

pub struct SignRequestParams<'a> {
    pub signing_key: &'a SigningKey,
    pub method: &'a Method,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub body: &'a [u8],
    pub timestamp: &'a str,
    pub nonce: &'a str,
    pub key_id: &'a str,
}

#[async_trait]
pub trait NonceStore: Send + Sync {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError>;
}

/// Resolves a device's ed25519 verifying key by its key id.
///
/// Device-signed requests carry `keyId=<device-key-id>`, which is not in
/// any service JWKS or the static trusted-keys map, so the verifier falls
/// through to this resolver. The service that owns the device-key registry
/// (auth-service) plugs in a DB-backed implementation, giving device
/// requests true proof-of-possession: the transport signature is verified
/// against the stored public key.
#[async_trait]
pub trait DeviceKeyResolver: Send + Sync {
    /// Return the active device key's verifying key, or `None` when the kid
    /// is unknown or revoked. `Err` is reserved for backend failures (e.g.
    /// the store being unreachable) so the caller can tell "no such key"
    /// apart from "couldn't check".
    async fn lookup_device_verifying_key(
        &self,
        key_id: &str,
    ) -> Result<Option<VerifyingKey>, AuthError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedRequestPrincipal {
    pub key_id: String,
    pub timestamp: DateTime<Utc>,
    pub nonce: String,
    pub id_jwt: Option<String>,
    pub alg: Option<String>,
    pub content_sha256: String,
    /// True when the signing key was resolved ONLY via the cnf-bound id_jwt
    /// proof-of-possession fallback — i.e. this is an end-user device, not a
    /// statically-trusted service key or a JWKS/registry-resolved key. Internal
    /// service-to-service middleware (`require_signed_request`) rejects these so
    /// an enrolled user can't reach `/internal/*` endpoints meant for services.
    pub via_id_token_pop: bool,
}

#[derive(Clone)]
pub struct SignedRequestVerifier {
    trusted_keys: Arc<BTreeMap<String, VerifyingKey>>,
    /// Optional JWKS-backed key resolver. When the static
    /// `trusted_keys` map misses, the verifier falls through to
    /// JWKS lookup across every registered issuer. This is what
    /// lets a signing service rotate kids without coordinating an
    /// env-var redeploy of every receiver.
    jwks_resolver: Option<MultiIssuerJwksVerifier>,
    max_skew: Duration,
    replay_window: Duration,
    nonce_store: Arc<dyn NonceStore>,
    id_token_verifier: Option<Arc<IdTokenVerifier>>,
    /// Last-resort resolver for device keys, consulted when both the
    /// static map and the JWKS resolver miss. Enables device
    /// proof-of-possession (see [`DeviceKeyResolver`]).
    device_key_resolver: Option<Arc<dyn DeviceKeyResolver>>,
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

pub fn canonical_query(query: Option<&str>) -> String {
    let Some(query) = query else {
        return String::new();
    };

    let mut grouped = BTreeMap::<String, Vec<String>>::new();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        grouped
            .entry(key.into_owned())
            .or_default()
            .push(value.into_owned());
    }

    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, values) in grouped {
        if values.is_empty() {
            serializer.append_pair(&key, "");
            continue;
        }

        for value in values {
            serializer.append_pair(&key, &value);
        }
    }

    serializer.finish()
}

pub fn content_sha256_base64url(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn canonical_signature_base(
    method: &Method,
    path: &str,
    query: Option<&str>,
    content_sha256: &str,
    timestamp: &str,
    nonce: &str,
    key_id: &str,
) -> String {
    [
        method.as_str().to_ascii_uppercase(),
        path.to_string(),
        canonical_query(query),
        content_sha256.to_string(),
        timestamp.to_string(),
        nonce.to_string(),
        key_id.to_string(),
    ]
    .join("\n")
}

pub fn sign_request(params: SignRequestParams<'_>) -> String {
    let signature_base = canonical_signature_base(
        params.method,
        params.path,
        params.query,
        &content_sha256_base64url(params.body),
        params.timestamp,
        params.nonce,
        params.key_id,
    );
    URL_SAFE_NO_PAD.encode(
        params
            .signing_key
            .sign(signature_base.as_bytes())
            .to_bytes(),
    )
}

pub fn encode_verifying_key(verifying_key: &VerifyingKey) -> String {
    URL_SAFE_NO_PAD.encode(verifying_key.as_bytes())
}

pub fn decode_verifying_key(encoded: &str) -> Result<VerifyingKey, AuthError> {
    let bytes =
        decode_url_safe(encoded).map_err(|error| AuthError::InvalidPublicKey(error.to_string()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AuthError::InvalidPublicKey("expected 32-byte ed25519 key".to_string()))?;

    VerifyingKey::from_bytes(&bytes).map_err(|error| AuthError::InvalidPublicKey(error.to_string()))
}

#[derive(Default)]
struct InMemoryNonceStore {
    entries: Mutex<HashMap<String, DateTime<Utc>>>,
}

#[async_trait]
impl NonceStore for InMemoryNonceStore {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError> {
        let now = Utc::now();
        let expires_at = timestamp + replay_window;
        let storage_key = format!("{key_id}:{nonce}");
        let mut entries = self.entries.lock().map_err(|_| {
            AuthError::InvalidTrustedSigningKeys("nonce store poisoned".to_string())
        })?;

        entries.retain(|_, active_until| *active_until > now);
        if matches!(entries.get(&storage_key), Some(active_until) if *active_until > now) {
            return Err(AuthError::NonceReused);
        }

        entries.insert(storage_key, expires_at);
        Ok(())
    }
}

struct RedisNonceStore {
    client: RedisClient,
}

impl RedisNonceStore {
    fn new(redis_url: &str) -> Result<Self, AuthError> {
        let client = RedisClient::open(redis_url).map_err(|error| {
            AuthError::InvalidNonceStoreConfiguration(format!(
                "invalid redis url for nonce store: {error}"
            ))
        })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl NonceStore for RedisNonceStore {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AuthError::NonceStoreUnavailable(error.to_string()))?;
        let storage_key = nonce_storage_key(key_id, nonce);
        let ttl_seconds = replay_ttl_seconds(timestamp, replay_window);
        let set_result: Option<String> = redis::cmd("SET")
            .arg(&storage_key)
            .arg(timestamp.to_rfc3339())
            .arg("NX")
            .arg("EX")
            .arg(ttl_seconds)
            .query_async(&mut connection)
            .await
            .map_err(|error| AuthError::NonceStoreUnavailable(error.to_string()))?;

        if set_result.is_some() {
            Ok(())
        } else {
            Err(AuthError::NonceReused)
        }
    }
}

/// Returns an empty Vec when the env var is unset rather than
/// erroring. Used by `from_env` so a service that wires JWKS-only
/// doesn't have to also set a stub static-keys env var.
fn parse_trusted_keys_from_env_optional() -> Result<Vec<(String, VerifyingKey)>, AuthError> {
    match env::var(SIGNATURE_TRUSTED_KEYS_ENV) {
        Ok(raw) if !raw.trim().is_empty() => parse_trusted_keys(&raw),
        _ => Ok(Vec::new()),
    }
}

/// Parse `CRATESTACK_AUTH_SIGNATURE_TRUSTED_ISSUERS` as a JSON object mapping
/// issuer name → JWKS URL. The verifier walks every entry on cache
/// miss to find the kid carried in the signed-request header.
///
/// Example:
///
/// ```text
/// CRATESTACK_AUTH_SIGNATURE_TRUSTED_ISSUERS={
///   "vendor-service": "http://vendor-service:8082/jwks.json",
///   "order-service":  "http://order-service:8084/jwks.json"
/// }
/// ```
///
/// Empty / unset returns an empty map (callers decide whether
/// JWKS-only is acceptable for them).
fn parse_trusted_issuers_from_env_optional() -> Result<HashMap<String, String>, AuthError> {
    let raw = match env::var(SIGNATURE_TRUSTED_ISSUERS_ENV) {
        Ok(raw) if !raw.trim().is_empty() => raw,
        _ => return Ok(HashMap::new()),
    };
    let parsed: HashMap<String, String> = serde_json::from_str(&raw).map_err(|err| {
        AuthError::InvalidTrustedSigningKeys(format!(
            "{SIGNATURE_TRUSTED_ISSUERS_ENV} must be a JSON object: {err}",
        ))
    })?;
    Ok(parsed)
}

fn parse_trusted_keys(raw: &str) -> Result<Vec<(String, VerifyingKey)>, AuthError> {
    let mut trusted_keys = Vec::new();

    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (key_id, encoded_key) = entry.split_once(':').ok_or_else(|| {
            AuthError::InvalidTrustedSigningKeys(
                "expected CRATESTACK_AUTH_SIGNATURE_TRUSTED_KEYS entries in keyId:base64url format"
                    .to_string(),
            )
        })?;

        let verifying_key = decode_verifying_key(encoded_key)?;
        trusted_keys.push((key_id.to_string(), verifying_key));
    }

    if trusted_keys.is_empty() {
        return Err(AuthError::MissingTrustedSigningKeys);
    }

    Ok(trusted_keys)
}

fn parse_window_seconds(env_name: &str, default_value: i64) -> Result<i64, AuthError> {
    match env::var(env_name) {
        Ok(value) => {
            let parsed = value.parse::<i64>().map_err(|error| {
                AuthError::InvalidTrustedSigningKeys(format!(
                    "{env_name} must be an integer: {error}"
                ))
            })?;
            if parsed <= 0 {
                return Err(AuthError::InvalidTrustedSigningKeys(format!(
                    "{env_name} must be greater than zero"
                )));
            }
            Ok(parsed)
        }
        Err(_) => Ok(default_value),
    }
}

fn validate_timestamp(timestamp: DateTime<Utc>, max_skew: Duration) -> Result<(), AuthError> {
    let skew = Utc::now().signed_duration_since(timestamp).abs();
    if skew > max_skew {
        return Err(AuthError::SignatureTimestampOutOfWindow);
    }

    Ok(())
}

fn validate_signature_algorithm(alg: Option<&str>) -> Result<(), AuthError> {
    let Some(alg) = alg else {
        return Ok(());
    };

    if alg.eq_ignore_ascii_case("ed25519") || alg.eq_ignore_ascii_case("eddsa") {
        Ok(())
    } else {
        Err(AuthError::UnsupportedSignatureAlgorithm(alg.to_string()))
    }
}

fn validate_content_hash(
    header: &SignatureHeader,
    calculated_content_sha256: &str,
) -> Result<(), AuthError> {
    if let Some(supplied) = &header.content_sha256
        && supplied != calculated_content_sha256
    {
        return Err(AuthError::SignatureContentHashMismatch);
    }

    Ok(())
}

fn decode_signature(encoded: &str) -> Result<Signature, AuthError> {
    decode_signature_url_safe(encoded)
}

/// Decodes a base64url-encoded Ed25519 signature. Tolerates both padded and
/// unpadded forms. Public so other services (e.g. auth-service's device-
/// pairing flow) can verify ad-hoc signatures without re-implementing the
/// decode dance.
pub fn decode_signature_url_safe(encoded: &str) -> Result<Signature, AuthError> {
    let bytes = decode_url_safe(encoded)
        .map_err(|error| AuthError::InvalidSignatureEncoding(error.to_string()))?;
    Signature::from_slice(&bytes)
        .map_err(|error| AuthError::InvalidSignatureEncoding(error.to_string()))
}

/// Returns a Redis-backed [NonceStore] when [redis_url] is set, falling back
/// to an in-memory store. Useful for backend code that needs single-use
/// nonce protection outside the SignedRequestVerifier hot path (e.g. the
/// device-pairing envelope nonce).
pub fn nonce_store_from_redis_url(
    redis_url: Option<&str>,
) -> Result<Arc<dyn NonceStore>, AuthError> {
    match redis_url {
        Some(url) if !url.is_empty() => Ok(Arc::new(RedisNonceStore::new(url)?)),
        _ => Ok(Arc::new(InMemoryNonceStore::default())),
    }
}

fn decode_url_safe(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
}

fn replay_ttl_seconds(timestamp: DateTime<Utc>, replay_window: Duration) -> u64 {
    let ttl = (timestamp + replay_window - Utc::now())
        .num_seconds()
        .max(1);
    ttl as u64
}

fn nonce_storage_key(key_id: &str, nonce: &str) -> String {
    format!("{REDIS_NONCE_KEY_PREFIX}:{key_id}:{nonce}")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::SecondsFormat;

    use super::{
        DEFAULT_SIGNATURE_MAX_SKEW_SECONDS, NonceStore, SignRequestParams, SignedRequestVerifier,
        canonical_query, canonical_signature_base, content_sha256_base64url, encode_verifying_key,
        nonce_storage_key, replay_ttl_seconds, sign_request,
    };
    use crate::AuthError;
    use chrono::{Duration, Utc};
    use ed25519_dalek::SigningKey;
    use http::Method;

    #[test]
    fn canonicalizes_query_keys_lexicographically() {
        assert_eq!(canonical_query(Some("z=9&a=1&a=2&b=3")), "a=1&a=2&b=3&z=9");
    }

    #[test]
    fn content_hash_uses_base64url_sha256() {
        assert_eq!(
            content_sha256_base64url(b"hello"),
            "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ"
        );
    }

    #[test]
    fn canonical_signature_base_uses_newline_join() {
        assert_eq!(
            canonical_signature_base(
                &Method::POST,
                "/uploads/presign",
                Some("b=2&a=1"),
                "hash",
                "2026-04-24T12:00:00Z",
                "n_123",
                "vk_123"
            ),
            "POST\n/uploads/presign\na=1&b=2\nhash\n2026-04-24T12:00:00Z\nn_123\nvk_123"
        );
    }

    #[tokio::test]
    async fn verifies_signed_requests_and_rejects_reused_nonces() {
        let signing_key = example_signing_key();
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: &signing_key,
            method: &Method::POST,
            path: "/uploads/presign",
            query: None,
            body: br#"{"purpose":"vendorLogo"}"#,
            timestamp: &timestamp,
            nonce: "nonce-1",
            key_id: example_key_id().as_str(),
        });
        let header = format!(
            "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-1\", signature=\"{}\", alg=\"Ed25519\", content_sha256=\"{}\"",
            example_key_id(),
            timestamp,
            signature,
            content_sha256_base64url(br#"{"purpose":"vendorLogo"}"#),
        );

        let principal = verifier
            .verify(
                &Method::POST,
                &"/uploads/presign".parse().unwrap(),
                br#"{"purpose":"vendorLogo"}"#,
                &header,
            )
            .await
            .expect("signature should verify");
        assert_eq!(principal.key_id, example_key_id());
        // A statically-trusted key is NOT a PoP fallback caller.
        assert!(!principal.via_id_token_pop);

        let reused = verifier
            .verify(
                &Method::POST,
                &"/uploads/presign".parse().unwrap(),
                br#"{"purpose":"vendorLogo"}"#,
                &header,
            )
            .await;
        assert!(matches!(reused, Err(AuthError::NonceReused)));
    }

    #[tokio::test]
    async fn verifies_device_signed_request_via_cnf_bound_id_token() {
        // A device key present in NO trusted-key map, JWKS, or device resolver —
        // exactly the prod situation for a non-auth service.
        let device_key = SigningKey::from_bytes(&[13u8; 32]);
        let device_key_id = "vk_smoke_device";

        // Issuer key whose PUBLIC jwk is published at the test JWKS endpoint.
        let issuer_key = SigningKey::from_bytes(&[17u8; 32]);
        let issuer_kid = "issuer-test-v1";
        let issuer_url = "http://127.0.0.1:8081";

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let jwks_doc = crate::jwks(vec![crate::issuer_jwk(&issuer_key, issuer_kid)]);
        let router = axum::Router::new().route(
            "/jwks.json",
            axum::routing::get(move || {
                let jwks_doc = jwks_doc.clone();
                async move { axum::Json(jwks_doc) }
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        // Mint an id_jwt binding the device key (kid + jwk) in cnf.
        let claims = crate::default_id_token_claims(crate::IdTokenClaimsParams {
            issuer: issuer_url,
            client_id: "example-client",
            subject: "usr_smoke",
            bound_key_id: device_key_id,
            bound_key_jwk: Some(crate::verifying_key_jwk(
                &device_key.verifying_key(),
                device_key_id,
            )),
            profile_version: 1,
            enrollment_status: "enrolled",
            kyc_status: None,
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosures: Vec::new(),
        });
        let id_jwt = crate::issue_id_token(&issuer_key, issuer_kid, &claims).unwrap();

        let id_verifier = crate::IdTokenVerifier::new(
            issuer_url,
            &format!("http://{addr}/jwks.json"),
            Some(crate::DEFAULT_ID_TOKEN_AUDIENCE),
        )
        .unwrap();
        // No trusted keys, no device resolver — only the id-token verifier, like
        // a non-auth service built via `from_env().with_id_token_verifier(...)`.
        let verifier =
            SignedRequestVerifier::new(Vec::<(String, ed25519_dalek::VerifyingKey)>::new())
                .with_id_token_verifier(id_verifier);

        let body = br#"{"args":{}}"#;
        let path = "/rpc/procedure.myVendorContexts";
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: &device_key,
            method: &Method::POST,
            path,
            query: None,
            body,
            timestamp: &timestamp,
            nonce: "nonce-pop-1",
            key_id: device_key_id,
        });
        let header = format!(
            "Signature keyId=\"{device_key_id}\", timestamp=\"{timestamp}\", nonce=\"nonce-pop-1\", signature=\"{signature}\", alg=\"Ed25519\", id_jwt=\"{id_jwt}\""
        );

        let principal = verifier
            .verify(&Method::POST, &path.parse().unwrap(), body, &header)
            .await
            .expect("device-signed request should verify via cnf-bound id_jwt");
        assert_eq!(principal.key_id, device_key_id);
        // Tagged as PoP-resolved so internal middleware can reject end-user callers.
        assert!(principal.via_id_token_pop);

        // The same request WITHOUT the id_jwt is unresolvable (no PoP anchor).
        let header_no_jwt = format!(
            "Signature keyId=\"{device_key_id}\", timestamp=\"{timestamp}\", nonce=\"nonce-pop-2\", signature=\"{signature}\", alg=\"Ed25519\""
        );
        assert!(matches!(
            verifier
                .verify(&Method::POST, &path.parse().unwrap(), body, &header_no_jwt)
                .await,
            Err(AuthError::UnknownSigningKey(_))
        ));

        server.abort();
    }

    #[tokio::test]
    async fn device_resolver_none_is_authoritative_over_cnf_fallback() {
        // Regression: a wired DeviceKeyResolver returning None (how auth-service
        // reports a REVOKED/disabled device) must be final — the cnf-bound PoP
        // fallback must NOT run and resurrect the device via its stale id_jwt.
        let device_key = SigningKey::from_bytes(&[13u8; 32]);
        let device_key_id = "vk_revoked_device";
        let issuer_key = SigningKey::from_bytes(&[17u8; 32]);
        let issuer_kid = "issuer-test-v1";
        let issuer_url = "http://127.0.0.1:8081";

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let jwks_doc = crate::jwks(vec![crate::issuer_jwk(&issuer_key, issuer_kid)]);
        let router = axum::Router::new().route(
            "/jwks.json",
            axum::routing::get(move || {
                let jwks_doc = jwks_doc.clone();
                async move { axum::Json(jwks_doc) }
            }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let claims = crate::default_id_token_claims(crate::IdTokenClaimsParams {
            issuer: issuer_url,
            client_id: "example-client",
            subject: "usr_revoked",
            bound_key_id: device_key_id,
            bound_key_jwk: Some(crate::verifying_key_jwk(
                &device_key.verifying_key(),
                device_key_id,
            )),
            profile_version: 1,
            enrollment_status: "enrolled",
            kyc_status: None,
            main_email: None,
            main_phone: None,
            main_address: None,
            disclosures: Vec::new(),
        });
        let id_jwt = crate::issue_id_token(&issuer_key, issuer_kid, &claims).unwrap();
        let id_verifier = crate::IdTokenVerifier::new(
            issuer_url,
            &format!("http://{addr}/jwks.json"),
            Some(crate::DEFAULT_ID_TOKEN_AUDIENCE),
        )
        .unwrap();

        // Resolver that always reports "no such active key" — the revoked case.
        struct RevokedResolver;
        #[async_trait]
        impl super::DeviceKeyResolver for RevokedResolver {
            async fn lookup_device_verifying_key(
                &self,
                _key_id: &str,
            ) -> Result<Option<ed25519_dalek::VerifyingKey>, AuthError> {
                Ok(None)
            }
        }
        let verifier =
            SignedRequestVerifier::new(Vec::<(String, ed25519_dalek::VerifyingKey)>::new())
                .with_id_token_verifier(id_verifier)
                .with_device_key_resolver(Arc::new(RevokedResolver));

        let body = br#"{"args":{}}"#;
        let path = "/rpc/procedure.myDevices";
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: &device_key,
            method: &Method::POST,
            path,
            query: None,
            body,
            timestamp: &timestamp,
            nonce: "nonce-revoked",
            key_id: device_key_id,
        });
        let header = format!(
            "Signature keyId=\"{device_key_id}\", timestamp=\"{timestamp}\", nonce=\"nonce-revoked\", signature=\"{signature}\", alg=\"Ed25519\", id_jwt=\"{id_jwt}\""
        );

        // Even with a perfectly valid cnf-bound id_jwt, the resolver's None wins.
        assert!(matches!(
            verifier
                .verify(&Method::POST, &path.parse().unwrap(), body, &header)
                .await,
            Err(AuthError::UnknownSigningKey(_))
        ));

        server.abort();
    }

    #[tokio::test]
    async fn rejects_stale_timestamps() {
        let signing_key = example_signing_key();
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
        let timestamp = (Utc::now() - Duration::seconds(DEFAULT_SIGNATURE_MAX_SKEW_SECONDS + 30))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: &signing_key,
            method: &Method::GET,
            path: "/vendors",
            query: Some("limit=20"),
            body: b"",
            timestamp: &timestamp,
            nonce: "nonce-2",
            key_id: example_key_id().as_str(),
        });
        let header = format!(
            "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-2\", signature=\"{}\"",
            example_key_id(),
            timestamp,
            signature,
        );

        let result = verifier
            .verify(
                &Method::GET,
                &"/vendors?limit=20".parse().unwrap(),
                b"",
                &header,
            )
            .await;

        assert!(matches!(
            result,
            Err(AuthError::SignatureTimestampOutOfWindow)
        ));
    }

    #[tokio::test]
    async fn rejects_non_utc_timestamp_offsets() {
        let signing_key = example_signing_key();
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
        let timestamp = "2026-04-24T12:00:00+01:00";
        let signature = sign_request(SignRequestParams {
            signing_key: &signing_key,
            method: &Method::GET,
            path: "/vendors",
            query: None,
            body: b"",
            timestamp,
            nonce: "nonce-utc",
            key_id: example_key_id().as_str(),
        });
        let header = format!(
            "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-utc\", signature=\"{}\"",
            example_key_id(),
            timestamp,
            signature,
        );

        let result = verifier
            .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
            .await;
        assert!(matches!(
            result,
            Err(AuthError::InvalidSignatureTimestamp(_))
        ));
    }

    #[tokio::test]
    async fn accepts_explicit_utc_offset() {
        let signing_key = example_signing_key();
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
        let timestamp = Utc::now()
            .to_rfc3339_opts(SecondsFormat::Secs, false)
            .replace('Z', "+00:00");
        let signature = sign_request(SignRequestParams {
            signing_key: &signing_key,
            method: &Method::GET,
            path: "/vendors",
            query: None,
            body: b"",
            timestamp: &timestamp,
            nonce: "nonce-utc-zero",
            key_id: example_key_id().as_str(),
        });
        let header = format!(
            "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-utc-zero\", signature=\"{}\"",
            example_key_id(),
            timestamp,
            signature,
        );

        let principal = verifier
            .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
            .await
            .expect("+00:00 timestamps should verify");
        assert_eq!(principal.key_id, example_key_id());
    }

    #[tokio::test]
    async fn supports_custom_nonce_store() {
        let signing_key = example_signing_key();
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())])
                .with_nonce_store(Arc::new(RejectingNonceStore));
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: &signing_key,
            method: &Method::GET,
            path: "/vendors",
            query: None,
            body: b"",
            timestamp: &timestamp,
            nonce: "nonce-3",
            key_id: example_key_id().as_str(),
        });
        let header = format!(
            "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-3\", signature=\"{}\"",
            example_key_id(),
            timestamp,
            signature,
        );

        let result = verifier
            .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
            .await;
        assert!(matches!(result, Err(AuthError::NonceReused)));
    }

    #[test]
    fn builds_nonce_storage_keys() {
        assert_eq!(
            nonce_storage_key("vk_123", "n_456"),
            "cratestack:signature-nonce:vk_123:n_456"
        );
    }

    #[test]
    fn replay_ttl_uses_remaining_window() {
        let ttl = replay_ttl_seconds(Utc::now() - Duration::seconds(60), Duration::seconds(300));
        assert!((239..=240).contains(&ttl));
    }

    #[test]
    fn rejects_invalid_redis_nonce_store_configuration() {
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), example_signing_key().verifying_key())]);
        let error = verifier
            .with_redis_nonce_store("not-a-redis-url")
            .err()
            .expect("invalid redis urls should be rejected");

        assert!(matches!(
            error,
            AuthError::InvalidNonceStoreConfiguration(_)
        ));
    }

    #[tokio::test]
    async fn fails_closed_when_redis_nonce_store_is_unavailable() {
        let signing_key = example_signing_key();
        let verifier =
            SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())])
                .with_redis_nonce_store("redis://127.0.0.1:1/")
                .expect("redis url should parse");
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: &signing_key,
            method: &Method::GET,
            path: "/vendors",
            query: None,
            body: b"",
            timestamp: &timestamp,
            nonce: "nonce-redis-down",
            key_id: example_key_id().as_str(),
        });
        let header = format!(
            "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-redis-down\", signature=\"{}\"",
            example_key_id(),
            timestamp,
            signature,
        );

        let result = verifier
            .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
            .await;
        assert!(matches!(result, Err(AuthError::NonceStoreUnavailable(_))));
    }

    #[test]
    fn round_trips_verifying_keys() {
        let verifying_key = example_signing_key().verifying_key();
        let encoded = encode_verifying_key(&verifying_key);
        let decoded = super::decode_verifying_key(&encoded).expect("verifying key should decode");

        assert_eq!(decoded, verifying_key);
    }

    /// Rotation simulation: signer rolls from `kid_v1` to `kid_v2`
    /// while both keys live in JWKS. The verifier picks the right
    /// VerifyingKey by `kid` without any env-var update on the
    /// receiver side.
    #[tokio::test]
    async fn jwks_resolver_falls_through_for_unknown_static_kid() {
        use crate::service_signing::{MultiIssuerJwksVerifier, ServiceSigningKey};
        use crate::{JwksDocument, issuer_jwk};
        use axum::Router;
        use std::collections::HashMap;
        use std::sync::Arc;

        // Two signing keys live behind the issuer's JWKS at once —
        // simulating the `current` + `next` window during rotation.
        let key_v1 = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-v1");
        let key_v2 = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-v2");

        let combined_jwks = JwksDocument {
            keys: vec![
                issuer_jwk(key_v1.signing_key(), "vendor-service-v1"),
                issuer_jwk(key_v2.signing_key(), "vendor-service-v2"),
            ],
        };
        let jwks = Arc::new(combined_jwks);
        let app = Router::new().route(
            "/jwks.json",
            axum::routing::get({
                let jwks = jwks.clone();
                move || {
                    let jwks = jwks.clone();
                    async move { axum::Json::<JwksDocument>((*jwks).clone()) }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let resolver = MultiIssuerJwksVerifier::new(HashMap::from([(
            "vendor-service".to_owned(),
            format!("http://{addr}/jwks.json"),
        )]))
        .unwrap();

        // Static map is empty — every lookup must go through JWKS.
        let verifier = SignedRequestVerifier::new(std::iter::empty::<(String, _)>())
            .with_jwks_resolver(resolver);

        // Sign with v2 — the kid the verifier has never seen statically.
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: key_v2.signing_key(),
            method: &Method::POST,
            path: "/uploads/presign",
            query: None,
            body: br#"{"hi":1}"#,
            timestamp: &timestamp,
            nonce: "nonce-rot-v2",
            key_id: "vendor-service-v2",
        });
        let header = format!(
            "Signature keyId=\"vendor-service-v2\", timestamp=\"{timestamp}\", nonce=\"nonce-rot-v2\", signature=\"{signature}\""
        );

        let principal = verifier
            .verify(
                &Method::POST,
                &"/uploads/presign".parse().unwrap(),
                br#"{"hi":1}"#,
                &header,
            )
            .await
            .expect("v2 signature should resolve via JWKS");
        assert_eq!(principal.key_id, "vendor-service-v2");

        // v1 is also still accepted (the rotation window).
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: key_v1.signing_key(),
            method: &Method::POST,
            path: "/uploads/presign",
            query: None,
            body: br#"{"hi":2}"#,
            timestamp: &timestamp,
            nonce: "nonce-rot-v1",
            key_id: "vendor-service-v1",
        });
        let header = format!(
            "Signature keyId=\"vendor-service-v1\", timestamp=\"{timestamp}\", nonce=\"nonce-rot-v1\", signature=\"{signature}\""
        );
        let principal = verifier
            .verify(
                &Method::POST,
                &"/uploads/presign".parse().unwrap(),
                br#"{"hi":2}"#,
                &header,
            )
            .await
            .expect("v1 signature should also resolve");
        assert_eq!(principal.key_id, "vendor-service-v1");

        // Unknown kid still fails with UnknownSigningKey.
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: key_v2.signing_key(),
            method: &Method::POST,
            path: "/uploads/presign",
            query: None,
            body: br#"{"hi":3}"#,
            timestamp: &timestamp,
            nonce: "nonce-rot-bad",
            key_id: "vendor-service-v99",
        });
        let header = format!(
            "Signature keyId=\"vendor-service-v99\", timestamp=\"{timestamp}\", nonce=\"nonce-rot-bad\", signature=\"{signature}\""
        );
        let result = verifier
            .verify(
                &Method::POST,
                &"/uploads/presign".parse().unwrap(),
                br#"{"hi":3}"#,
                &header,
            )
            .await;
        assert!(matches!(result, Err(AuthError::UnknownSigningKey(_))));

        // Static map is preferred over JWKS when both have the kid —
        // rebuild a verifier with v1 in the static map; v1 lookup
        // should not touch the JWKS server.
        let resolver_unreachable = MultiIssuerJwksVerifier::new(HashMap::from([(
            "vendor-service".to_owned(),
            "http://127.0.0.1:1/jwks.json".to_owned(),
        )]))
        .unwrap();
        let verifier = SignedRequestVerifier::new([(
            "vendor-service-v1".to_owned(),
            key_v1.signing_key().verifying_key(),
        )])
        .with_jwks_resolver(resolver_unreachable);
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let signature = sign_request(SignRequestParams {
            signing_key: key_v1.signing_key(),
            method: &Method::POST,
            path: "/uploads/presign",
            query: None,
            body: br#"{"hi":4}"#,
            timestamp: &timestamp,
            nonce: "nonce-static-pref",
            key_id: "vendor-service-v1",
        });
        let header = format!(
            "Signature keyId=\"vendor-service-v1\", timestamp=\"{timestamp}\", nonce=\"nonce-static-pref\", signature=\"{signature}\""
        );
        verifier
            .verify(
                &Method::POST,
                &"/uploads/presign".parse().unwrap(),
                br#"{"hi":4}"#,
                &header,
            )
            .await
            .expect("static map should short-circuit JWKS");

        server.abort();
    }

    fn example_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[
            0x52, 0x21, 0x09, 0x7a, 0x8c, 0x1b, 0x2d, 0x48, 0x93, 0x4f, 0x61, 0xf0, 0xa5, 0x33,
            0x1e, 0x9c, 0x74, 0x08, 0xa1, 0x64, 0x5b, 0x91, 0x2f, 0x3c, 0xb8, 0x27, 0xa0, 0xd9,
            0x1f, 0x45, 0x6c, 0x22,
        ])
    }

    fn example_key_id() -> String {
        "vk_example".to_string()
    }

    struct RejectingNonceStore;

    #[async_trait]
    impl NonceStore for RejectingNonceStore {
        async fn claim(
            &self,
            _key_id: &str,
            _nonce: &str,
            _timestamp: chrono::DateTime<Utc>,
            _replay_window: chrono::Duration,
        ) -> Result<(), AuthError> {
            Err(AuthError::NonceReused)
        }
    }
}
