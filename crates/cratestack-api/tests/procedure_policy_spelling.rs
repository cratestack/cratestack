//! GHSA-69g4-xvcm-vm2j over HTTP: a procedure's `@deny` must refuse the
//! caller it names however the line around it is written.
//!
//! Before the fix, `transferCommented` and `transferShared` answered
//! `200 OK` to a `banned` caller: the generator recognised a policy only
//! when the whole attribute line was exactly `@deny(...)`, so a trailing
//! `// comment` or a preceding `@no_idempotency` on the same line made it
//! skip the rule with no diagnostic. `transferCanonical` is the control
//! that has always answered `403`.

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackContext, CratestackError, Value};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/procedure_policy_spelling.cstack", db = None);

use cratestack_schema::procedures as procs;

#[derive(Clone, Default)]
struct Procedures;

fn receipt() -> Result<cratestack_schema::Receipt, CratestackError> {
    Ok(cratestack_schema::Receipt { ok: true })
}

impl procs::ProcedureRegistry for Procedures {
    async fn transfer_canonical(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: procs::transfer_canonical::Args,
        _authorized: procs::transfer_canonical::Authorized,
    ) -> Result<procs::transfer_canonical::Output, CratestackError> {
        receipt()
    }

    async fn transfer_commented(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: procs::transfer_commented::Args,
        _authorized: procs::transfer_commented::Authorized,
    ) -> Result<procs::transfer_commented::Output, CratestackError> {
        receipt()
    }

    async fn transfer_shared(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: procs::transfer_shared::Args,
        _authorized: procs::transfer_shared::Authorized,
    ) -> Result<procs::transfer_shared::Output, CratestackError> {
        receipt()
    }
}

/// Authenticates every request, with the role from `x-role`.
#[derive(Clone)]
struct HeaderRole;

impl cratestack::AuthProvider for HeaderRole {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let role = request
            .headers
            .get("x-role")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("user")
            .to_owned();
        core::future::ready(Ok(CratestackContext::authenticated([
            ("id".to_owned(), Value::Int(7)),
            ("role".to_owned(), Value::String(role)),
        ])))
    }
}

async fn post(route: &str, role: &str) -> StatusCode {
    let app = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        JsonCodec,
        HeaderRole,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let body = serde_json::json!({ "args": { "accountId": 1, "amount": 5 } });
    let request = Request::post(format!("/$procs/{route}"))
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .header("x-role", role)
        .body(Body::from(serde_json::to_vec(&body).expect("body encodes")))
        .expect("request builds");
    let response = app.oneshot(request).await.expect("router answers");
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    status
}

const ROUTES: [&str; 3] = ["transferCanonical", "transferCommented", "transferShared"];

#[tokio::test]
async fn every_spelling_refuses_the_caller_its_deny_names() {
    for route in ROUTES {
        assert_eq!(
            post(route, "banned").await,
            StatusCode::FORBIDDEN,
            "{route}"
        );
    }
}

#[tokio::test]
async fn every_spelling_admits_the_caller_its_allow_names() {
    for route in ROUTES {
        assert_eq!(post(route, "user").await, StatusCode::OK, "{route}");
    }
}

#[test]
fn every_spelling_generates_its_deny_policy() {
    assert_eq!(procs::transfer_canonical::DENY_POLICIES.len(), 1);
    assert_eq!(procs::transfer_commented::DENY_POLICIES.len(), 1);
    assert_eq!(procs::transfer_shared::DENY_POLICIES.len(), 1);
}
