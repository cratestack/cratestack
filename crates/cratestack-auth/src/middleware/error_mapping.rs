//! Maps an [`AuthError`] onto the HTTP status + stable error code the
//! signed-request middleware returns to the caller.
//!
//! Kept separate from the middleware itself because the mapping is a
//! standalone policy table: which failures are the caller's fault (401),
//! which are a wrong-audience caller (403), and which are the server's own
//! dependency being unavailable (503). The catch-all deliberately collapses
//! every remaining signature failure into one opaque `signature_invalid`
//! response so the exact reason a signature failed isn't handed back to an
//! unauthenticated caller.

use axum::{http::StatusCode, response::Response};

use crate::AuthError;
use crate::response::error_response;

pub(super) fn auth_error_response(error: AuthError) -> Response {
    let (status, code, message) = match error {
        AuthError::MissingAuthorizationHeader => (
            StatusCode::UNAUTHORIZED,
            "signature_required",
            "Protected endpoints require Authorization: Signature ...",
        ),
        AuthError::RequestBodyRead(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "request_body_unreadable",
            "Failed to read the request body for signature verification",
        ),
        AuthError::NonceStoreUnavailable(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "nonce_store_unavailable",
            "Replay protection storage is unavailable for signed-request verification",
        ),
        AuthError::IdTokenJwksUnavailable(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "id_token_verification_unavailable",
            "Identity token verification is temporarily unavailable",
        ),
        AuthError::InternalEndpointForbidden => (
            StatusCode::FORBIDDEN,
            "service_caller_required",
            "This endpoint is restricted to trusted service callers",
        ),
        AuthError::NonceReused => (
            StatusCode::UNAUTHORIZED,
            "nonce_reused",
            "Request nonce has already been used within the active replay window",
        ),
        AuthError::SignatureTimestampOutOfWindow | AuthError::InvalidSignatureTimestamp(_) => (
            StatusCode::UNAUTHORIZED,
            "timestamp_invalid",
            "Request timestamp is outside the accepted skew window",
        ),
        AuthError::IdTokenDecoding(_)
        | AuthError::UnknownIdTokenSigningKey(_)
        | AuthError::UnsupportedIdTokenAlgorithm(_)
        | AuthError::IdTokenVerificationFailed
        | AuthError::IdTokenIssuerMismatch
        | AuthError::IdTokenAudienceMismatch
        | AuthError::IdTokenExpired
        | AuthError::IdTokenBindingMismatch => (
            StatusCode::UNAUTHORIZED,
            "id_token_invalid",
            "Request id_jwt could not be validated against the configured issuer",
        ),
        _ => (
            StatusCode::UNAUTHORIZED,
            "signature_invalid",
            "Request signature verification failed",
        ),
    };

    error_response(status, code, message)
}
