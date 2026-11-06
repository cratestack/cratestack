//! cratestack#1243 story 9: `SignedRequestAuthProvider`, CrateStack's own
//! HTTP-signature provider, passes the conformance kit. It signs exactly the
//! canonical request (ADR 0021 §4): the declared route with no mount prefix,
//! the query where the operation reads it, the exact body bytes.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{SecondsFormat, Utc};
use cratestack_auth::{
    SignRequestParams, SignedRequestAuthProvider, SignedRequestVerifier, TransportCallerMode,
    content_sha256_base64url, sign_request,
};
use cratestack_auth_conformance::{Canonical, Expected, auth_conformance};
use ed25519_dalek::SigningKey;
use http::{HeaderMap, HeaderValue, Method};

const KEY_ID: &str = "kit-service";

/// One `Authorization: Signature ...` header over `path`, with a fresh nonce
/// (the verifier refuses a nonce it has seen in its replay window).
fn signature(key: &SigningKey, canonical: &Canonical, path: &str) -> HeaderMap {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = format!("kit-{}", NONCE.fetch_add(1, Ordering::Relaxed));
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let method = Method::from_bytes(canonical.method.as_bytes()).expect("a valid method");
    let signature = sign_request(SignRequestParams {
        signing_key: key,
        method: &method,
        path,
        query: canonical.query,
        body: &canonical.body,
        timestamp: &timestamp,
        nonce: &nonce,
        key_id: KEY_ID,
    });
    let value = format!(
        "Signature keyId=\"{KEY_ID}\", timestamp=\"{timestamp}\", nonce=\"{nonce}\", \
         signature=\"{signature}\", alg=\"Ed25519\", content_sha256=\"{}\"",
        content_sha256_base64url(&canonical.body),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_str(&value).expect("an ASCII header"),
    );
    headers
}

#[tokio::test]
async fn signed_request_auth_provider_conforms() {
    let key = SigningKey::from_bytes(&[0x44; 32]);
    let verifier = SignedRequestVerifier::new([(KEY_ID.to_owned(), key.verifying_key())]);
    let provider = SignedRequestAuthProvider::new(verifier)
        .allow_transport_callers(TransportCallerMode::AllMethods);

    let signed = {
        let key = key.clone();
        move |canonical: &Canonical| signature(&key, canonical, canonical.path)
    };
    // A client that signs the URL it sent, mount prefix included, signs bytes
    // the server never verifies: refused on every transport, mounted or not
    // (at the root the prefix is empty, so the extra segment is added).
    let signed_with_the_prefix = move |canonical: &Canonical| {
        let path = format!("{}/prefixed{}", canonical.mount, canonical.path);
        signature(&key, canonical, &path)
    };

    auth_conformance(provider)
        .accepts(format!("svc:{KEY_ID}"), signed)
        .refuses(
            "a signature over a path other than the canonical one",
            signed_with_the_prefix,
            Expected::unauthorized(Some("signature verification failed")),
        )
        .run()
        .await;
}
