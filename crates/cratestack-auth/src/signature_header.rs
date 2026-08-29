//! Parsing of the `Authorization: Signature ...` header and of plain
//! `Authorization: Bearer ...` tokens.

use crate::SIGNATURE_SCHEME;
use crate::error::AuthError;

#[cfg(test)]
mod tests;

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

fn assign_once(slot: &mut Option<String>, value: String, name: &str) -> Result<(), AuthError> {
    if slot.is_some() {
        return Err(AuthError::DuplicateSignatureParameter(name.to_string()));
    }

    *slot = Some(value);
    Ok(())
}
