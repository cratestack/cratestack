//! SD-JWT disclosure handling: splitting the compact form, hashing/parsing
//! individual disclosures, and verifying them against a token's `_sd[]` digests.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Map as JsonMap, Value};
use sha2::{Digest, Sha256};

use super::claims::IdTokenClaims;
use crate::AuthError;

pub(super) fn split_sd_jwt(token: &str) -> (&str, Vec<String>) {
    let mut iter = token.split('~');
    let jwt = iter.next().unwrap_or(token);
    let disclosures: Vec<String> = iter
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect();
    (jwt, disclosures)
}

pub(super) fn disclosure_digest(disclosure_string: &str) -> String {
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

pub(super) fn verify_disclosures(
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
