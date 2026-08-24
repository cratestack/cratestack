//! cratestack#413 — default request body limit + override mechanism.
//! Reuses `no_database_procedures.cstack` (the same `db = None` fixture
//! `no_database_procedures.rs` already exercises), so this stays scoped to
//! `cargo test -p cratestack-api` with no database involved — rejection
//! happens at the `Bytes` extractor, before any handler/DB code runs.
//!
//! `DEFAULT_BODY_LIMIT_BYTES` is 2 MiB — chosen to match axum's own
//! implicit `Bytes` default (see `crates/cratestack-core/src/limits.rs`'s
//! doc comment), so this file's default-limit tests use bodies clearly
//! above/below 2 MiB rather than right at the boundary, to stay robust
//! against small JSON-wrapper overhead shifting a body a few bytes either
//! side of the limit.
//!
//! The decisive test here is `raising body_limit_bytes actually raises the
//! ceiling` — this is the exact case that would have caught the
//! empirically-broken "re-layer `DefaultBodyLimit` after the fact" design
//! documented (and rejected) in
//! `docs/design/request-response-size-bounds.md` Decision 2: a body
//! between the default and the override succeeds against the router built
//! with the larger `body_limit_bytes` and fails against the one built with
//! the default, proving the override is a real, working parameter and not
//! a no-op.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn build_router(body_limit_bytes: usize) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::router(
        db,
        Procedures,
        (),
        JsonCodec,
        AllowAllAuth,
        body_limit_bytes,
    )
}

fn ping_body(message_len: usize) -> Vec<u8> {
    let message = "a".repeat(message_len);
    serde_json::to_vec(&serde_json::json!({ "args": { "message": message } })).unwrap()
}

async fn post_ping(router: cratestack::axum::Router, body: Vec<u8>) -> StatusCode {
    let response = router
        .oneshot(
            Request::post("/$procs/ping")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    response.status()
}

#[tokio::test]
async fn default_limit_does_not_break_an_ordinary_under_limit_request() {
    let router = build_router(cratestack::DEFAULT_BODY_LIMIT_BYTES);
    let status = post_ping(router, ping_body(4 * 1024)).await; // 4 KiB, comfortably under 2 MiB
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn default_limit_rejects_a_body_clearly_over_2_mebibytes() {
    let router = build_router(cratestack::DEFAULT_BODY_LIMIT_BYTES);
    // 3 MiB message — clearly over the 2 MiB default with margin to spare,
    // not right at the boundary (a body a few bytes past 2 MiB would also
    // fail, but wouldn't distinguish "the limit is enforced" from "the
    // limit happens to be smaller than expected").
    let status = post_ping(router, ping_body(3 * 1024 * 1024)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn raising_body_limit_bytes_actually_raises_the_ceiling() {
    // A body between the 2 MiB default and a 4 MiB override — 3 MiB,
    // comfortably on either side of both ceilings so JSON-wrapper
    // overhead can't accidentally cross a boundary and produce a
    // vacuous pass.
    let body = ping_body(3 * 1024 * 1024);

    let default_router = build_router(cratestack::DEFAULT_BODY_LIMIT_BYTES);
    let default_status = post_ping(default_router, body.clone()).await;
    assert_eq!(
        default_status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "control case: the same body must still be rejected at the default limit",
    );

    let overridden_router = build_router(4 * 1024 * 1024);
    let overridden_status = post_ping(overridden_router, body).await;
    assert_eq!(
        overridden_status,
        StatusCode::OK,
        "a consumer-supplied larger body_limit_bytes must actually take effect",
    );
}
