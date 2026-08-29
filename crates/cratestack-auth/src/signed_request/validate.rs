//! Per-field validation applied to an incoming signed-request header,
//! independent of key resolution or signature verification itself.

use chrono::{DateTime, Duration, Utc};

use crate::{AuthError, SignatureHeader};

pub(super) fn validate_timestamp(
    timestamp: DateTime<Utc>,
    max_skew: Duration,
) -> Result<(), AuthError> {
    let skew = Utc::now().signed_duration_since(timestamp).abs();
    if skew > max_skew {
        return Err(AuthError::SignatureTimestampOutOfWindow);
    }

    Ok(())
}

pub(super) fn validate_signature_algorithm(alg: Option<&str>) -> Result<(), AuthError> {
    let Some(alg) = alg else {
        return Ok(());
    };

    if alg.eq_ignore_ascii_case("ed25519") || alg.eq_ignore_ascii_case("eddsa") {
        Ok(())
    } else {
        Err(AuthError::UnsupportedSignatureAlgorithm(alg.to_string()))
    }
}

pub(super) fn validate_content_hash(
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
