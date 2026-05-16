//! `Idempotency-Key` header parsing.

use cratestack_core::CoolError;

/// Parse the `Idempotency-Key` request header. Returns `Ok(None)` if absent.
/// The key must be ASCII and reasonably short to avoid storage abuse.
pub fn parse_idempotency_key(headers: &http::HeaderMap) -> Result<Option<String>, CoolError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let raw = value
        .to_str()
        .map_err(|_| CoolError::BadRequest("Idempotency-Key must be ASCII".to_owned()))?
        .trim();
    if raw.is_empty() {
        return Err(CoolError::BadRequest(
            "Idempotency-Key must not be empty".to_owned(),
        ));
    }
    if raw.len() > 255 {
        return Err(CoolError::BadRequest(
            "Idempotency-Key must be at most 255 characters".to_owned(),
        ));
    }
    Ok(Some(raw.to_owned()))
}
