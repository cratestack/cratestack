//! SPIKE (`spike/b1-internal-actions`): end-to-end proof against a
//! **really generated** axum router, not just the macro's token
//! output.
//!
//! The unit tests in `cratestack-macros` assert on token streams,
//! which proves the codegen branch is taken but not that the mounted
//! router actually behaves differently. These tests drive the router
//! `include_server_schema!` produces and check the HTTP status axum
//! returns for each verb.
//!
//! No database is touched: the pool is built with `connect_lazy`, and
//! every assertion here is about a request that axum resolves at the
//! routing layer (404 / 405) before any handler runs. That is exactly
//! the point — a suppressed route must not exist, so it can never
//! reach a handler, a policy, or a connection.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CoolContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/internal_actions_spike.cstack",
    db = Postgres
);

#[derive(Clone)]
struct SubjectAuthProvider;

impl AuthProvider for SubjectAuthProvider {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let ctx = match request
            .headers
            .get("x-subject-id")
            .and_then(|value| value.to_str().ok())
        {
            Some(subject) => CoolContext::authenticated([(
                "subjectId".to_owned(),
                Value::String(subject.to_owned()),
            )]),
            None => CoolContext::anonymous(),
        };
        core::future::ready(Ok(ctx))
    }
}

fn router() -> cratestack::axum::Router {
    // Short acquire timeout: the routed-but-unsuppressed cases do
    // reach a handler and try to talk to a database that isn't there.
    // We only care that they got past routing, so fail fast rather
    // than sit on the default 30s timeout.
    let pool = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(250))
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        JsonCodec,
        SubjectAuthProvider,
    )
}

async fn status(method: &str, path: &str) -> StatusCode {
    router()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("x-subject-id", "subject-1")
                .body(Body::from("{}"))
                .expect("request should build"),
        )
        .await
        .expect("router should respond")
        .status()
}

/// The control model: no `@@internal`, so every verb is routed.
/// `METHOD_NOT_ALLOWED`/`NOT_FOUND` here would mean the test harness
/// itself is broken rather than that suppression worked.
#[tokio::test]
async fn control_model_mounts_every_verb() {
    for (method, path) in [
        ("POST", "/public_notes"),
        ("PATCH", "/public_notes/1"),
        ("DELETE", "/public_notes/1"),
    ] {
        let status = status(method, path).await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path} should be routed on the control model"
        );
        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} should be routed on the control model"
        );
    }
}

/// The headline assertion: the three `@@internal` write verbs are not
/// mounted, so axum rejects them at the routing layer.
#[tokio::test]
async fn internal_write_verbs_are_not_routed() {
    for (method, path) in [
        ("POST", "/devices"),
        ("PATCH", "/devices/1"),
        ("DELETE", "/devices/1"),
    ] {
        assert_eq!(
            status(method, path).await,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path} must not be mounted for an @@internal action"
        );
    }
}

/// ...while the read verbs on the same model, which are *not*
/// `@@internal`, are still mounted. This is what distinguishes route
/// suppression from simply not generating the model.
#[tokio::test]
async fn non_internal_read_verbs_on_the_same_model_stay_routed() {
    for (method, path) in [("GET", "/devices"), ("GET", "/devices/1")] {
        let status = status(method, path).await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path} should still be routed"
        );
        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} should still be routed"
        );
    }
}

/// The other half of the spike, checked against the real generated
/// descriptor: suppressing the route must not suppress the policy.
/// If `@@internal` had been implemented by dropping the action
/// wholesale, these slots would be empty and server-side writes would
/// silently fail closed for the wrong reason.
#[test]
fn policies_survive_route_suppression() {
    let descriptor = cratestack_schema::DEVICE_MODEL;
    assert_eq!(
        descriptor.create_allow_policies.len(),
        1,
        "create policy must still be compiled for an @@internal action"
    );
    assert_eq!(
        descriptor.update_allow_policies.len(),
        1,
        "update policy must still be compiled for an @@internal action"
    );
    assert_eq!(
        descriptor.delete_allow_policies.len(),
        1,
        "delete policy must still be compiled for an @@internal action"
    );
}
