//! id-token cnf-bound proof-of-possession fallback: a device key unknown to
//! every other resolver tier is accepted when vouched for by a trusted
//! issuer's id_jwt — UNLESS a wired `DeviceKeyResolver` has already given an
//! authoritative "no such key" (e.g. a revoked device), which must win.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use ed25519_dalek::{SigningKey, VerifyingKey};
use http::Method;
use tokio::task::JoinHandle;

use crate::signed_request::{
    DeviceKeyResolver, SignRequestParams, SignedRequestVerifier, sign_request,
};
use crate::{AuthError, IdTokenClaimsParams, IdTokenVerifier};

const ISSUER_KID: &str = "issuer-test-v1";
const ISSUER_URL: &str = "http://127.0.0.1:8081";

/// Spins up a throwaway JWKS endpoint publishing a fresh issuer key, and
/// mints an id_jwt (for `subject`) whose `cnf` binds `device_key`/
/// `device_key_id`. Shared by both tests below, which differ only in
/// whether a `DeviceKeyResolver` is wired (and thus whether the PoP
/// fallback runs at all).
async fn spawn_bound_id_token(
    device_key: &SigningKey,
    device_key_id: &str,
    subject: &str,
) -> (String, IdTokenVerifier, JoinHandle<()>) {
    let issuer_key = SigningKey::from_bytes(&[17u8; 32]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let jwks_doc = crate::jwks(vec![crate::issuer_jwk(&issuer_key, ISSUER_KID)]);
    let router = axum::Router::new().route(
        "/jwks.json",
        axum::routing::get(move || {
            let jwks_doc = jwks_doc.clone();
            async move { axum::Json(jwks_doc) }
        }),
    );
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let claims = crate::default_id_token_claims(IdTokenClaimsParams {
        issuer: ISSUER_URL,
        client_id: "example-client",
        subject,
        bound_key_id: device_key_id,
        bound_key_jwk: Some(crate::verifying_key_jwk(
            &device_key.verifying_key(),
            device_key_id,
        )),
        profile_version: 1,
        enrollment_status: "enrolled",
        kyc_status: None,
        main_email: None,
        main_phone: None,
        main_address: None,
        disclosures: Vec::new(),
    });
    let id_jwt = crate::issue_id_token(&issuer_key, ISSUER_KID, &claims).unwrap();
    let id_verifier = IdTokenVerifier::new(
        ISSUER_URL,
        &format!("http://{addr}/jwks.json"),
        Some(crate::DEFAULT_ID_TOKEN_AUDIENCE),
    )
    .unwrap();

    (id_jwt, id_verifier, server)
}

#[tokio::test]
async fn verifies_device_signed_request_via_cnf_bound_id_token() {
    // A device key present in NO trusted-key map, JWKS, or device resolver —
    // exactly the prod situation for a non-auth service.
    let device_key = SigningKey::from_bytes(&[13u8; 32]);
    let device_key_id = "vk_smoke_device";
    let (id_jwt, id_verifier, server) =
        spawn_bound_id_token(&device_key, device_key_id, "usr_smoke").await;

    // No trusted keys, no device resolver — only the id-token verifier, like
    // a non-auth service built via `from_env().with_id_token_verifier(...)`.
    let verifier = SignedRequestVerifier::new(Vec::<(String, VerifyingKey)>::new())
        .with_id_token_verifier(id_verifier);

    let body = br#"{"args":{}}"#;
    let path = "/rpc/procedure.myVendorContexts";
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &device_key,
        method: &Method::POST,
        path,
        query: None,
        body,
        timestamp: &timestamp,
        nonce: "nonce-pop-1",
        key_id: device_key_id,
    });
    let header = format!(
        "Signature keyId=\"{device_key_id}\", timestamp=\"{timestamp}\", nonce=\"nonce-pop-1\", signature=\"{signature}\", alg=\"Ed25519\", id_jwt=\"{id_jwt}\""
    );

    let principal = verifier
        .verify(&Method::POST, &path.parse().unwrap(), body, &header)
        .await
        .expect("device-signed request should verify via cnf-bound id_jwt");
    assert_eq!(principal.key_id, device_key_id);
    // Tagged as PoP-resolved so internal middleware can reject end-user callers.
    assert!(principal.via_id_token_pop);

    // The same request WITHOUT the id_jwt is unresolvable (no PoP anchor).
    let header_no_jwt = format!(
        "Signature keyId=\"{device_key_id}\", timestamp=\"{timestamp}\", nonce=\"nonce-pop-2\", signature=\"{signature}\", alg=\"Ed25519\""
    );
    assert!(matches!(
        verifier
            .verify(&Method::POST, &path.parse().unwrap(), body, &header_no_jwt)
            .await,
        Err(AuthError::UnknownSigningKey(_))
    ));

    server.abort();
}

#[tokio::test]
async fn device_resolver_none_is_authoritative_over_cnf_fallback() {
    // Regression: a wired DeviceKeyResolver returning None (how auth-service
    // reports a REVOKED/disabled device) must be final — the cnf-bound PoP
    // fallback must NOT run and resurrect the device via its stale id_jwt.
    let device_key = SigningKey::from_bytes(&[13u8; 32]);
    let device_key_id = "vk_revoked_device";
    let (id_jwt, id_verifier, server) =
        spawn_bound_id_token(&device_key, device_key_id, "usr_revoked").await;

    // Resolver that always reports "no such active key" — the revoked case.
    struct RevokedResolver;
    #[async_trait]
    impl DeviceKeyResolver for RevokedResolver {
        async fn lookup_device_verifying_key(
            &self,
            _key_id: &str,
        ) -> Result<Option<VerifyingKey>, AuthError> {
            Ok(None)
        }
    }
    let verifier = SignedRequestVerifier::new(Vec::<(String, VerifyingKey)>::new())
        .with_id_token_verifier(id_verifier)
        .with_device_key_resolver(Arc::new(RevokedResolver));

    let body = br#"{"args":{}}"#;
    let path = "/rpc/procedure.myDevices";
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &device_key,
        method: &Method::POST,
        path,
        query: None,
        body,
        timestamp: &timestamp,
        nonce: "nonce-revoked",
        key_id: device_key_id,
    });
    let header = format!(
        "Signature keyId=\"{device_key_id}\", timestamp=\"{timestamp}\", nonce=\"nonce-revoked\", signature=\"{signature}\", alg=\"Ed25519\", id_jwt=\"{id_jwt}\""
    );

    // Even with a perfectly valid cnf-bound id_jwt, the resolver's None wins.
    assert!(matches!(
        verifier
            .verify(&Method::POST, &path.parse().unwrap(), body, &header)
            .await,
        Err(AuthError::UnknownSigningKey(_))
    ));

    server.abort();
}
