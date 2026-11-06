//! cratestack#1243 on the sync surface: an `AuthProvider` refusal keeps its
//! status, its members and `details` (top-level in the sync error body, as
//! on REST) and its `WWW-Authenticate` challenges, on the JSON endpoints and
//! on the poke stream. A plain provider error keeps its status class and
//! reads as its public message, never an operator detail.
//!
//! No database: every request here is refused by the provider, before the
//! server touches its pool, so the pool is a lazy one that never connects.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header::WWW_AUTHENTICATE};
use cratestack_core::{AuthRefusal, CratestackContext, CratestackError, Value};
use cratestack_sqlx::sqlx::PgPool;
use cratestack_sync::protocol::{SyncErrorBody, SyncErrorCode};
use cratestack_sync::server::{SyncServer, sync_router};
use serde_json::json;
use tower::ServiceExt;

fn provider(headers: &HeaderMap) -> Result<CratestackContext, CratestackError> {
    let case = headers.get("x-refusal").and_then(|v| v.to_str().ok());
    Err(match case {
        Some("renew") => AuthRefusal::unauthenticated("attestation expired")
            .with_member("error", Value::String("attestation_expired".into()))
            .with_challenge(r#"Signature error="attestation_expired""#)
            .into_error(),
        Some("revoked") => AuthRefusal::forbidden("device revoked")
            .with_member("error", Value::String("DEVICE_REVOKED".into()))
            .with_details(Value::Map(
                [("device".to_owned(), Value::String("d-1".into()))].into(),
            ))
            .into_error(),
        Some("reserved") => AuthRefusal::unauthenticated("kept")
            .with_member("lastMutationSeq", Value::Int(99))
            .with_member("retryAfterSecs", Value::Int(99))
            .into_error(),
        Some("forbidden") => CratestackError::Forbidden("not this device".into()),
        Some("internal") => CratestackError::Internal("jwks down at 10.0.0.7 secret".into()),
        _ => CratestackError::Unauthorized("no credential".into()),
    })
}

fn app() -> Router {
    let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
    let server = SyncServer::builder(pool, Default::default(), |_: &CratestackContext| None)
        .build()
        .unwrap();
    sync_router(server, provider)
}

struct Answer {
    status: StatusCode,
    challenges: Vec<String>,
    json: serde_json::Value,
}

async fn send(request: axum::http::request::Builder, case: &str, body: Body) -> Answer {
    let request = request.header("x-refusal", case).body(body).unwrap();
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let challenges = response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap();
    Answer {
        status,
        challenges,
        json,
    }
}

async fn register(case: &str) -> Answer {
    let request = Request::post("/sync/register").header("content-type", "application/json");
    send(request, case, Body::from("{}")).await
}

#[tokio::test]
async fn a_401_refusal_keeps_its_members_and_challenge() {
    let answer = register("renew").await;

    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        answer.json,
        json!({
            "code": "unauthenticated",
            "message": "attestation expired",
            "error": "attestation_expired",
        })
    );
    assert_eq!(
        answer.challenges,
        [r#"Signature error="attestation_expired""#]
    );

    // The sync client reads the member back.
    let body: SyncErrorBody = serde_json::from_value(answer.json).unwrap();
    assert_eq!(body.code, SyncErrorCode::Unauthenticated);
    assert_eq!(
        body.extra["error"],
        Value::String("attestation_expired".into())
    );
}

#[tokio::test]
async fn a_403_refusal_is_forbidden_with_its_details() {
    let answer = register("revoked").await;

    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert_eq!(
        answer.json,
        json!({
            "code": "forbidden",
            "message": "device revoked",
            "details": { "device": "d-1" },
            "error": "DEVICE_REVOKED",
        })
    );
    assert!(answer.challenges.is_empty());
}

/// A member cannot pose as one of the body's own fields.
#[tokio::test]
async fn a_member_named_like_a_sync_field_is_never_written() {
    let answer = register("reserved").await;

    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        answer.json,
        json!({ "code": "unauthenticated", "message": "kept" })
    );
}

/// A plain error: its public message, no member and no header.
#[tokio::test]
async fn a_plain_provider_error_reads_as_its_public_message() {
    let answer = register("plain").await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        answer.json,
        json!({ "code": "unauthenticated", "message": "no credential" })
    );
    assert!(answer.challenges.is_empty());

    // A caller the provider knows and refuses is not told to sign in.
    let answer = register("forbidden").await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert_eq!(
        answer.json,
        json!({ "code": "forbidden", "message": "not this device" })
    );
}

/// A provider that could not decide is a 500 with the canned message: the
/// detail stays in the log, and the client is not sent to sign in again.
#[tokio::test]
async fn a_provider_failure_is_internal_and_leaks_nothing() {
    let answer = register("internal").await;

    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        answer.json,
        json!({ "code": "internal", "message": "internal error" })
    );
    let text = answer.json.to_string();
    assert!(
        !text.contains("secret") && !text.contains("10.0.0.7"),
        "{text}"
    );
}

/// The poke stream authenticates through the same path and answers a
/// refusal the same way.
#[tokio::test]
async fn the_poke_stream_renders_a_refusal_too() {
    let request = Request::get("/sync/poke?clientId=00000000-0000-4000-8000-000000000001");
    let answer = send(request, "renew", Body::empty()).await;

    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.json["error"], "attestation_expired");
    assert_eq!(
        answer.challenges,
        [r#"Signature error="attestation_expired""#]
    );
}
