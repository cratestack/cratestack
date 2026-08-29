//! JWKS-backed key resolution: fall-through for a `kid` unknown to the
//! static map, a rotation window where two kids are simultaneously valid,
//! and the static map short-circuiting JWKS when both know a `kid`.

use chrono::{SecondsFormat, Utc};
use http::Method;

use crate::AuthError;
use crate::signed_request::{SignRequestParams, SignedRequestVerifier, sign_request};

/// Rotation simulation: signer rolls from `kid_v1` to `kid_v2`
/// while both keys live in JWKS. The verifier picks the right
/// VerifyingKey by `kid` without any env-var update on the
/// receiver side.
#[tokio::test]
async fn jwks_resolver_falls_through_for_unknown_static_kid() {
    use crate::service_signing::{MultiIssuerJwksVerifier, ServiceSigningKey};
    use crate::{JwksDocument, issuer_jwk};
    use axum::Router;
    use std::collections::HashMap;
    use std::sync::Arc;

    // Two signing keys live behind the issuer's JWKS at once —
    // simulating the `current` + `next` window during rotation.
    let key_v1 = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-v1");
    let key_v2 = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-v2");

    let combined_jwks = JwksDocument {
        keys: vec![
            issuer_jwk(key_v1.signing_key(), "vendor-service-v1"),
            issuer_jwk(key_v2.signing_key(), "vendor-service-v2"),
        ],
    };
    let jwks = Arc::new(combined_jwks);
    let app = Router::new().route(
        "/jwks.json",
        axum::routing::get({
            let jwks = jwks.clone();
            move || {
                let jwks = jwks.clone();
                async move { axum::Json::<JwksDocument>((*jwks).clone()) }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resolver = MultiIssuerJwksVerifier::new(HashMap::from([(
        "vendor-service".to_owned(),
        format!("http://{addr}/jwks.json"),
    )]))
    .unwrap();

    // Static map is empty — every lookup must go through JWKS.
    let verifier =
        SignedRequestVerifier::new(std::iter::empty::<(String, _)>()).with_jwks_resolver(resolver);

    // Sign with v2 — the kid the verifier has never seen statically.
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: key_v2.signing_key(),
        method: &Method::POST,
        path: "/uploads/presign",
        query: None,
        body: br#"{"hi":1}"#,
        timestamp: &timestamp,
        nonce: "nonce-rot-v2",
        key_id: "vendor-service-v2",
    });
    let header = format!(
        "Signature keyId=\"vendor-service-v2\", timestamp=\"{timestamp}\", nonce=\"nonce-rot-v2\", signature=\"{signature}\""
    );

    let principal = verifier
        .verify(
            &Method::POST,
            &"/uploads/presign".parse().unwrap(),
            br#"{"hi":1}"#,
            &header,
        )
        .await
        .expect("v2 signature should resolve via JWKS");
    assert_eq!(principal.key_id, "vendor-service-v2");

    // v1 is also still accepted (the rotation window).
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: key_v1.signing_key(),
        method: &Method::POST,
        path: "/uploads/presign",
        query: None,
        body: br#"{"hi":2}"#,
        timestamp: &timestamp,
        nonce: "nonce-rot-v1",
        key_id: "vendor-service-v1",
    });
    let header = format!(
        "Signature keyId=\"vendor-service-v1\", timestamp=\"{timestamp}\", nonce=\"nonce-rot-v1\", signature=\"{signature}\""
    );
    let principal = verifier
        .verify(
            &Method::POST,
            &"/uploads/presign".parse().unwrap(),
            br#"{"hi":2}"#,
            &header,
        )
        .await
        .expect("v1 signature should also resolve");
    assert_eq!(principal.key_id, "vendor-service-v1");

    // Unknown kid still fails with UnknownSigningKey.
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: key_v2.signing_key(),
        method: &Method::POST,
        path: "/uploads/presign",
        query: None,
        body: br#"{"hi":3}"#,
        timestamp: &timestamp,
        nonce: "nonce-rot-bad",
        key_id: "vendor-service-v99",
    });
    let header = format!(
        "Signature keyId=\"vendor-service-v99\", timestamp=\"{timestamp}\", nonce=\"nonce-rot-bad\", signature=\"{signature}\""
    );
    let result = verifier
        .verify(
            &Method::POST,
            &"/uploads/presign".parse().unwrap(),
            br#"{"hi":3}"#,
            &header,
        )
        .await;
    assert!(matches!(result, Err(AuthError::UnknownSigningKey(_))));

    // Static map is preferred over JWKS when both have the kid —
    // rebuild a verifier with v1 in the static map; v1 lookup
    // should not touch the JWKS server.
    let resolver_unreachable = MultiIssuerJwksVerifier::new(HashMap::from([(
        "vendor-service".to_owned(),
        "http://127.0.0.1:1/jwks.json".to_owned(),
    )]))
    .unwrap();
    let verifier = SignedRequestVerifier::new([(
        "vendor-service-v1".to_owned(),
        key_v1.signing_key().verifying_key(),
    )])
    .with_jwks_resolver(resolver_unreachable);
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: key_v1.signing_key(),
        method: &Method::POST,
        path: "/uploads/presign",
        query: None,
        body: br#"{"hi":4}"#,
        timestamp: &timestamp,
        nonce: "nonce-static-pref",
        key_id: "vendor-service-v1",
    });
    let header = format!(
        "Signature keyId=\"vendor-service-v1\", timestamp=\"{timestamp}\", nonce=\"nonce-static-pref\", signature=\"{signature}\""
    );
    verifier
        .verify(
            &Method::POST,
            &"/uploads/presign".parse().unwrap(),
            br#"{"hi":4}"#,
            &header,
        )
        .await
        .expect("static map should short-circuit JWKS");

    server.abort();
}
