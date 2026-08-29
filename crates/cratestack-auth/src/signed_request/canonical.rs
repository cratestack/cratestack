//! Canonicalisation of the request onto the exact byte string that gets
//! signed / verified, and the signing helper built on top of it.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::Signer;
use http::Method;
use sha2::{Digest, Sha256};
use url::form_urlencoded;

use super::types::SignRequestParams;

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
