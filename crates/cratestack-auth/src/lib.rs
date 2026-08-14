//! CrateStack signed-request + identity-token auth.
//!
//! - SigV4-style canonical request construction/signing/verification over
//!   Ed25519 ([`sign_request`], [`SignedRequestVerifier`]).
//! - SD-JWT id-token issuance and verification ([`issue_sd_id_token`],
//!   [`IdTokenVerifier`]).
//! - Multi-issuer JWKS resolution ([`MultiIssuerJwksVerifier`]) and
//!   per-service signing identities ([`ServiceSigningKey`]).
//! - COSE-signed enrolment challenges ([`build_cose_enroll_response`] /
//!   [`parse_cose_enroll_response`]).
//! - [`SignedRequestAuthProvider`], a [`cratestack_core::AuthProvider`]
//!   implementation wiring [`SignedRequestVerifier`] into a cratestack
//!   server.
//!
//! The `axum` Cargo feature (default-on) gates the items that touch the
//! `axum` crate itself: [`require_signed_request`], `jwks_router`, and the
//! `FromRequestParts` extractor impls on [`CurrentPrincipal`]/
//! [`AuthenticatedPrincipal`]. Everything else depends only on the plain
//! `http` crate for header/method/URI types, so a signing-only consumer
//! (e.g. a `cratestack-client`) can `default-features = false` to keep axum
//! (and its own tower/hyper/matchit dependency tree) out of its build.

mod id_token;
#[cfg(feature = "axum")]
mod middleware;
#[cfg(feature = "axum")]
mod response;
mod service_signing;
mod signed_request;

