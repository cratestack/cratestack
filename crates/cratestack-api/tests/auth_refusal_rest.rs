//! cratestack#1208 on the REST binding: an `AuthProvider` refusal sets the
//! response status, adds members next to `code` and `message`, and sends
//! `WWW-Authenticate`, through the real generated `router()` (`db = None`,
//! no network, `oneshot`). The RPC binding's half is `auth_refusal_rpc.rs`.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode, header::WWW_AUTHENTICATE};
use cratestack::{AuthRefusal, CratestackContext, CratestackError, Value, include_server_schema};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/trusted_proxy_client_ip.cstack", db = None);

/// Refuses according to the `x-refusal` request header, so one router
/// covers every case.
#[derive(Clone)]
struct RefusingAuthProvider;

impl cratestack::AuthProvider for RefusingAuthProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> Result<CratestackContext, Self::Error> {
        let case = request
            .headers
            .get("x-refusal")
            .and_then(|v| v.to_str().ok());
        Err(match case {
            Some("renew") => AuthRefusal::unauthenticated("attestation expired")
                .with_member("error", Value::String("attestation_expired".into()))
                .with_challenge(r#"Signature error="attestation_expired""#)
                .into_error(),
            Some("revoked") => AuthRefusal::forbidden("device revoked")
                .with_member("error", Value::String("device_revoked".into()))
                .with_details(Value::Map(
                    [("device".to_owned(), Value::String("d-1".into()))].into(),
                ))
                .into_error(),
            Some("reserved") => AuthRefusal::unauthenticated("kept")
                .with_member("code", Value::String("HIJACKED".into()))
                .with_member("message", Value::String("hijacked".into()))
                .with_member("details", Value::String("hijacked".into()))
                .into_error(),
            _ => CratestackError::Unauthorized("no credential".into()),
        })
    }
}

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn who_am_i(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: cratestack_schema::procedures::who_am_i::Args,
        _authorized: cratestack_schema::procedures::who_am_i::Authorized,
    ) -> Result<cratestack_schema::procedures::who_am_i::Output, CratestackError> {
        Ok("unreachable".to_owned())
    }
}

/// Sends `whoAmI` with the given refusal case; returns the status, the
/// `WWW-Authenticate` values and the decoded JSON body.
async fn call(case: &str) -> (StatusCode, Vec<String>, serde_json::Value) {
    let db = cratestack_schema::Cratestack::builder().build();
    let app = cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        RefusingAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = Request::post("/$procs/whoAmI")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .header("x-refusal", case)
        .body(Body::from(r#"{"args":{}}"#))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let challenges = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, challenges, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn an_unauthenticated_refusal_carries_its_member_and_challenge() {
    let (status, challenges, body) = call("renew").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(challenges, [r#"Signature error="attestation_expired""#]);
    assert_eq!(
        body,
        serde_json::json!({
            "code": "UNAUTHORIZED",
            "message": "attestation expired",
            "details": null,
            "error": "attestation_expired",
        }),
    );
}

#[tokio::test]
async fn a_forbidden_refusal_is_a_403_with_its_details() {
    let (status, challenges, body) = call("revoked").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(challenges.is_empty());
    assert_eq!(body["code"], "FORBIDDEN");
    assert_eq!(body["error"], "device_revoked");
    assert_eq!(body["details"], serde_json::json!({ "device": "d-1" }));
}

#[tokio::test]
async fn a_refusal_cannot_replace_code_message_or_details() {
    let (status, _, body) = call("reserved").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        serde_json::json!({ "code": "UNAUTHORIZED", "message": "kept", "details": null }),
    );
}

/// A provider that keeps returning `CratestackError::Unauthorized` gets the
/// envelope it always got: no extra member, no header.
#[tokio::test]
async fn a_plain_unauthorized_is_unchanged() {
    let (status, challenges, body) = call("plain").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(challenges.is_empty());
    assert_eq!(
        body,
        serde_json::json!({ "code": "UNAUTHORIZED", "message": "no credential", "details": null }),
    );
}
