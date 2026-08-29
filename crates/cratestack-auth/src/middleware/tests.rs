use axum::{Router, body::Body, extract::Extension, http::Request, middleware, routing::post};
use chrono::{SecondsFormat, Utc};
use ed25519_dalek::SigningKey;
use tower::ServiceExt;

use crate::{RequestPrincipal, SignRequestParams, SignedRequestVerifier, sign_request};

use super::require_signed_request;

fn example_signing_key() -> SigningKey {
    // Deterministic test-only key, distinct from any key ever committed
    // as production key material — see `cose_enroll::challenge_signing_key`'s
    // doc comment for why that distinction matters.
    SigningKey::from_bytes(&[
        0x31, 0x8a, 0x77, 0x02, 0x5e, 0x9b, 0x14, 0x3c, 0x8f, 0x21, 0xd4, 0x66, 0x0a, 0x99, 0x5d,
        0x3e, 0x12, 0x87, 0xb0, 0x44, 0x6f, 0x91, 0x2a, 0x5c, 0xe3, 0x08, 0x71, 0x4d, 0x2b, 0x93,
        0x60, 0xf7,
    ])
}

fn example_key_id() -> &'static str {
    "vk_middleware_example"
}

async fn echo_principal(Extension(principal): Extension<RequestPrincipal>) -> String {
    principal.transport.key_id
}

fn app(verifier: SignedRequestVerifier) -> Router {
    Router::new()
        .route("/protected", post(echo_principal))
        .layer(middleware::from_fn_with_state(
            verifier,
            require_signed_request,
        ))
}

#[tokio::test]
async fn rejects_requests_missing_the_authorization_header() {
    let verifier = SignedRequestVerifier::new([(
        example_key_id().to_string(),
        example_signing_key().verifying_key(),
    )]);
    let response = app(verifier)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn accepts_a_validly_signed_request_and_installs_the_principal() {
    let signing_key = example_signing_key();
    let verifier =
        SignedRequestVerifier::new([(example_key_id().to_string(), signing_key.verifying_key())]);
    let body = b"{}";
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &axum::http::Method::POST,
        path: "/protected",
        query: None,
        body,
        timestamp: &timestamp,
        nonce: "middleware-nonce-1",
        key_id: example_key_id(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{timestamp}\", nonce=\"middleware-nonce-1\", signature=\"{signature}\"",
        example_key_id(),
    );

    let response = app(verifier)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/protected")
                .header("authorization", header)
                .body(Body::from(&body[..]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(&bytes[..], example_key_id().as_bytes());
}

#[tokio::test]
async fn rejects_a_tampered_signature() {
    let signing_key = example_signing_key();
    let verifier =
        SignedRequestVerifier::new([(example_key_id().to_string(), signing_key.verifying_key())]);
    let body = b"{}";
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &axum::http::Method::POST,
        path: "/protected",
        query: None,
        body,
        timestamp: &timestamp,
        nonce: "middleware-nonce-2",
        key_id: example_key_id(),
    });
    // Flip the signature's first character — still valid base64url, but
    // no longer a signature over this request.
    let mut tampered_signature = signature.clone();
    tampered_signature.replace_range(0..1, if signature.starts_with('A') { "B" } else { "A" });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{timestamp}\", nonce=\"middleware-nonce-2\", signature=\"{tampered_signature}\"",
        example_key_id(),
    );

    let response = app(verifier)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/protected")
                .header("authorization", header)
                .body(Body::from(&body[..]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}