use chrono::{DateTime, Duration, Utc};
use coset::{CoseSign1, CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SIGNATURE_SCHEME: &str = "Signature";
pub const ID_TOKEN_GRANT: &str = "urn:cratestack:params:oauth:grant-type:id-sd-jwt";
pub const REFRESH_TOKEN_GRANT: &str = "refresh_token";
pub const ENROLL_CHALLENGE_COSE_KID: &str = "cratestack-auth-enroll-challenge-v1";
/// Env var carrying the URL-safe-base64-no-pad-encoded 32-byte Ed25519
/// seed used to sign/verify COSE enrolment challenge responses. See
/// [`challenge_signing_key`].
pub const CHALLENGE_SIGNING_KEY_ENV: &str = "CRATESTACK_AUTH_CHALLENGE_SIGNING_KEY";

pub use id_token::{
    AuthenticatedPrincipal, CurrentPrincipal, DEFAULT_ID_TOKEN_AUDIENCE, DisclosureClaim,
    ID_TOKEN_AUDIENCE_ENV, IdTokenClaims, IdTokenClaimsParams, IdTokenVerifier, IssuedSdIdToken,
    RequestPrincipal, UserPrincipal, decode_disclosures_unverified,
    decode_id_token_claims_unverified, decode_signing_key, default_id_token_claims,
    encode_signing_key, issue_id_token, issue_sd_id_token, issuer_jwk, take_disclosures,
    verifying_key_from_jwk, verifying_key_jwk,
};
#[cfg(feature = "axum")]
pub use middleware::require_signed_request;
#[cfg(feature = "axum")]
pub use service_signing::jwks_router;
pub use service_signing::{
    MultiIssuerJwksVerifier, ServiceSigningKey, VerifiedToken, mint_signed_token,
};
pub use signed_request::{
    DEFAULT_SIGNATURE_MAX_SKEW_SECONDS, DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS, DeviceKeyResolver,
    NonceStore, SIGNATURE_MAX_SKEW_SECONDS_ENV, SIGNATURE_REPLAY_WINDOW_SECONDS_ENV,
    SIGNATURE_TRUSTED_ISSUERS_ENV, SIGNATURE_TRUSTED_KEYS_ENV, SignRequestParams,
    SignedRequestPrincipal, SignedRequestVerifier, canonical_query, canonical_signature_base,
    content_sha256_base64url, decode_signature_url_safe, decode_verifying_key,
    encode_verifying_key, nonce_store_from_redis_url, sign_request,
};

#[derive(Clone)]
pub struct SignedRequestAuthProvider {
    verifier: SignedRequestVerifier,
    transport_caller_mode: TransportCallerMode,
}

impl SignedRequestAuthProvider {
    pub fn new(verifier: SignedRequestVerifier) -> Self {
        Self {
            verifier,
            transport_caller_mode: TransportCallerMode::Never,
        }
    }

    pub fn allow_transport_callers(mut self, mode: TransportCallerMode) -> Self {
        self.transport_caller_mode = mode;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportCallerMode {
    Never,
    SafeReadOnly,
    AllMethods,
}

impl TransportCallerMode {
    fn allows(self, method: &str) -> bool {
        match self {
            Self::Never => false,
            Self::SafeReadOnly => matches!(method, "GET" | "HEAD" | "OPTIONS"),
            Self::AllMethods => true,
        }
    }
}

impl cratestack_core::AuthProvider for SignedRequestAuthProvider {
    type Error = cratestack_core::CratestackError;

    fn authenticate(
        &self,
        request: &cratestack_core::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<cratestack_core::CratestackContext, Self::Error>> + Send
    {
        let allow_transport_caller = self.transport_caller_mode.allows(request.method);
        authenticate_cool_request(self.verifier.clone(), request, move |principal| {
            principal_to_cool_context(principal, Some("caller"), allow_transport_caller)
        })
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("missing authorization signature scheme")]
    MissingScheme,
    #[error("malformed authorization signature header")]
    MalformedSignatureHeader,
    #[error("unknown authorization signature parameter: {0}")]
    UnknownSignatureParameter(String),
    #[error("duplicate authorization signature parameter: {0}")]
    DuplicateSignatureParameter(String),
    #[error("missing authorization signature parameter: {0}")]
    MissingSignatureParameter(&'static str),
    #[error("missing authorization header")]
    MissingAuthorizationHeader,
    #[error("invalid authorization header encoding")]
    InvalidAuthorizationHeaderEncoding,
    #[error("missing trusted signing keys configuration")]
    MissingTrustedSigningKeys,
    #[error("invalid trusted signing key configuration: {0}")]
    InvalidTrustedSigningKeys(String),
    #[error("invalid nonce store configuration: {0}")]
    InvalidNonceStoreConfiguration(String),
    #[error("invalid signature timestamp: {0}")]
    InvalidSignatureTimestamp(String),
    #[error("signature timestamp is outside the accepted skew window")]
    SignatureTimestampOutOfWindow,
    #[error("unsupported signature algorithm: {0}")]
    UnsupportedSignatureAlgorithm(String),
    #[error("unknown signing key: {0}")]
    UnknownSigningKey(String),
    #[error("device key lookup failed: {0}")]
    DeviceKeyLookup(String),
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("invalid signature encoding: {0}")]
    InvalidSignatureEncoding(String),
    #[error("signature verification failed")]
    SignatureVerificationFailed,
    #[error("signature content hash mismatch")]
    SignatureContentHashMismatch,
    #[error("nonce has already been used within the replay window")]
    NonceReused,
    #[error("nonce store unavailable: {0}")]
    NonceStoreUnavailable(String),
    #[error("failed to read request body: {0}")]
    RequestBodyRead(String),
    #[error("failed to encode id token: {0}")]
    IdTokenEncoding(String),
    #[error("failed to decode id token: {0}")]
    IdTokenDecoding(String),
    #[error("failed to fetch issuer jwks: {0}")]
    IdTokenJwksUnavailable(String),
    #[error("issuer jwks did not contain signing key: {0}")]
    UnknownIdTokenSigningKey(String),
    #[error("id token uses unsupported signing algorithm: {0}")]
    UnsupportedIdTokenAlgorithm(String),
    #[error("id token signature verification failed")]
    IdTokenVerificationFailed,
    #[error("id token issuer did not match configured issuer")]
    IdTokenIssuerMismatch,
    #[error("id token audience did not match configured audience")]
    IdTokenAudienceMismatch,
    #[error("id token is expired")]
    IdTokenExpired,
    #[error("id token binding key did not match request signature key")]
    IdTokenBindingMismatch,
    #[error("missing bearer token")]
    MissingBearerToken,
    #[error("unsupported grant type")]
    UnsupportedGrantType,
    #[error("failed to encode challenge artifact: {0}")]
    ChallengeEncoding(String),
    #[error("failed to decode challenge artifact: {0}")]
    ChallengeDecoding(String),
    #[error("challenge artifact payload is missing")]
    MissingChallengePayload,
    #[error("missing service signing key environment variable: {0}")]
    MissingSigningKeyEnv(String),
    #[error("token issuer is not in the configured trust list: {0}")]
    UntrustedIssuer(String),
    #[error("internal endpoint requires a trusted service caller, not an end-user device")]
    InternalEndpointForbidden,
}

pub fn auth_error_to_cool_error(error: AuthError) -> cratestack_core::CratestackError {
    match error {
        AuthError::MissingAuthorizationHeader
        | AuthError::MissingScheme
        | AuthError::MalformedSignatureHeader
        | AuthError::UnknownSignatureParameter(_)
        | AuthError::DuplicateSignatureParameter(_)
        | AuthError::MissingSignatureParameter(_)
        | AuthError::InvalidAuthorizationHeaderEncoding
        | AuthError::MissingTrustedSigningKeys
        | AuthError::InvalidTrustedSigningKeys(_)
        | AuthError::InvalidSignatureTimestamp(_)
        | AuthError::SignatureTimestampOutOfWindow
        | AuthError::UnsupportedSignatureAlgorithm(_)
        | AuthError::UnknownSigningKey(_)
        | AuthError::InvalidPublicKey(_)
        | AuthError::InvalidSignatureEncoding(_)
        | AuthError::SignatureVerificationFailed
        | AuthError::SignatureContentHashMismatch
        | AuthError::NonceReused
        | AuthError::MissingBearerToken
        | AuthError::IdTokenDecoding(_)
        | AuthError::UnknownIdTokenSigningKey(_)
        | AuthError::UnsupportedIdTokenAlgorithm(_)
        | AuthError::IdTokenVerificationFailed
        | AuthError::IdTokenIssuerMismatch
        | AuthError::IdTokenAudienceMismatch
        | AuthError::IdTokenExpired
        | AuthError::IdTokenBindingMismatch
        | AuthError::UntrustedIssuer(_) => {
            cratestack_core::CratestackError::Unauthorized(error.to_string())
        }
        AuthError::InternalEndpointForbidden => {
            cratestack_core::CratestackError::Forbidden(error.to_string())
        }
        AuthError::NonceStoreUnavailable(_)
        | AuthError::IdTokenJwksUnavailable(_)
        | AuthError::DeviceKeyLookup(_) => {
            cratestack_core::CratestackError::Internal(error.to_string())
        }
        AuthError::RequestBodyRead(_)
        | AuthError::IdTokenEncoding(_)
        | AuthError::UnsupportedGrantType
        | AuthError::ChallengeEncoding(_)
        | AuthError::ChallengeDecoding(_)
        | AuthError::MissingChallengePayload
        | AuthError::InvalidNonceStoreConfiguration(_)
        | AuthError::MissingSigningKeyEnv(_) => {
            cratestack_core::CratestackError::BadRequest(error.to_string())
        }
    }
}

pub fn authorization_header(headers: &http::HeaderMap) -> Option<String> {
    headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

pub fn request_uri(path: &str, query: Option<&str>) -> Result<http::Uri, http::uri::InvalidUri> {
    let value = match query {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    };
    value.parse()
}

pub fn principal_to_cool_context(
    principal: &RequestPrincipal,
    role: Option<&str>,
    allow_transport_caller: bool,
) -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError> {
    let Some(user) = principal.user.as_ref() else {
        if allow_transport_caller {
            return Ok(service_principal_to_cool_context(principal, role));
        }
        return Ok(cratestack_core::CratestackContext::anonymous());
    };

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ActorPrincipal {
        id: String,
        enrollment_status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        kyc_status: Option<String>,
        profile_version: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        main_email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        main_phone: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        main_address: Option<serde_json::Value>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SessionPrincipal {
        client_id: String,
        audience: String,
        bound_key_id: String,
        request_key_id: String,
        request_nonce: String,
        request_timestamp: String,
        issued_at: i64,
        expires_at: i64,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CratestackPrincipal {
        actor: ActorPrincipal,
        session: SessionPrincipal,
        id: String,
        client_id: String,
        enrollment_status: String,
        bound_key_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kyc_status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kyc_dossier_id: Option<String>,
    }

    let kyc_dossier_id = user
        .disclosed_claims
        .get("kycDossierId")
        .and_then(|value| value.as_str().map(str::to_owned));

    cratestack_core::CratestackContext::from_principal(Some(CratestackPrincipal {
        actor: ActorPrincipal {
            id: user.user_id.clone(),
            enrollment_status: user.enrollment_status.clone(),
            kyc_status: user.kyc_status.clone(),
            profile_version: user.profile_version,
            main_email: user.main_email.clone(),
            main_phone: user.main_phone.clone(),
            main_address: user.main_address.clone(),
        },
        session: SessionPrincipal {
            client_id: user.client_id.clone(),
            audience: user.audience.clone(),
            bound_key_id: user.bound_key_id.clone(),
            request_key_id: principal.transport.key_id.clone(),
            request_nonce: principal.transport.nonce.clone(),
            request_timestamp: principal.transport.timestamp.to_rfc3339(),
            issued_at: user.issued_at,
            expires_at: user.expires_at,
        },
        id: user.user_id.clone(),
        client_id: user.client_id.clone(),
        enrollment_status: user.enrollment_status.clone(),
        bound_key_id: user.bound_key_id.clone(),
        // A user principal's role comes from the *verified* id_jwt `role`
        // claim, NOT the caller-supplied `role` argument. The argument only
        // names the default role for the service-caller path below. This is
        // what makes admin server-backed: the issuer stamps `role` from
        // `User.isAdmin`, so a caller can't self-grant via `client_id`/`azp`.
        role: Some(user.role.clone()),
        kyc_status: user.kyc_status.clone(),
        kyc_dossier_id,
    }))
}

fn service_principal_to_cool_context(
    principal: &RequestPrincipal,
    role: Option<&str>,
) -> cratestack_core::CratestackContext {
    let caller_id = format!("svc:{}", principal.transport.key_id);
    cratestack_core::CratestackContext::authenticated([
        (
            "id".to_owned(),
            cratestack_core::Value::String(caller_id.clone()),
        ),
        (
            "clientId".to_owned(),
            cratestack_core::Value::String(caller_id.clone()),
        ),
        (
            "enrollmentStatus".to_owned(),
            cratestack_core::Value::String("trusted_signature".to_owned()),
        ),
        (
            "boundKeyId".to_owned(),
            cratestack_core::Value::String(principal.transport.key_id.clone()),
        ),
        (
            "role".to_owned(),
            cratestack_core::Value::String(role.unwrap_or("caller").to_owned()),
        ),
        (
            "callerService".to_owned(),
            cratestack_core::Value::String(principal.transport.key_id.clone()),
        ),
        (
            "serviceName".to_owned(),
            cratestack_core::Value::String(principal.transport.key_id.clone()),
        ),
        (
            "actorType".to_owned(),
            cratestack_core::Value::String("service".to_owned()),
        ),
    ])
}

pub async fn authenticate_cool_request<F>(
    verifier: SignedRequestVerifier,
    request: &cratestack_core::RequestContext<'_>,
    map_context: F,
) -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>
where
    F: FnOnce(
            &RequestPrincipal,
        )
            -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>
        + Send,
{
    authenticate_cool_request_with(verifier, request, |principal| {
        core::future::ready(map_context(&principal))
    })
    .await
}

/// Like [`authenticate_cool_request`] but the context mapper is **async** and
/// receives the principal by value. This lets the caller consult live state
/// (e.g. reload a user's role from the database) and adjust the verified
/// principal before building the `CratestackContext` — e.g. re-deriving an admin
/// role on every request so revoking it takes effect immediately instead of
/// waiting for the frozen `role` claim to expire.
pub async fn authenticate_cool_request_with<F, Fut>(
    verifier: SignedRequestVerifier,
    request: &cratestack_core::RequestContext<'_>,
    map_context: F,
) -> Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>
where
    F: FnOnce(RequestPrincipal) -> Fut + Send,
    Fut: core::future::Future<
            Output = Result<cratestack_core::CratestackContext, cratestack_core::CratestackError>,
        > + Send,
{
    let authorization = authorization_header(request.headers);
    let method = request.method.to_owned();
    let path = request.path.to_owned();
    let query = request.query.map(str::to_owned);
    let body = request.body.to_vec();

    let Some(authorization) = authorization else {
        return Ok(cratestack_core::CratestackContext::anonymous());
    };

    let uri = request_uri(&path, query.as_deref())
        .map_err(|error| cratestack_core::CratestackError::BadRequest(error.to_string()))?;
    let method = http::Method::from_bytes(method.as_bytes())
        .map_err(|error| cratestack_core::CratestackError::BadRequest(error.to_string()))?;
    let principal = verifier
        .authenticate(&method, &uri, &body, &authorization)
        .await
        .map_err(auth_error_to_cool_error)?;

    map_context(principal).await
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub userinfo_endpoint: String,
    pub introspection_endpoint: String,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JwksDocument {
    pub keys: Vec<Jwk>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Jwk {
    pub kty: String,
    pub kid: String,
    pub alg: String,
    #[serde(rename = "use")]
    pub key_use: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnrollRequest {
    #[serde(rename = "deviceName")]
    pub device_name: String,
    pub platform: String,
    #[serde(rename = "publicKey")]
    pub public_key: Option<String>,
    #[serde(rename = "publicJwk")]
    pub public_jwk: Option<Value>,
    #[serde(rename = "proposedKeyId")]
    pub proposed_key_id: Option<String>,
    #[serde(rename = "appVersion")]
    pub app_version: Option<String>,
    #[serde(rename = "clientInfo")]
    pub client_info: Option<Value>,
    #[serde(rename = "antiAbuse")]
    pub anti_abuse: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnrollResponse {
    #[serde(rename = "enrollmentId")]
    pub enrollment_id: String,
    #[serde(rename = "keyId")]
    pub key_id: String,
    pub challenge: String,
    #[serde(rename = "challengeFormat")]
    pub challenge_format: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerifyRequest {
    #[serde(rename = "enrollmentId")]
    pub enrollment_id: String,
    #[serde(rename = "challengeResponse")]
    pub challenge_response: String,
    #[serde(rename = "deviceProof")]
    pub device_proof: Option<String>,
    #[serde(rename = "clientInfo")]
    pub client_info: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerifyResponse {
    pub user: UserSummary,
    pub device: DeviceSummary,
    pub key: KeySummary,
    #[serde(rename = "nextStep")]
    pub next_step: NextStep,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserSummary {
    pub id: String,
    #[serde(rename = "enrollmentStatus")]
    pub enrollment_status: String,
    #[serde(rename = "kycStatus")]
    pub kyc_status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceSummary {
    #[serde(rename = "trustStatus")]
    pub trust_status: String,
    #[serde(rename = "deviceName")]
    pub device_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeySummary {
    #[serde(rename = "keyId")]
    pub key_id: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NextStep {
    #[serde(rename = "canRequestIdJwt")]
    pub can_request_id_jwt: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenRequest {
    #[serde(rename = "grant_type")]
    pub grant_type: String,
    #[serde(rename = "client_id")]
    pub client_id: String,
    #[serde(rename = "device_key_id")]
    pub device_key_id: Option<String>,
    #[serde(rename = "subject_token")]
    pub subject_token: Option<String>,
    #[serde(rename = "refresh_token")]
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    #[serde(rename = "profile_version_hint")]
    pub profile_version_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    #[serde(rename = "token_type")]
    pub token_type: String,
    #[serde(rename = "issued_token_type")]
    pub issued_token_type: String,
    #[serde(rename = "id_jwt")]
    pub id_jwt: String,
    #[serde(rename = "expires_in")]
    pub expires_in: i64,
    #[serde(rename = "refresh_token")]
    pub refresh_token: String,
    pub cnf: Confirmation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Confirmation {
    pub kid: String,
    /// The holder's bound public key, as an OKP/Ed25519 JWK. When present, a
    /// JWKS-verified id_jwt lets any service verify a request signed by this
    /// device key WITHOUT its own device-key registry — the issuer vouches for
    /// the key (proof-of-possession). Absent on service-to-service tokens that
    /// bind a `kid` resolvable via static trust / JWKS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<Jwk>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserinfoResponse {
    pub sub: String,
    #[serde(rename = "mainEmail")]
    pub main_email: Option<String>,
    #[serde(rename = "mainPhone")]
    pub main_phone: Option<String>,
    #[serde(rename = "mainAddress")]
    pub main_address: Option<Value>,
    #[serde(rename = "profileVersion")]
    pub profile_version: i32,
    #[serde(rename = "enrollmentStatus")]
    pub enrollment_status: String,
    #[serde(rename = "kycStatus")]
    pub kyc_status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IntrospectRequest {
    pub token: String,
    #[serde(rename = "token_type_hint")]
    pub token_type_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IntrospectResponse {
    pub active: bool,
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub exp: Option<i64>,
    pub kid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHeader {
    pub key_id: String,
    pub timestamp: String,
    pub nonce: String,
    pub signature: String,
    pub id_jwt: Option<String>,
    pub alg: Option<String>,
    pub content_sha256: Option<String>,
}

pub fn authorization_server_metadata(issuer: &str) -> AuthorizationServerMetadata {
    AuthorizationServerMetadata {
        issuer: issuer.to_string(),
        token_endpoint: format!("{issuer}/token"),
        jwks_uri: format!("{issuer}/jwks.json"),
        userinfo_endpoint: format!("{issuer}/userinfo"),
        introspection_endpoint: format!("{issuer}/introspect"),
        grant_types_supported: vec![ID_TOKEN_GRANT.to_string(), REFRESH_TOKEN_GRANT.to_string()],
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
        response_types_supported: Vec::new(),
    }
}

pub fn jwks(keys: Vec<Jwk>) -> JwksDocument {
    JwksDocument { keys }
}

pub fn uses_signature_scheme(header: &str) -> bool {
    header.starts_with(SIGNATURE_SCHEME)
}

pub fn parse_signature_header(header: &str) -> Result<SignatureHeader, AuthError> {
    if !uses_signature_scheme(header) {
        return Err(AuthError::MissingScheme);
    }

    let payload = header
        .strip_prefix(SIGNATURE_SCHEME)
        .ok_or(AuthError::MissingScheme)?
        .trim();

    let mut key_id = None;
    let mut timestamp = None;
    let mut nonce = None;
    let mut signature = None;
    let mut id_jwt = None;
    let mut alg = None;
    let mut content_sha256 = None;

    for pair in payload.split(',') {
        let (name, raw_value) = pair
            .trim()
            .split_once('=')
            .ok_or(AuthError::MalformedSignatureHeader)?;
        let raw_value = raw_value.trim();
        let value = raw_value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(AuthError::MalformedSignatureHeader)?
            .to_string();

        match name {
            "keyId" => assign_once(&mut key_id, value, name)?,
            "timestamp" => assign_once(&mut timestamp, value, name)?,
            "nonce" => assign_once(&mut nonce, value, name)?,
            "signature" => assign_once(&mut signature, value, name)?,
            "id_jwt" => assign_once(&mut id_jwt, value, name)?,
            "alg" => assign_once(&mut alg, value, name)?,
            "content_sha256" => assign_once(&mut content_sha256, value, name)?,
            unknown => return Err(AuthError::UnknownSignatureParameter(unknown.to_string())),
        }
    }

    Ok(SignatureHeader {
        key_id: key_id.ok_or(AuthError::MissingSignatureParameter("keyId"))?,
        timestamp: timestamp.ok_or(AuthError::MissingSignatureParameter("timestamp"))?,
        nonce: nonce.ok_or(AuthError::MissingSignatureParameter("nonce"))?,
        signature: signature.ok_or(AuthError::MissingSignatureParameter("signature"))?,
        id_jwt,
        alg,
        content_sha256,
    })
}

pub fn parse_bearer_token(header: &str) -> Result<&str, AuthError> {
    header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or(AuthError::MissingBearerToken)
}

pub fn build_cose_enroll_response(response: &EnrollResponse) -> Result<Vec<u8>, AuthError> {
    let signing_key = challenge_signing_key()?;
    build_cose_enroll_response_with_key(response, &signing_key)
}

fn build_cose_enroll_response_with_key(
    response: &EnrollResponse,
    signing_key: &SigningKey,
) -> Result<Vec<u8>, AuthError> {
    let payload = minicbor_serde::to_vec(response)
        .map_err(|error| AuthError::ChallengeEncoding(error.to_string()))?;
    let protected = HeaderBuilder::new()
        .algorithm(iana::Algorithm::EdDSA)
        .key_id(ENROLL_CHALLENGE_COSE_KID.as_bytes().to_vec())
        .build();
    let sign1 = CoseSign1Builder::new()
        .protected(protected)
        .payload(payload)
        .create_signature(b"", |to_sign| signing_key.sign(to_sign).to_bytes().to_vec())
        .build();

    sign1
        .to_tagged_vec()
        .map_err(|error| AuthError::ChallengeEncoding(error.to_string()))
}

pub fn parse_cose_enroll_response(bytes: &[u8]) -> Result<EnrollResponse, AuthError> {
    let signing_key = challenge_signing_key()?;
    parse_cose_enroll_response_with_key(bytes, &signing_key)
}

fn parse_cose_enroll_response_with_key(
    bytes: &[u8],
    signing_key: &SigningKey,
) -> Result<EnrollResponse, AuthError> {
    let sign1 = CoseSign1::from_tagged_slice(bytes)
        .map_err(|error| AuthError::ChallengeDecoding(error.to_string()))?;
    let verifying_key = signing_key.verifying_key();

    sign1
        .verify_signature(b"", |signature, data| {
            verify_signature(&verifying_key, signature, data)
        })
        .map_err(|error| AuthError::ChallengeDecoding(error.to_string()))?;

    let payload = sign1.payload.ok_or(AuthError::MissingChallengePayload)?;
    minicbor_serde::from_slice(&payload)
        .map_err(|error| AuthError::ChallengeDecoding(error.to_string()))
}

pub fn enrollment_id() -> String {
    new_cuid()
}

pub fn key_id() -> String {
    new_cuid()
}

pub fn user_id() -> String {
    new_cuid()
}

pub fn challenge() -> String {
    new_cuid()
}

pub fn challenge_expiry() -> DateTime<Utc> {
    Utc::now() + Duration::minutes(15)
}

fn new_cuid() -> String {
    format!("c{}", cuid2::create_id())
}

/// Loads the Ed25519 signing key used to sign and verify COSE enrolment
/// challenge responses (`build_cose_enroll_response` /
/// `parse_cose_enroll_response`) from [`CHALLENGE_SIGNING_KEY_ENV`]
/// (URL-safe-base64-no-pad-encoded 32-byte seed — same encoding as
/// [`ServiceSigningKey::from_env`][crate::ServiceSigningKey::from_env] and
/// [`encode_signing_key`]/[`decode_signing_key`]).
///
/// **Security history:** the downstream crate this was absorbed from used
/// to return a hardcoded Ed25519 seed literal here. That seed was committed
/// to that repository's git history and MUST be treated as permanently
/// compromised — anyone with read access to the repo (or its history,
/// forks, or CI logs) can reconstruct it and forge COSE enrolment challenge
/// responses. It must never be reused as a default, fallback, or "example"
/// value anywhere, including in tests — this crate's own tests generate
/// their own key material (see `tests::test_challenge_signing_key_b64`).
///
/// Fails closed: an absent or whitespace-only env var is a hard error, not
/// a silently-generated ephemeral key or a fallback to any default — a
/// service that can't load its challenge signing key must not be able to
/// sign or verify enrolment challenges at all.
fn challenge_signing_key() -> Result<SigningKey, AuthError> {
    challenge_signing_key_from(|name| std::env::var(name))
}

fn challenge_signing_key_from(
    lookup_env: impl Fn(&str) -> Result<String, std::env::VarError>,
) -> Result<SigningKey, AuthError> {
    let raw = lookup_env(CHALLENGE_SIGNING_KEY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AuthError::MissingSigningKeyEnv(CHALLENGE_SIGNING_KEY_ENV.to_string()))?;
    decode_signing_key(&raw)
}

fn verify_signature(
    verifying_key: &ed25519_dalek::VerifyingKey,
    signature: &[u8],
    data: &[u8],
) -> Result<(), String> {
    let signature = Signature::from_slice(signature).map_err(|error| error.to_string())?;
    verifying_key
        .verify(data, &signature)
        .map_err(|error| error.to_string())
}

/// Installs a `ring`-backed `rustls::crypto::CryptoProvider` if the process
/// doesn't already have one. `reqwest`'s `rustls-no-provider` feature (see
/// the workspace `Cargo.toml`'s `reqwest` entry) ships no crypto provider at
/// all: `reqwest::Client::build()` PANICS at construction time if
/// `rustls::crypto::CryptoProvider::get_default()` finds nothing installed.
/// [`id_token::IdTokenVerifier::new`] and
/// [`service_signing::MultiIssuerJwksVerifier::new`] both build a
/// `reqwest::Client` internally, so this crate needs the same fallback
/// `cratestack-client-rust::client::core::ensure_crypto_provider` installs,
/// for the identical reason.
///
/// `install_default()` only ever takes effect the FIRST time it succeeds
/// process-wide — it's a courtesy fallback, not an override. A consumer that
/// installs its own provider (any backend, including `aws-lc-rs`) before
/// constructing its first verifier keeps that choice; this only fires when
/// nobody has chosen anything yet. The `Err` it returns on a race with
/// another caller installing first (or a no-op call to this same function
/// from a second verifier construction) is expected and intentionally
/// ignored.
pub(crate) fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn assign_once(slot: &mut Option<String>, value: String, name: &str) -> Result<(), AuthError> {
    if slot.is_some() {
        return Err(AuthError::DuplicateSignatureParameter(name.to_string()));
    }

    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AuthError, EnrollResponse, ID_TOKEN_GRANT, build_cose_enroll_response_with_key, challenge,
        challenge_expiry, challenge_signing_key_from, decode_signing_key, parse_bearer_token,
        parse_cose_enroll_response_with_key, parse_signature_header, uses_signature_scheme,
    };

    /// A key used ONLY in tests. Never reuse this (or any other committed
    /// literal) as a real challenge-signing key — see the doc comment on
    /// `challenge_signing_key` for why hardcoded seeds are unsafe here.
    /// Freshly generated for this absorption, distinct from any key that
    /// has ever appeared in this workspace's git history.
    fn test_challenge_signing_key_b64() -> &'static str {
        "HFvcdcj5MyyDnkwLJIVptLdkOefDJ_xQXHby1rIIbFc"
    }

    #[test]
    fn recognizes_signature_prefix() {
        assert!(uses_signature_scheme("Signature keyId=\"vk_123\""));
    }

    #[test]
    fn parses_signature_header() {
        let header = parse_signature_header(
            "Signature keyId=\"vk_123\", timestamp=\"2026-04-24T12:00:00Z\", nonce=\"n_123\", signature=\"sig_123\"",
        )
        .expect("signature header should parse");

        assert_eq!(header.key_id, "vk_123");
        assert_eq!(header.nonce, "n_123");
    }

    #[test]
    fn rejects_unquoted_signature_values() {
        let error = parse_signature_header(
            "Signature keyId=vk_123, timestamp=\"2026-04-24T12:00:00Z\", nonce=\"n_123\", signature=\"sig_123\"",
        )
        .expect_err("unquoted values should be rejected");

        assert!(matches!(error, super::AuthError::MalformedSignatureHeader));
    }

    #[test]
    fn parses_bearer_tokens() {
        assert_eq!(parse_bearer_token("Bearer token-123").unwrap(), "token-123");
    }

    #[test]
    fn issues_cuid_values() {
        let challenge = challenge();
        assert!(challenge.starts_with('c'));
        assert!(
            challenge
                .chars()
                .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
        );
        assert!(challenge_expiry() > chrono::Utc::now());
        assert!(ID_TOKEN_GRANT.contains("id-sd-jwt"));
    }

    /// COSE build/parse logic, tested in isolation from env-var loading —
    /// no process environment is touched (this workspace forbids
    /// `unsafe_code`, and `std::env::set_var` requires `unsafe` as of the
    /// 2024 edition). Env-var loading itself is covered separately by the
    /// two `challenge_signing_key_from` tests below.
    #[test]
    fn round_trips_cose_enroll_responses() {
        let key = decode_signing_key(test_challenge_signing_key_b64()).expect("test key decodes");

        let response = EnrollResponse {
            enrollment_id: "enr_123".to_string(),
            key_id: "vk_123".to_string(),
            challenge: "chl_123".to_string(),
            challenge_format: "cose".to_string(),
            expires_at: chrono::Utc::now(),
        };

        let encoded = build_cose_enroll_response_with_key(&response, &key)
            .expect("cose build should succeed");
        let decoded =
            parse_cose_enroll_response_with_key(&encoded, &key).expect("cose payload should parse");

        assert_eq!(decoded.enrollment_id, response.enrollment_id);
        assert_eq!(decoded.key_id, response.key_id);
        assert_eq!(decoded.challenge, response.challenge);
        assert_eq!(decoded.challenge_format, "cose");
    }

    #[test]
    fn challenge_signing_key_fails_closed_when_env_var_is_absent() {
        let result = challenge_signing_key_from(|_| Err(std::env::VarError::NotPresent));

        match result {
            Ok(_) => panic!("challenge_signing_key must fail closed without a signing key"),
            Err(error) => assert!(matches!(error, AuthError::MissingSigningKeyEnv(_))),
        }
    }

    #[test]
    fn challenge_signing_key_fails_closed_when_env_var_is_whitespace_only() {
        let result = challenge_signing_key_from(|_| Ok("   \n\t  ".to_string()));

        match result {
            Ok(_) => panic!("challenge_signing_key must fail closed on a whitespace-only value"),
            Err(error) => assert!(matches!(error, AuthError::MissingSigningKeyEnv(_))),
        }
    }
}
