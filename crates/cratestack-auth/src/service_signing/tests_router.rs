#![cfg(all(test, feature = "axum"))]

use std::collections::HashMap;

use axum::body::to_bytes;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use tower::ServiceExt;

use crate::{AuthError, JwksDocument};

use super::tests_fixtures::{UploadTicketClaims, future_exp};
use super::{MultiIssuerJwksVerifier, ServiceSigningKey, jwks_router};

#[tokio::test]
async fn jwks_router_serves_the_configured_keyset() {
    let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
    let router = jwks_router(key.jwks_document());

    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/jwks.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let document: JwksDocument = serde_json::from_slice(&body).unwrap();
    assert_eq!(document.keys.len(), 1);
    assert_eq!(document.keys[0].kid, "vendor-service-test");

    let well_known = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/.well-known/jwks.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(well_known.status(), 200);
}

#[tokio::test]
async fn end_to_end_mint_and_verify_via_jwks_router() {
    // Fire up the JWKS router on an ephemeral port and verify
    // a token end-to-end against the live HTTP JWKS.
    let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
    let app = jwks_router(key.jwks_document());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let claims = UploadTicketClaims {
        iss: "vendor-service".into(),
        sub: "user_123".into(),
        iat: chrono::Utc::now().timestamp(),
        exp: future_exp(),
        owner_type: "vendor".into(),
        owner_id: "vnd_1".into(),
        purpose: "vendor_logo".into(),
        nonce: "n1".into(),
    };
    let token = key.mint(&claims).unwrap();

    let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
        "vendor-service".to_string(),
        format!("http://{addr}/jwks.json"),
    )]))
    .unwrap();

    let verified = verifier
        .verify::<UploadTicketClaims>(&token)
        .await
        .expect("must verify");
    assert_eq!(verified.issuer, "vendor-service");
    assert_eq!(verified.kid, "vendor-service-test");
    assert_eq!(verified.claims, claims);

    // A second verification reuses the cached JWKS — no HTTP
    // call. We can't directly assert "no HTTP" without a
    // recording client, but at minimum it must still pass.
    let _ = verifier
        .verify::<UploadTicketClaims>(&token)
        .await
        .expect("cached verify");

    server.abort();
}

#[tokio::test]
async fn verifier_rejects_token_with_tampered_payload() {
    let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
    let app = jwks_router(key.jwks_document());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let claims = UploadTicketClaims {
        iss: "vendor-service".into(),
        sub: "user_123".into(),
        iat: chrono::Utc::now().timestamp(),
        exp: future_exp(),
        owner_type: "vendor".into(),
        owner_id: "vnd_1".into(),
        purpose: "vendor_logo".into(),
        nonce: "n1".into(),
    };
    let token = key.mint(&claims).unwrap();
    // Replace the middle segment (claims) with another
    // arbitrary base64 — signature now mismatches.
    let mut parts: Vec<&str> = token.split('.').collect();
    let evil_claims = URL_SAFE_NO_PAD
        .encode(b"{\"iss\":\"vendor-service\",\"exp\":99999999999,\"sub\":\"attacker\"}");
    parts[1] = &evil_claims;
    let tampered = parts.join(".");

    let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
        "vendor-service".to_string(),
        format!("http://{addr}/jwks.json"),
    )]))
    .unwrap();

    let err = verifier
        .verify::<UploadTicketClaims>(&tampered)
        .await
        .expect_err("must reject tampered payload");
    assert!(matches!(err, AuthError::IdTokenVerificationFailed));

    server.abort();
}
