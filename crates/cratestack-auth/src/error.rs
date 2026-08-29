//! [`AuthError`] and its mapping onto [`cratestack_core::CratestackError`].

use thiserror::Error;

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

pub fn auth_error_to_cratestack_error(error: AuthError) -> cratestack_core::CratestackError {
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
