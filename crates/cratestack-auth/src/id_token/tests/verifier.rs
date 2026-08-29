use ed25519_dalek::SigningKey;

use super::{issue_token_pair, test_issuer_jwk};
use crate::{
    AuthError,
    id_token::{
        DEFAULT_ID_TOKEN_AUDIENCE, IdTokenClaimsParams, IdTokenVerifier, default_id_token_claims,
        verifying_key_jwk,
    },
};

#[tokio::test]
async fn validates_signed_id_tokens_against_jwks() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("jwks test listener should bind");
    let addr = listener
        .local_addr()
        .expect("jwks test addr should resolve");
    let jwks = crate::jwks(vec![test_issuer_jwk()]);
    let router = axum::Router::new().route(
        "/jwks.json",
        axum::routing::get(move || {
            let jwks = jwks.clone();
            async move { axum::Json(jwks) }
        }),
    );
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let claims = default_id_token_claims(IdTokenClaimsParams {
        issuer: "http://127.0.0.1:8081",
        client_id: "example-client",
        subject: "usr_456",
        bound_key_id: "vk_bound",
        bound_key_jwk: None,
        profile_version: 3,
        enrollment_status: "enrolled",
        kyc_status: Some("approved".to_string()),
        main_email: None,
        main_phone: None,
        main_address: None,
        disclosures: Vec::new(),
    });
    let issued = issue_token_pair(claims).expect("token pair should issue");
    let verifier = IdTokenVerifier::new(
        "http://127.0.0.1:8081",
        &format!("http://{addr}/jwks.json"),
        Some("cratestack-issued-tokens"),
    )
    .expect("id token verifier should build");

    let principal = verifier
        .validate(&issued.id_jwt, "vk_bound")
        .await
        .expect("id token should validate");
    assert_eq!(principal.user_id, "usr_456");
    assert_eq!(principal.bound_key_id, "vk_bound");

    server.abort();
}

#[tokio::test]
async fn bound_request_key_resolves_device_key_from_cnf() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("jwks test listener should bind");
    let addr = listener
        .local_addr()
        .expect("jwks test addr should resolve");
    let jwks = crate::jwks(vec![test_issuer_jwk()]);
    let router = axum::Router::new().route(
        "/jwks.json",
        axum::routing::get(move || {
            let jwks = jwks.clone();
            async move { axum::Json(jwks) }
        }),
    );
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let device_key = SigningKey::from_bytes(&[11u8; 32]);
    let device_jwk = verifying_key_jwk(&device_key.verifying_key(), "vk_device");
    let claims = default_id_token_claims(IdTokenClaimsParams {
        issuer: "http://127.0.0.1:8081",
        client_id: "example-client",
        subject: "usr_device",
        bound_key_id: "vk_device",
        bound_key_jwk: Some(device_jwk),
        profile_version: 1,
        enrollment_status: "enrolled",
        kyc_status: None,
        main_email: None,
        main_phone: None,
        main_address: None,
        disclosures: Vec::new(),
    });
    let issued = issue_token_pair(claims).expect("token pair should issue");
    let verifier = IdTokenVerifier::new(
        "http://127.0.0.1:8081",
        &format!("http://{addr}/jwks.json"),
        Some(DEFAULT_ID_TOKEN_AUDIENCE),
    )
    .expect("id token verifier should build");

    // A JWKS-verified id_jwt whose cnf binds vk_device yields that device key.
    let resolved = verifier
        .validate_bound_request_key(&issued.id_jwt, "vk_device")
        .await
        .expect("bound key resolution should succeed")
        .expect("cnf.jwk should be present");
    assert_eq!(resolved, device_key.verifying_key());

    // The cnf binding is enforced: asking for a different request key fails.
    assert!(matches!(
        verifier
            .validate_bound_request_key(&issued.id_jwt, "vk_other")
            .await,
        Err(AuthError::IdTokenBindingMismatch)
    ));

    // A token without cnf.jwk verifies but resolves no key.
    let no_jwk_claims = default_id_token_claims(IdTokenClaimsParams {
        issuer: "http://127.0.0.1:8081",
        client_id: "example-client",
        subject: "usr_device",
        bound_key_id: "vk_service",
        bound_key_jwk: None,
        profile_version: 1,
        enrollment_status: "enrolled",
        kyc_status: None,
        main_email: None,
        main_phone: None,
        main_address: None,
        disclosures: Vec::new(),
    });
    let no_jwk = issue_token_pair(no_jwk_claims).expect("token pair should issue");
    assert!(
        verifier
            .validate_bound_request_key(&no_jwk.id_jwt, "vk_service")
            .await
            .expect("verification should succeed")
            .is_none()
    );

    server.abort();
}
