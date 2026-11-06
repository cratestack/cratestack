//! `@authn` on the REST binding (ADR 0022, cratestack#1244), through the
//! real generated `router()`: a caller who meets the requirement is served,
//! one who does not gets RFC 9470's step-up 401, and `@allow` still answers
//! first.

mod authn_support;

use authn_support::{HeaderProvider, PAY_CHALLENGE, STALE_WEAK, STRONG, pay_details_all_unmet};
use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode, header::WWW_AUTHENTICATE};
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_json::JsonCodec;
use cratestack_schema::procedures::{balance, close, pay};
use tower::ServiceExt;

include_server_schema!("tests/fixtures/authn_rest.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

fn ok() -> cratestack_schema::Receipt {
    cratestack_schema::Receipt { ok: true }
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn pay(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: pay::Args,
        _authorized: pay::Authorized,
    ) -> Result<pay::Output, CratestackError> {
        Ok(ok())
    }

    async fn close(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: close::Args,
        _authorized: close::Authorized,
    ) -> Result<close::Output, CratestackError> {
        Ok(ok())
    }

    async fn balance(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: balance::Args,
        _authorized: balance::Authorized,
    ) -> Result<balance::Output, CratestackError> {
        Ok(ok())
    }
}

async fn call(
    procedure: &str,
    caller: &[(&str, &str)],
) -> (StatusCode, Vec<String>, serde_json::Value) {
    let app = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        JsonCodec,
        HeaderProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let mut request = Request::post(format!("/$procs/{procedure}"))
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE);
    for (name, value) in caller {
        request = request.header(*name, *value);
    }
    let request = request
        .body(Body::from(r#"{"args":{"amount":5}}"#))
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
async fn a_caller_who_meets_the_requirement_is_served() {
    let (status, _, body) = call("pay", STRONG).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn a_stale_weak_caller_gets_the_rfc_9470_step_up() {
    let (status, challenges, body) = call("pay", STALE_WEAK).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(challenges, [PAY_CHALLENGE]);
    assert_eq!(
        body,
        serde_json::json!({
            "code": "UNAUTHORIZED",
            "message": "step-up authentication required",
            "details": pay_details_all_unmet(),
            "error": "insufficient_user_authentication",
        })
    );
}

#[tokio::test]
async fn only_the_unmet_part_is_named() {
    let fresh_but_password: &[(&str, &str)] = &[
        ("x-sub", "usr-1"),
        ("x-auth-age", "5"),
        ("x-amr", "pwd"),
        ("x-acr", "aal3"),
    ];
    let (status, _, body) = call("pay", fresh_but_password).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["details"]["unmet"], serde_json::json!(["amr"]));
}

#[tokio::test]
async fn allow_answers_before_the_requirement() {
    let (status, challenges, body) = call("close", STALE_WEAK).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(challenges.is_empty());
    let admin_stale: &[(&str, &str)] = &[
        ("x-sub", "usr-2"),
        ("x-roles", "admin"),
        ("x-auth-age", "61"),
    ];
    let (status, _, body) = call("close", admin_stale).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body["details"],
        serde_json::json!({ "max_age": 60, "unmet": ["max_age"] })
    );
    let admin_fresh: &[(&str, &str)] = &[
        ("x-sub", "usr-2"),
        ("x-roles", "admin"),
        ("x-auth-age", "1"),
    ];
    assert_eq!(call("close", admin_fresh).await.0, StatusCode::OK);
}

#[tokio::test]
async fn an_operation_without_a_requirement_is_unchanged() {
    let (status, _, _) = call("balance", STALE_WEAK).await;
    assert_eq!(status, StatusCode::OK);
}
